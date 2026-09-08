//! Device-local, resumable processing tasks. Source files are immutable inputs.
use super::*;
use crate::{control::Control, media};
use anyhow::{Context, Result, ensure};
use img_records::catalog::{Catalog, digest};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub input_id: String,
    pub path: PathBuf,
    pub content_hash: String,
    #[serde(default)]
    pub source_asset: Option<String>,
    #[serde(default)]
    pub annotations: Option<Vec<Annotation>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Output {
    pub task_id: String,
    pub input_id: String,
    pub output_order: usize,
    pub path: PathBuf,
    pub image: EncodedImage,
    #[serde(default)]
    pub remote: Option<serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessResult {
    pub task_id: String,
    pub input_id: String,
    pub input_order: usize,
    pub status: String,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub outputs: Vec<Output>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessingTask {
    pub version: u32,
    pub task_id: String,
    pub plan: ProcessingPlan,
    pub inputs: Vec<Input>,
    pub watermark: Option<Input>,
    pub output_dir: PathBuf,
    pub max_input_size: u64,
    pub files: Vec<ProcessResult>,
    #[serde(default)]
    pub output_names: Vec<String>,
}
impl Input {
    pub fn snapshot(path: &Path, limit: u64) -> Result<Self> {
        let path = path.canonicalize()?;
        let bytes = media::read_image(&path, limit)?;
        Ok(Self {
            input_id: uuid::Uuid::new_v4().to_string(),
            path,
            content_hash: digest(&bytes),
            source_asset: None,
            annotations: None,
        })
    }
    fn read(&self, limit: u64) -> Result<Vec<u8>> {
        let bytes = media::read_image(&self.path, limit)?;
        ensure!(
            digest(&bytes) == self.content_hash,
            "input changed; create a new processing task"
        );
        Ok(bytes)
    }
}
impl ProcessingTask {
    pub fn create(
        inputs: Vec<Input>,
        plan: ProcessingPlan,
        watermark: Option<Input>,
        output_dir: &Path,
        max_input_size: u64,
    ) -> Result<Self> {
        plan.validate()?;
        ensure!(
            !inputs.is_empty() && inputs.len() <= 10000,
            "processing requires 1–10000 inputs"
        );
        let mut ids = std::collections::HashSet::new();
        for input in &inputs {
            ensure!(ids.insert(&input.input_id), "duplicate processing input ID");
        }
        if let Some(mark) = &plan.watermark {
            ensure!(
                watermark.as_ref().is_some_and(
                    |input| mark.resource.is_empty() || input.content_hash == mark.resource
                ),
                "watermark resource missing on this device"
            );
        }
        std::fs::create_dir_all(output_dir)?;
        let output_dir = output_dir.canonicalize()?;
        let task_id = uuid::Uuid::new_v4().to_string();
        let output_names = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let stem = if plan.stitch.is_some() {
                    "stitched".into()
                } else {
                    input
                        .path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .chars()
                        .filter(|c| !c.is_control() && !"\\/:*?\"<>|".contains(*c))
                        .take(40)
                        .collect::<String>()
                };
                format!(
                    "{}-img-{}-{:04}",
                    stem.trim_matches([' ', '.']),
                    &task_id[..8],
                    index + 1
                )
            })
            .collect();
        Ok(Self {
            version: 1,
            task_id,
            plan,
            inputs,
            watermark,
            output_dir,
            max_input_size,
            files: vec![],
            output_names,
        })
    }
    pub fn load(catalog: &Catalog, id: &str) -> Result<Self> {
        ensure!(
            uuid::Uuid::parse_str(id).is_ok(),
            "invalid processing task ID"
        );
        let task: Self = serde_json::from_str(&catalog.task(&format!("process:{id}"))?)?;
        ensure!(
            task.task_id == id && task.version == 1,
            "unsupported processing task"
        );
        task.validate()?;
        Ok(task)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && uuid::Uuid::parse_str(&self.task_id).is_ok(),
            "unsupported processing task"
        );
        self.plan.validate()?;
        ensure!(
            !self.inputs.is_empty()
                && self.inputs.len() <= 10000
                && self.max_input_size > 0
                && self.max_input_size <= 256 << 20,
            "invalid processing inputs or size limit"
        );
        let mut ids = std::collections::HashSet::new();
        for input in &self.inputs {
            ensure!(
                !input.input_id.is_empty() && ids.insert(&input.input_id),
                "duplicate processing input ID"
            );
            if let Some(annotations) = &input.annotations {
                let mut plan = self.plan.clone();
                plan.annotations = annotations.clone();
                plan.validate()?;
            }
        }
        ensure!(
            self.output_names.is_empty()
                || (self.output_names.len() == self.inputs.len()
                    && self.output_names.iter().all(|name| !name.is_empty()
                        && !name
                            .chars()
                            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)))),
            "invalid processing output names"
        );
        Ok(())
    }
    pub fn save(&self, catalog: &Catalog) -> Result<()> {
        catalog.save_task(
            &format!("process:{}", self.task_id),
            "process",
            &serde_json::to_string(self)?,
        )
    }
    pub fn summary(&self) -> serde_json::Value {
        let outputs = self
            .files
            .iter()
            .flat_map(|f| f.outputs.iter())
            .collect::<Vec<_>>();
        serde_json::json!({"task_id":self.task_id,"files":self.files,"outputs":outputs,"complete":self.files.len()==if self.plan.stitch.is_some(){1}else{self.inputs.len()} && self.files.iter().all(|r|r.status=="complete")})
    }
}
fn write_output(output: &Path, data: &[u8], hash: &str) -> Result<()> {
    if let Ok(metadata) = output.symlink_metadata() {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "output already exists and is not a regular file"
        );
        ensure!(
            metadata.len() == data.len() as u64 && digest(&std::fs::read(output)?) == hash,
            "output already exists with different content"
        );
        return Ok(());
    }
    let mut file =
        tempfile::NamedTempFile::new_in(output.parent().context("output folder missing")?)?;
    file.write_all(data)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output).map_err(|e| e.error)?;
    Ok(())
}
fn one(
    task: &ProcessingTask,
    index: usize,
    mark: Option<&[u8]>,
    control: &Control,
) -> ProcessResult {
    let input = &task.inputs[index];
    let input_id = if task.plan.stitch.is_some() {
        "stitch"
    } else {
        &input.input_id
    };
    let mut outputs = vec![];
    let operation = (|| -> Result<()> {
        control.check()?;
        control.stage("processing", 1);
        let mut plan = task.plan.clone();
        if let Some(annotations) = &input.annotations {
            plan.annotations = annotations.clone();
        }
        let images = if plan.stitch.is_some() {
            super::render::stitch_expected(
                &task
                    .inputs
                    .iter()
                    .map(|input| input.path.clone())
                    .collect::<Vec<_>>(),
                &plan,
                mark,
                task.max_input_size,
                control,
                Some(
                    &task
                        .inputs
                        .iter()
                        .map(|input| input.content_hash.clone())
                        .collect::<Vec<_>>(),
                ),
            )?
        } else {
            process(&input.read(task.max_input_size)?, &plan, mark, control)?
        };
        for mut image in images {
            control.check()?;
            let name = task
                .output_names
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("{}-{:04}", task.task_id, index + 1));
            let path = task.output_dir.join(format!(
                "{}-{:04}.{}",
                name,
                image.output_order + 1,
                task.plan.encoding.format.extension()
            ));
            write_output(&path, &image.data, &image.content_hash)?;
            image.data = Vec::new();
            outputs.push(Output {
                task_id: task.task_id.clone(),
                input_id: input_id.into(),
                output_order: index * 1024 + image.output_order,
                path,
                image,
                remote: None,
            });
        }
        Ok(())
    })();
    let (status, error, error_code) = match operation {
        Ok(()) => ("complete", None, None),
        Err(error) => (
            if control.is_cancelled() {
                "cancelled"
            } else {
                "failed"
            },
            Some(error.to_string()),
            Some(
                serde_json::to_value(
                    crate::failure::Failure::from_error(&error, crate::failure::ErrorCode::Unknown)
                        .code,
                )
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
            ),
        ),
    };
    ProcessResult {
        task_id: task.task_id.clone(),
        input_id: input_id.into(),
        input_order: index,
        status: status.into(),
        error,
        error_code,
        outputs,
    }
}
/// A transient preview writes the same final encoded bytes without storing a task.
pub fn preview(task: &mut ProcessingTask, control: &Control) -> Result<()> {
    task.validate()?;
    let watermark = task
        .watermark
        .as_ref()
        .map(|input| input.read(20 << 20))
        .transpose()?;
    ensure!(
        task.plan.stitch.is_some() || task.inputs.len() == 1,
        "preview requires one image or one stitch task"
    );
    task.files = vec![one(task, 0, watermark.as_deref(), control)];
    Ok(())
}
/// Two workers by default, with the catalog written only on the coordinating thread.
pub fn run(catalog: &Catalog, task: &mut ProcessingTask, control: &Control) -> Result<()> {
    task.validate()?;
    ensure!(
        task.output_dir.canonicalize()? == task.output_dir,
        "output folder changed; create a new task"
    );
    let _lease =
        img_records::remote_lock::acquire(&catalog.root, "processing-task", &task.task_id, true)?;
    if let Some(saved) = catalog.task_optional(&format!("process:{}", task.task_id))? {
        let saved: ProcessingTask = serde_json::from_str(&saved)?;
        ensure!(
            saved.plan == task.plan
                && serde_json::to_value(&saved.inputs)? == serde_json::to_value(&task.inputs)?
                && saved.output_dir == task.output_dir
                && saved.output_names == task.output_names
                && saved.max_input_size == task.max_input_size
                && serde_json::to_value(&saved.watermark)?
                    == serde_json::to_value(&task.watermark)?,
            "saved processing task differs"
        );
        task.files = saved.files;
    }
    task.save(catalog)?;
    let watermark = task
        .watermark
        .as_ref()
        .map(|input| input.read(20 << 20))
        .transpose()?;
    let count = if task.plan.stitch.is_some() {
        1
    } else {
        task.inputs.len()
    };
    let jobs = (0..count)
        .filter(|i| {
            !task.files.iter().any(|r| {
                r.input_order == *i
                    && r.status == "complete"
                    && !r.outputs.is_empty()
                    && r.outputs.iter().all(|output| {
                        output.path.symlink_metadata().is_ok_and(|metadata| {
                            metadata.is_file()
                                && !metadata.file_type().is_symlink()
                                && metadata.len() == output.image.size
                                && metadata.len() <= 256 << 20
                        }) && std::fs::read(&output.path)
                            .is_ok_and(|bytes| digest(&bytes) == output.image.content_hash)
                    })
            })
        })
        .collect::<Vec<_>>();
    let snapshot = Arc::new(task.clone());
    let next = AtomicUsize::new(0);
    let mut save_error = None;
    std::thread::scope(|scope| {
        let (send, receive) = mpsc::channel();
        for _ in 0..2.min(jobs.len()) {
            let (send, snapshot, mark) = (send.clone(), snapshot.clone(), watermark.as_deref());
            let (jobs, next) = (&jobs, &next);
            scope.spawn(move || {
                loop {
                    if control.is_cancelled() {
                        break;
                    }
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&index) = jobs.get(at) else {
                        break;
                    };
                    if send.send(one(&snapshot, index, mark, control)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(send);
        let mut results: BTreeMap<usize, ProcessResult> = task
            .files
            .iter()
            .cloned()
            .map(|result| (result.input_order, result))
            .collect();
        for result in receive {
            results.insert(result.input_order, result);
            task.files = results.values().cloned().collect();
            if let Err(error) = task.save(catalog) {
                control.cancel();
                save_error = Some(error);
                break;
            }
        }
    });
    if let Some(error) = save_error {
        return Err(error.context(
            "processing outputs may exist, but task progress could not be saved; retry this task",
        ));
    }
    for index in jobs {
        if !task.files.iter().any(|result| result.input_order == index) {
            task.files.push(ProcessResult {
                task_id: task.task_id.clone(),
                input_id: if task.plan.stitch.is_some() {
                    "stitch".into()
                } else {
                    task.inputs[index].input_id.clone()
                },
                input_order: index,
                status: "cancelled".into(),
                error: None,
                error_code: Some("cancelled".into()),
                outputs: vec![],
            });
        }
    }
    task.files.sort_by_key(|result| result.input_order);
    task.save(catalog)
}

/// Reassemble decoded exported tiles so the displayed preview includes their actual encoding.
/// The preview PNG size is never used as an estimate of the exported files' sizes.
pub fn preview_artifact(task: &ProcessingTask) -> Result<serde_json::Value> {
    use image::{DynamicImage, RgbaImage};
    let outputs = &task.files.first().context("preview has no result")?.outputs;
    ensure!(!outputs.is_empty(), "preview has no output");
    if outputs.len() == 1 {
        return Ok(serde_json::to_value(&outputs[0])?);
    }
    let (columns, rows) = match task.plan.split {
        Some(Split::Grid { columns, rows }) => (columns as usize, rows as usize),
        Some(Split::Height { .. }) => (1, outputs.len()),
        None => anyhow::bail!("unexpected preview output count"),
    };
    ensure!(
        columns * rows == outputs.len(),
        "preview tile count mismatch"
    );
    let width = outputs[..columns].iter().try_fold(0u32, |sum, out| {
        sum.checked_add(out.image.width)
            .context("preview width overflow")
    })?;
    let height = outputs.iter().step_by(columns).try_fold(0u32, |sum, out| {
        sum.checked_add(out.image.height)
            .context("preview height overflow")
    })?;
    dimensions(width, height)?;
    let mut canvas = RgbaImage::new(width, height);
    let mut y = 0;
    for row in outputs.chunks(columns) {
        let mut x = 0;
        for output in row {
            let bytes = std::fs::read(&output.path)?;
            ensure!(
                digest(&bytes) == output.image.content_hash,
                "preview output changed"
            );
            let image = image::load_from_memory(&bytes)?;
            image::imageops::overlay(&mut canvas, &image, i64::from(x), i64::from(y));
            x += output.image.width;
        }
        y += row[0].image.height;
    }
    let path = task
        .output_dir
        .join(format!("{}-preview.png", task.task_id));
    let mut data = std::io::Cursor::new(vec![]);
    DynamicImage::ImageRgba8(canvas).write_to(&mut data, image::ImageFormat::Png)?;
    let bytes = data.into_inner();
    write_output(&path, &bytes, &digest(&bytes))?;
    let size = outputs.iter().map(|out| out.image.size).sum::<u64>();
    let original_size = outputs[0].image.original_size;
    Ok(
        serde_json::json!({"path":path,"preview_content_type":"image/png","output_count":outputs.len(),"image":{"width":width,"height":height,"size":size,"original_size":original_size,"saving_percent":if original_size>0{(1.-size as f64/original_size as f64)*100.}else{0.},"content_type":outputs[0].image.content_type,"has_alpha":outputs.iter().any(|o|o.image.has_alpha),"target_met":if outputs.iter().any(|o|o.image.target_met==Some(false)){Some(false)}else{outputs[0].image.target_met}}}),
    )
}
