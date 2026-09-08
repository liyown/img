use super::*;
use crate::tool_editor::Edits;
use img_records::processing::{self as plan, Annotation, ProcessingPlan, Shape};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Instant};
#[path = "tools_actions.rs"]
mod actions;
#[path = "tools_controls.rs"]
mod controls;
#[path = "tools_canvas.rs"]
mod editing;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Convert,
    Geometry,
    Annotate,
    Split,
    Stitch,
    Watermark,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkTool {
    Select,
    Arrow,
    Rectangle,
    Text,
    Step,
    Redact,
}
#[derive(Clone)]
struct ToolInput {
    id: String,
    path: PathBuf,
    hash: String,
    source_asset: Option<String>,
    edits: Edits,
}
#[derive(Clone)]
struct Preview {
    lease: Arc<img_records::cache::CacheLease>,
    metadata: serde_json::Value,
}
pub(super) struct Tools {
    root: PathBuf,
    engine: PathBuf,
    inputs: Vec<ToolInput>,
    selected: usize,
    fields: HashMap<&'static str, Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
    plan: ProcessingPlan,
    tool: Tool,
    mark_tool: MarkTool,
    selected_annotation: Option<String>,
    stitch_edits: Edits,
    preview: Option<Preview>,
    results: Vec<serde_json::Value>,
    result_selected: Option<usize>,
    task_id: Option<String>,
    busy: bool,
    preview_busy: bool,
    importing: bool,
    revision: u64,
    error: Option<String>,
    control: Option<Control>,
    preview_control: Option<Control>,
    watermark: Option<(PathBuf, String)>,
    thumbnails: Entity<crate::thumbnails::ThumbnailCache>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<(plan::Point, plan::Point)>,
    crop_editing: bool,
    crop_ratio: Option<f32>,
    resize_mode: u8,
    compression_mode: u8,
    split_mode: u8,
    focus: FocusHandle,
    stopping: bool,
    copied_until: Option<Instant>,
}
impl Tools {
    pub fn new(
        root: PathBuf,
        engine: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut fields = HashMap::new();
        let mut subscriptions = vec![];
        for (key, value) in [
            ("quality", "85"),
            ("target", "200"),
            ("jpeg_bg", "#ffffff"),
            ("width", "1200"),
            ("height", "0"),
            ("edge", "1600"),
            ("crop_x", "0"),
            ("crop_y", "0"),
            ("crop_w", "100"),
            ("crop_h", "100"),
            ("text", "文字"),
            ("font_size", "32"),
            ("color", "#ef4444"),
            ("stroke", "4"),
            ("split_height", "1200"),
            ("rows", "2"),
            ("cols", "2"),
            ("spacing", "12"),
            ("cross_size", "0"),
            ("stitch_bg", "#ffffff"),
            ("margin", "16"),
            ("watermark_scale", "20"),
            ("opacity", "60"),
        ] {
            let field = cx.new(|cx| {
                InputState::new(window, cx).default_value(if key == "text" {
                    crate::i18n::text(value)
                } else {
                    value.into()
                })
            });
            subscriptions.push(cx.subscribe(&field, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.schedule_preview(cx);
                }
            }));
            fields.insert(key, field);
        }
        Self {
            thumbnails: crate::thumbnails::ThumbnailCache::preview(root.clone(), cx),
            root,
            engine,
            inputs: vec![],
            selected: 0,
            fields,
            _subscriptions: subscriptions,
            plan: ProcessingPlan::default(),
            tool: Tool::Convert,
            mark_tool: MarkTool::Arrow,
            selected_annotation: None,
            stitch_edits: Edits::default(),
            preview: None,
            results: vec![],
            result_selected: None,
            task_id: None,
            busy: false,
            preview_busy: false,
            importing: false,
            revision: 0,
            error: None,
            control: None,
            preview_control: None,
            watermark: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
            drag: None,
            crop_editing: false,
            crop_ratio: None,
            resize_mode: 0,
            compression_mode: 0,
            split_mode: 0,
            focus: cx.focus_handle(),
            stopping: false,
            copied_until: None,
        }
    }
    pub fn stop(&mut self) -> Vec<Control> {
        self.stopping = true;
        let controls = self
            .control
            .take()
            .into_iter()
            .chain(self.preview_control.take())
            .collect::<Vec<_>>();
        for control in &controls {
            control.stop(engine::CANCEL);
        }
        controls
    }
    pub fn add_paths(
        &mut self,
        paths: Vec<PathBuf>,
        source_assets: HashMap<PathBuf, String>,
        cx: &mut Context<Self>,
    ) {
        if self.importing || self.stopping {
            return;
        }
        self.importing = true;
        let current = self.inputs.len();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<Vec<ToolInput>> {
                let files = img_records::files::collect(&paths, true, 1000)?;
                anyhow::ensure!(current + files.len() <= 1000, "一次最多处理 1000 张图片");
                files
                    .into_iter()
                    .map(|path| {
                        let metadata = path.metadata()?;
                        anyhow::ensure!(metadata.len() <= 256 << 20, "图片文件超过 256 MiB");
                        let bytes = std::fs::read(&path)?;
                        Ok(ToolInput {
                            id: uuid::Uuid::new_v4().to_string(),
                            source_asset: source_assets.get(&path).cloned(),
                            path,
                            hash: img_records::catalog::digest(&bytes),
                            edits: Edits::default(),
                        })
                    })
                    .collect()
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.importing = false;
                match result {
                    Ok(inputs) => {
                        for input in inputs {
                            if !this.inputs.iter().any(|i| i.path == input.path) {
                                this.inputs.push(input);
                            }
                        }
                        this.schedule_preview(cx);
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub fn choose(&mut self, cx: &mut Context<Self>) {
        let task = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some(crate::i18n::text("选择图片或文件夹")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = task.await {
                let _ = this.update(cx, |this, cx| this.add_paths(paths, HashMap::new(), cx));
            }
        })
        .detach();
    }
    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            for entry in item.entries() {
                match entry {
                    ClipboardEntry::ExternalPaths(paths) => {
                        self.add_paths(paths.0.to_vec(), HashMap::new(), cx);
                        return;
                    }
                    ClipboardEntry::Image(image) => {
                        let bytes = image.bytes().to_vec();
                        let root = self.root.clone();
                        let task = cx.background_executor().spawn(async move {
                            (|| -> anyhow::Result<PathBuf> {
                                let directory = root.join("tool-inputs");
                                std::fs::create_dir_all(&directory)?;
                                let path = directory.join(format!("{}.png", uuid::Uuid::new_v4()));
                                std::fs::write(&path, bytes)?;
                                Ok(path)
                            })()
                        });
                        cx.spawn(async move |this, cx| {
                            let result = task.await;
                            let _ = this.update(cx, |this, cx| match result {
                                Ok(path) => this.add_paths(vec![path], HashMap::new(), cx),
                                Err(error) => {
                                    this.error = Some(error.to_string());
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                        return;
                    }
                    _ => {}
                }
            }
        }
        self.error = Some("请先复制图片或图片文件".into());
        cx.notify();
    }
    fn value(&self, key: &str, cx: &App) -> String {
        self.fields[key].read(cx).value().trim().to_owned()
    }
    fn number(&self, key: &str, label: &str, cx: &App) -> anyhow::Result<u32> {
        self.value(key, cx)
            .parse()
            .map_err(|_| anyhow::anyhow!("{label}需要填写非负整数"))
    }
    fn color(&self, key: &str, cx: &App) -> anyhow::Result<[u8; 4]> {
        let text = self.value(key, cx);
        let text = text.trim_start_matches('#');
        anyhow::ensure!(
            text.len() == 6 && text.bytes().all(|c| c.is_ascii_hexdigit()),
            "颜色请填写六位十六进制值，例如 #ffffff"
        );
        let number = u32::from_str_radix(text, 16)?;
        Ok([(number >> 16) as u8, (number >> 8) as u8, number as u8, 255])
    }
    fn edits(&self) -> Option<&Edits> {
        if self.plan.stitch.is_some() {
            Some(&self.stitch_edits)
        } else {
            self.inputs.get(self.selected).map(|input| &input.edits)
        }
    }
    fn edits_mut(&mut self) -> Option<&mut Edits> {
        if self.plan.stitch.is_some() {
            Some(&mut self.stitch_edits)
        } else {
            self.inputs
                .get_mut(self.selected)
                .map(|input| &mut input.edits)
        }
    }
    fn recipe(&self, preview: bool, cx: &App) -> anyhow::Result<ProcessingPlan> {
        let mut plan = self.plan.clone();
        plan.encoding.compression = match self.compression_mode {
            1 => plan::Compression::Target {
                bytes: u64::from(self.number("target", "目标体积", cx)?) * 1024,
            },
            2 => plan::Compression::Lossless,
            _ => plan::Compression::Quality {
                quality: u8::try_from(self.number("quality", "质量", cx)?)?,
            },
        };
        let color = self.color("jpeg_bg", cx)?;
        plan.encoding.jpeg_background = [color[0], color[1], color[2]];
        plan.geometry.resize = match self.resize_mode {
            1 => Some(plan::Resize {
                max_edge: self.number("edge", "最长边", cx)?,
                allow_upscale: self
                    .plan
                    .geometry
                    .resize
                    .as_ref()
                    .is_some_and(|r| r.allow_upscale),
                ..Default::default()
            }),
            2 => Some(plan::Resize {
                width: self.number("width", "宽度", cx)?,
                height: self.number("height", "高度", cx)?,
                keep_aspect: self
                    .plan
                    .geometry
                    .resize
                    .as_ref()
                    .is_none_or(|r| r.keep_aspect),
                allow_upscale: self
                    .plan
                    .geometry
                    .resize
                    .as_ref()
                    .is_some_and(|r| r.allow_upscale),
                max_edge: 0,
            }),
            _ => None,
        };
        if self.crop_editing && preview {
            plan.geometry.crop = None;
            plan.geometry.quarter_turns = 0;
            plan.geometry.flip_horizontal = false;
            plan.geometry.flip_vertical = false;
            plan.annotations.clear();
        } else {
            plan.annotations = self
                .edits()
                .map(|edits| edits.annotations.clone())
                .unwrap_or_default();
        }
        plan.split = match self.split_mode {
            1 => Some(plan::Split::Height {
                height: self.number("split_height", "切分高度", cx)?,
            }),
            2 => Some(plan::Split::Grid {
                rows: self.number("rows", "行数", cx)?,
                columns: self.number("cols", "列数", cx)?,
            }),
            _ => None,
        };
        if let Some(stitch) = &mut plan.stitch {
            stitch.spacing = self.number("spacing", "间距", cx)?;
            let size = self.number("cross_size", "统一尺寸", cx)?;
            stitch.cross_size = (size > 0).then_some(size);
            if stitch.background[3] != 0 {
                stitch.background = self.color("stitch_bg", cx)?;
            }
        }
        if let Some(mark) = &mut plan.watermark {
            mark.margin = self.number("margin", "边距", cx)?;
            mark.scale = self.number("watermark_scale", "水印比例", cx)? as f32 / 100.;
            mark.opacity = self.number("opacity", "透明度", cx)? as f32 / 100.;
            let (_, hash) = self
                .watermark
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("此预设需要水印图片，请在本机选择同一张图片。"))?;
            anyhow::ensure!(
                mark.resource.is_empty() || &mark.resource == hash,
                "水印图片与预设不一致，请选择原水印或移除水印后重新设置"
            );
            mark.resource = hash.clone();
        }
        plan.validate()?;
        Ok(plan)
    }
    fn manifest(&self, preview: bool) -> Vec<serde_json::Value> {
        self.inputs.iter().enumerate().filter(|(i,_)|!preview || self.plan.stitch.is_some() || *i==self.selected).map(|(_,input)|serde_json::json!({"input_id":input.id,"path":input.path,"content_hash":input.hash,"source_asset":input.source_asset,"annotations":if self.plan.stitch.is_none() && !self.crop_editing{Some(input.edits.annotations.clone())}else{None}})).collect()
    }
    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        self.copied_until = None;
        self.result_selected = None;
        let revision = self.revision;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.revision == revision && !this.stopping {
                    this.render_preview(cx);
                }
            });
        })
        .detach();
        cx.notify();
    }
    fn render_preview(&mut self, cx: &mut Context<Self>) {
        if self.inputs.is_empty() {
            self.preview = None;
            return;
        }
        let plan = match self.recipe(true, cx) {
            Ok(plan) => plan,
            Err(error) => {
                self.error = Some(error.to_string());
                if let Some(control) = self.preview_control.take() {
                    control.stop(engine::CANCEL);
                }
                self.preview_busy = false;
                cx.notify();
                return;
            }
        };
        if let Some(control) = self.preview_control.take() {
            control.stop(engine::CANCEL);
        }
        self.preview_busy = true;
        self.error = None;
        let control = Control::default();
        self.preview_control = Some(control.clone());
        let revision = self.revision;
        let root = self.root.clone();
        let engine = self.engine.clone();
        let inputs = self.manifest(true);
        let watermark = self.watermark.as_ref().map(|(path, _)| path.clone());
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<Preview> {
                let scratch = tempfile::tempdir()?;
                let recipe = scratch.path().join("recipe.json");
                let manifest = scratch.path().join("inputs.json");
                std::fs::write(&recipe, serde_json::to_vec(&plan)?)?;
                std::fs::write(&manifest, serde_json::to_vec(&inputs)?)?;
                let mut command = std::process::Command::new(engine);
                command
                    .args(["process", "--recipe"])
                    .arg(recipe)
                    .arg("--inputs-manifest")
                    .arg(manifest)
                    .arg("--output-dir")
                    .arg(scratch.path())
                    .arg("--preview")
                    .env("IMG_DATA_DIR", &root);
                if let Some(watermark) = watermark {
                    command.arg("--watermark").arg(watermark);
                }
                let output = engine::run(command, &control.child())?;
                anyhow::ensure!(output.stopped == 0, "预览已取消");
                let result: serde_json::Value = serde_json::from_slice(&output.stdout)
                    .map_err(|_| anyhow::anyhow!("无法生成预览，请检查图片和参数"))?;
                if result["complete"] != true {
                    anyhow::bail!(
                        "{}",
                        result["files"][0]["error"]
                            .as_str()
                            .or(result["error"].as_str())
                            .unwrap_or("无法生成预览，请检查图片和参数")
                    );
                }
                let metadata = result
                    .get("preview")
                    .unwrap_or(&result["outputs"][0])
                    .clone();
                let path = metadata["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("预览文件缺失"))?;
                let bytes = std::fs::read(path)?;
                let cache = img_records::cache::Cache::open(&root)?;
                let key = cache.put(&bytes)?;
                let lease = Arc::new(cache.lease(&key)?);
                cache.trim(cache.limit()?)?;
                Ok(Preview { lease, metadata })
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.revision != revision {
                    return;
                }
                this.preview_busy = false;
                this.preview_control = None;
                match result {
                    Ok(preview) => this.preview = Some(preview),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn export(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.inputs.is_empty() {
            return;
        }
        let plan = match self.recipe(false, cx) {
            Ok(plan) => plan,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let inputs = self.manifest(false);
        let watermark = self.watermark.as_ref().map(|(path, _)| path.clone());
        let task = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::i18n::text("选择结果保存目录")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = task.await
                && let Some(path) = paths.first()
            {
                let _ = this.update(cx, |this, cx| {
                    this.run_export(path.clone(), plan, inputs, watermark, cx)
                });
            }
        })
        .detach();
    }
    fn run_export(
        &mut self,
        directory: PathBuf,
        plan: ProcessingPlan,
        inputs: Vec<serde_json::Value>,
        watermark: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.busy || self.stopping {
            return;
        }
        self.busy = true;
        self.error = None;
        let root = self.root.clone();
        let engine = self.engine.clone();
        let control = Control::default();
        self.control = Some(control.clone());
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<serde_json::Value> {
                let scratch = tempfile::tempdir()?;
                let recipe = scratch.path().join("recipe.json");
                let manifest = scratch.path().join("inputs.json");
                std::fs::write(&recipe, serde_json::to_vec(&plan)?)?;
                std::fs::write(&manifest, serde_json::to_vec(&inputs)?)?;
                let mut command = std::process::Command::new(engine);
                command
                    .arg("process")
                    .arg("--recipe")
                    .arg(recipe)
                    .arg("--inputs-manifest")
                    .arg(manifest)
                    .arg("--output-dir")
                    .arg(directory)
                    .env("IMG_DATA_DIR", root);
                if let Some(watermark) = watermark {
                    command.arg("--watermark").arg(watermark);
                }
                let output = engine::run(command, &control.child())?;
                anyhow::ensure!(output.stopped == 0, "处理已取消，可在任务面板继续");
                serde_json::from_slice(&output.stdout)
                    .map_err(|_| anyhow::anyhow!("处理失败，请检查文件和参数"))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                match result {
                    Ok(result) => {
                        this.task_id = result["task_id"].as_str().map(str::to_owned);
                        this.results = result["outputs"].as_array().cloned().unwrap_or_default();
                        this.result_selected = None;
                        if result["complete"] != true {
                            this.error = Some(
                                result["files"]
                                    .as_array()
                                    .and_then(|files| {
                                        files.iter().find_map(|file| file["error"].as_str())
                                    })
                                    .or(result["error"].as_str())
                                    .unwrap_or("部分图片未完成，可在任务面板重试")
                                    .into(),
                            );
                        }
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn select_result(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(metadata) = self.results.get(index).cloned() else {
            return;
        };
        let Some(path) = metadata["path"].as_str().map(PathBuf::from) else {
            return;
        };
        self.result_selected = Some(index);
        self.revision += 1;
        let revision = self.revision;
        self.preview_busy = false;
        self.copied_until = None;
        if let Some(control) = self.preview_control.take() {
            control.stop(engine::CANCEL);
        }
        let root = self.root.clone();
        let selected = metadata.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<Preview> {
                let bytes = std::fs::read(path)?;
                anyhow::ensure!(
                    img_records::catalog::digest(&bytes)
                        == selected["image"]["content_hash"].as_str().unwrap_or(""),
                    "结果文件已经变化"
                );
                let cache = img_records::cache::Cache::open(&root)?;
                let key = cache.put(&bytes)?;
                Ok(Preview {
                    lease: Arc::new(cache.lease(&key)?),
                    metadata: selected,
                })
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.result_selected != Some(index) || this.revision != revision {
                    return;
                }
                match result {
                    Ok(preview) => this.preview = Some(preview),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn copy_image(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = self.preview.clone() else {
            return;
        };
        let task = cx.background_executor().spawn(async move {
            std::fs::read(&preview.lease.path).map(|bytes| {
                (
                    bytes,
                    preview.metadata["preview_content_type"]
                        .as_str()
                        .or(preview.metadata["image"]["content_type"].as_str())
                        .unwrap_or("image/png")
                        .to_owned(),
                )
            })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((bytes, ct)) => {
                        let format = match ct.as_str() {
                            "image/jpeg" => ImageFormat::Jpeg,
                            "image/webp" => ImageFormat::Webp,
                            _ => ImageFormat::Png,
                        };
                        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                            format, bytes,
                        )));
                        this.copied_until = Some(Instant::now() + Duration::from_secs(2));
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(Duration::from_secs(2)).await;
                            let _ = this.update(cx, |_, cx| cx.notify());
                        })
                        .detach();
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
impl Drop for Tools {
    fn drop(&mut self) {
        self.stop();
    }
}
