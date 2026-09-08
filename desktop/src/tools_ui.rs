use super::*;
use img_records::processing::{self as plan, ProcessingPlan};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Instant};
#[path = "tools_actions.rs"]
mod actions;
#[path = "tools_controls.rs"]
mod controls;
#[path = "tools_canvas.rs"]
mod editing;
fn supports_recipe(plan: &ProcessingPlan) -> bool {
    plan.annotations.is_empty()
        && plan.split.is_none()
        && plan.stitch.is_none()
        && plan.watermark.is_none()
        && plan.validate().is_ok()
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Convert,
    Geometry,
}
#[derive(Clone)]
struct ToolInput {
    id: String,
    path: PathBuf,
    hash: String,
    source_asset: Option<String>,
}
#[derive(Clone)]
struct Preview {
    lease: Arc<img_records::cache::CacheLease>,
    metadata: serde_json::Value,
}
pub(super) struct Tools {
    root: PathBuf,
    engine: PathBuf,
    input: Option<ToolInput>,
    fields: HashMap<&'static str, Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
    plan: ProcessingPlan,
    tool: Tool,
    preview: Option<Preview>,
    preview_cache: VecDeque<(String, Preview)>,
    active_preview_key: Option<String>,
    result: Option<serde_json::Value>,
    task_id: Option<String>,
    saved_to_user: bool,
    busy: bool,
    preview_busy: bool,
    importing: bool,
    revision: u64,
    error: Option<String>,
    control: Option<Control>,
    preview_control: Option<Control>,
    thumbnails: Entity<crate::thumbnails::ThumbnailCache>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<(plan::Point, plan::Point)>,
    crop_editing: bool,
    crop_ratio: Option<f32>,
    resize_mode: u8,
    compression_mode: u8,
    focus: FocusHandle,
    inspector_scroll: ScrollHandle,
    stopping: bool,
    copied_until: Option<Instant>,
}
impl Tools {
    pub fn set_tool(&mut self, geometry: bool, cx: &mut Context<Self>) {
        let tool = if geometry {
            Tool::Geometry
        } else {
            Tool::Convert
        };
        if self.tool != tool {
            self.inspector_scroll.set_offset(point(px(0.), px(0.)));
            self.tool = tool;
            self.crop_editing = false;
            self.drag = None;
            self.schedule_preview(cx);
        }
    }
    pub fn status(&self) -> String {
        if self.busy {
            "处理中…"
        } else if self.importing {
            "正在读取…"
        } else if self.input.is_some() {
            "保留原文件"
        } else {
            "请选择一张图片"
        }
        .into()
    }
    fn clear_input(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.importing {
            return;
        }
        self.input = None;
        self.result = None;
        self.task_id = None;
        self.preview_cache.clear();
        self.crop_editing = false;
        self.drag = None;
        self.plan.geometry.crop = None;
        self.schedule_preview(cx);
    }
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
        ] {
            let field = cx.new(|cx| InputState::new(window, cx).default_value(value));
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
            input: None,
            fields,
            _subscriptions: subscriptions,
            plan: ProcessingPlan::default(),
            tool: Tool::Convert,
            preview: None,
            preview_cache: VecDeque::new(),
            active_preview_key: None,
            result: None,
            task_id: None,
            saved_to_user: false,
            busy: false,
            preview_busy: false,
            importing: false,
            revision: 0,
            error: None,
            control: None,
            preview_control: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
            drag: None,
            crop_editing: false,
            crop_ratio: None,
            resize_mode: 0,
            compression_mode: 0,
            focus: cx.focus_handle(),
            inspector_scroll: ScrollHandle::new(),
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
        if self.importing || self.stopping || self.busy {
            return;
        }
        if paths.len() != 1 {
            self.error = Some("每次处理一张图片，请只选择一个图片文件".into());
            cx.notify();
            return;
        }
        self.importing = true;
        let path = paths.into_iter().next().unwrap();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<ToolInput> {
                let metadata = path.metadata()?;
                anyhow::ensure!(metadata.is_file(), "请选择图片文件，不支持导入文件夹");
                anyhow::ensure!(metadata.len() <= 256 << 20, "图片文件超过 256 MiB");
                let bytes = std::fs::read(&path)?;
                Ok(ToolInput {
                    id: uuid::Uuid::new_v4().to_string(),
                    source_asset: source_assets.get(&path).cloned(),
                    path,
                    hash: img_records::catalog::digest(&bytes),
                })
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.importing = false;
                match result {
                    Ok(input) => {
                        this.input = Some(input);
                        this.result = None;
                        this.task_id = None;
                        this.preview = None;
                        this.preview_cache.clear();
                        this.crop_editing = false;
                        this.drag = None;
                        this.plan.geometry.crop = None;
                        this.plan.geometry.quarter_turns = 0;
                        this.plan.geometry.flip_horizontal = false;
                        this.plan.geometry.flip_vertical = false;
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
        if self.busy || self.importing || self.stopping {
            return;
        }
        let task = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::i18n::text("选择图片")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = task.await {
                let _ = this.update(cx, |this, cx| this.add_paths(paths, HashMap::new(), cx));
            }
        })
        .detach();
    }
    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if self.importing || self.busy || self.stopping {
            return;
        }
        if let Some(item) = cx.read_from_clipboard() {
            for entry in item.entries() {
                match entry {
                    ClipboardEntry::ExternalPaths(paths) => {
                        self.add_paths(paths.0.to_vec(), HashMap::new(), cx);
                        return;
                    }
                    ClipboardEntry::Image(image) => {
                        self.importing = true;
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
                            let _ = this.update(cx, |this, cx| {
                                this.importing = false;
                                match result {
                                    Ok(path) => this.add_paths(vec![path], HashMap::new(), cx),
                                    Err(error) => {
                                        this.error = Some(error.to_string());
                                        cx.notify();
                                    }
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
    fn recipe(&self, preview: bool, cx: &App) -> anyhow::Result<ProcessingPlan> {
        anyhow::ensure!(
            supports_recipe(&self.plan),
            "此预设包含当前工具不支持的操作"
        );
        let mut plan = self.plan.clone();
        plan.encoding.compression = match self.compression_mode {
            1 => plan::Compression::Target {
                bytes: u64::from(self.number("target", "目标体积", cx)?) * 1024,
            },
            2 => plan::Compression::Lossless,
            _ if plan.encoding.format == plan::Format::Png => plan::Compression::Lossless,
            _ => plan::Compression::Quality {
                quality: u8::try_from(self.number("quality", "质量", cx)?)?,
            },
        };
        if plan.encoding.format == plan::Format::Jpeg {
            let color = self.color("jpeg_bg", cx)?;
            plan.encoding.jpeg_background = [color[0], color[1], color[2]];
        }
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
        }
        plan.validate()?;
        Ok(plan)
    }
    fn manifest(&self) -> Vec<serde_json::Value> {
        self.input
            .iter()
            .map(|input| {
                serde_json::json!({
                    "input_id": input.id, "path": input.path,
                    "content_hash": input.hash, "source_asset": input.source_asset
                })
            })
            .collect()
    }
    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        let key = self
            .recipe(true, cx)
            .ok()
            .and_then(|plan| self.preview_key(&plan).ok());
        if key.is_some() && key == self.active_preview_key {
            cx.notify();
            return;
        }
        self.revision += 1;
        self.copied_until = None;
        if let Some(control) = self.preview_control.take() {
            control.stop(engine::CANCEL);
        }
        self.active_preview_key = None;
        self.result = None;
        self.task_id = None;
        self.saved_to_user = false;
        if self.input.is_none() {
            self.preview = None;
            self.preview_busy = false;
            self.error = None;
            cx.notify();
            return;
        }
        if let Some(key) = &key
            && let Some(index) = self
                .preview_cache
                .iter()
                .position(|(cached, _)| cached == key)
        {
            let entry = self.preview_cache.remove(index).unwrap();
            self.preview = Some(entry.1.clone());
            self.preview_cache.push_back(entry);
            self.preview_busy = false;
            self.error = None;
            self.active_preview_key = Some(key.clone());
            cx.notify();
            return;
        }
        self.preview_busy = self.input.is_some();
        let revision = self.revision;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
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
    fn preview_key(&self, plan: &ProcessingPlan) -> anyhow::Result<String> {
        Ok(img_records::catalog::digest(&serde_json::to_vec(&(
            plan,
            self.manifest(),
        ))?))
    }
    fn render_preview(&mut self, cx: &mut Context<Self>) {
        if self.input.is_none() {
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
        let key = self
            .preview_key(&plan)
            .expect("validated processing plan is serializable");
        self.active_preview_key = Some(key.clone());
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
        let inputs = self.manifest();
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
                let output = engine::run_json(command, &control.child())?;
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
                if let Ok(preview) = &result {
                    this.preview_cache.retain(|(cached, _)| cached != &key);
                    this.preview_cache.push_back((key, preview.clone()));
                    while this.preview_cache.len() > 8 {
                        this.preview_cache.pop_front();
                    }
                }
                if this.revision != revision {
                    return;
                }
                this.preview_busy = false;
                this.preview_control = None;
                match result {
                    Ok(preview) => this.preview = Some(preview),
                    Err(error) => {
                        this.active_preview_key = None;
                        this.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn export(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.input.is_none() {
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
        let inputs = self.manifest();
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
                    this.run_export(path.clone(), plan, inputs, None, cx)
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
        publish_window: Option<AnyWindowHandle>,
        cx: &mut Context<Self>,
    ) {
        if self.busy || self.stopping {
            return;
        }
        self.busy = true;
        self.error = None;
        let export_key = img_records::catalog::digest(
            &serde_json::to_vec(&(&plan, &inputs)).expect("validated processing plan"),
        );
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
                let output = engine::run_json(command, &control.child())?;
                anyhow::ensure!(output.stopped == 0, "处理已取消，可在任务面板继续");
                serde_json::from_slice(&output.stdout)
                    .map_err(|_| anyhow::anyhow!("处理失败，请检查文件和参数"))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let publish = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                let mut publish = false;
                match result {
                    Ok(result) => {
                        if this
                            .recipe(false, cx)
                            .and_then(|plan| this.preview_key(&plan))
                            .is_ok_and(|key| key == export_key)
                        {
                            this.task_id = result["task_id"].as_str().map(str::to_owned);
                            this.result = result["outputs"]
                                .as_array()
                                .and_then(|outputs| outputs.first())
                                .cloned();
                            this.saved_to_user = publish_window.is_none() && this.result.is_some();
                            publish = result["complete"] == true
                                && this.task_id.is_some()
                                && this.result.is_some()
                                && !this.stopping;
                        }
                        if result["complete"] != true {
                            this.error = Some(
                                result["files"]
                                    .as_array()
                                    .and_then(|files| {
                                        files.iter().find_map(|file| file["error"].as_str())
                                    })
                                    .or(result["error"].as_str())
                                    .unwrap_or("图片处理未完成，可在任务面板重试")
                                    .into(),
                            );
                        }
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
                publish
            });
            if publish.unwrap_or(false)
                && let Some(window) = publish_window
            {
                let _ = window.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |this, cx| this.publish(window, cx));
                });
            }
        })
        .detach();
        cx.notify();
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
