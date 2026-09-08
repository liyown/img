use super::*;
use img_records::catalog::Catalog;
pub(super) struct PublishDialog {
    root: PathBuf,
    engine: PathBuf,
    parent: WeakEntity<Tools>,
    task: String,
    providers: Vec<String>,
    provider: String,
    prefix: Entity<InputState>,
    busy: bool,
    report: Option<serde_json::Value>,
    error: Option<String>,
    control: Option<Control>,
}
impl Tools {
    pub(super) fn publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self.task_id.clone() else {
            return;
        };
        let root = self.root.clone();
        let engine = self.engine.clone();
        let parent = cx.weak_entity();
        let providers = storage::configured_providers()
            .map(|(providers, _)| {
                providers
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let provider = providers.first().cloned().unwrap_or_default();
        let panel = cx.new(|cx| {
            let prefix = cx.new(|cx| InputState::new(window, cx).default_value("processed"));
            PublishDialog {
                root,
                engine,
                parent,
                task,
                providers,
                provider,
                prefix,
                busy: false,
                report: None,
                error: None,
                control: None,
            }
        });
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .title(crate::i18n::text("上传处理结果"))
                .w(px(
                    (f32::from(window.viewport_size().width) - 80.).clamp(280., 600.)
                ))
                .child(panel.clone())
        });
    }
    pub(super) fn presets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = cx.weak_entity();
        let panel = cx.new(|cx| PresetDialog::new(parent, self.root.clone(), window, cx));
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .title(crate::i18n::text("处理预设"))
                .w(px(
                    (f32::from(window.viewport_size().width) - 80.).clamp(280., 520.)
                ))
                .child(panel.clone())
        });
    }
}
impl PublishDialog {
    fn run(&mut self, retry: bool, cx: &mut Context<Self>) {
        if self.busy || self.provider.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        let control = Control::default();
        self.control = Some(control.clone());
        let _ = self.parent.update(cx, |parent, cx| {
            parent.busy = true;
            parent.control = Some(control.clone());
            cx.notify();
        });
        let (root, engine, provider, task, prefix) = (
            self.root.clone(),
            self.engine.clone(),
            self.provider.clone(),
            self.task.clone(),
            self.prefix.read(cx).value().trim().to_owned(),
        );
        let saved = self
            .report
            .as_ref()
            .and_then(|value| value["task_id"].as_str())
            .map(str::to_owned);
        let parent = self.parent.clone();
        let job = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<serde_json::Value> {
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .arg("tasks")
                    .env("IMG_DATA_DIR", root);
                if retry {
                    command.arg("retry").arg(format!(
                        "publish:{}",
                        saved.ok_or_else(|| anyhow::anyhow!("上传任务不存在"))?
                    ));
                } else {
                    command.args([
                        "upload",
                        &task,
                        "--provider",
                        &provider,
                        "--prefix",
                        &prefix,
                    ]);
                }
                let output = engine::run(command, &control.child())?;
                anyhow::ensure!(output.stopped == 0, "上传已暂停，可在任务面板继续");
                let result: serde_json::Value = serde_json::from_slice(&output.stdout)
                    .map_err(|_| anyhow::anyhow!("上传失败，请检查存储源和连接"))?;
                if let Some(error) = result["error"].as_str() {
                    anyhow::bail!("{error}");
                }
                Ok(result)
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = parent.update(cx, |parent, cx| {
                parent.busy = false;
                parent.control = None;
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                match result {
                    Ok(report) => this.report = Some(report),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
impl Render for PublishDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex().flex_col().gap(px(12.));
        let weak = cx.weak_entity();
        let providers = self.providers.clone();
        if self.report.is_none() {
            body = body
                .child(label(
                    "图片已按当前参数处理。上传将保留这些像素，使用新的远端路径。",
                    12.,
                    MUTED,
                ))
                .child(
                    action(
                        "publish-provider",
                        if self.provider.is_empty() {
                            "请先添加存储源"
                        } else {
                            &self.provider
                        },
                    )
                    .disabled(self.busy)
                    .dropdown_menu(move |mut menu, _, _| {
                        for provider in &providers {
                            let provider = provider.clone();
                            let weak = weak.clone();
                            menu = menu.item(PopupMenuItem::new(provider.clone()).on_click(
                                move |_, _, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.provider = provider.clone();
                                        cx.notify();
                                    });
                                },
                            ));
                        }
                        menu
                    }),
                )
                .child(label("目标目录", 12., TEXT))
                .child(Input::new(&self.prefix).disabled(self.busy));
        }
        if let Some(report) = &self.report {
            let rows = report["files"].as_array().cloned().unwrap_or_default();
            let count = rows.iter().filter(|file| file["success"] == true).count();
            body = body.child(label(
                format!("已上传并验证 {} / {} 项", count, rows.len()),
                13.,
                TEXT,
            ));
            let mut list = div()
                .id("publish-results")
                .max_h(px(210.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(8.));
            for (index, file) in rows.iter().take(50).enumerate() {
                let message = file["error"]
                    .as_str()
                    .or(file["url"].as_str())
                    .unwrap_or("等待继续");
                list = list.child(label(
                    format!("{}. {}", index + 1, message),
                    12.,
                    if file["error"].as_str().is_some() {
                        RED
                    } else {
                        MUTED
                    },
                ));
            }
            body = body.child(list);
            let complete = count == rows.len();
            body = body.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        action("publish-copy-links", "复制已验证链接")
                            .disabled(count == 0)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let format = Preferences::load(&this.root)
                                    .unwrap_or_default()
                                    .copy_format;
                                let text = rows
                                    .iter()
                                    .filter(|file| file["success"] == true)
                                    .filter_map(|file| file["url"].as_str())
                                    .map(|url| format.render("image", url))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if !text.is_empty() {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                            })),
                    )
                    .when(!complete, |row| {
                        row.child(
                            action("publish-retry", "重试失败项")
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| this.run(true, cx))),
                        )
                    }),
            );
            if let Some(warning) = report["record_warning"].as_str() {
                body = body.child(label(warning.to_owned(), 12., RED));
            }
        }
        if let Some(error) = &self.error {
            body = body.child(label(error.clone(), 12., RED));
        }
        body.child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(
                    action("publish-close", if self.busy { "暂停" } else { "关闭" })
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(control) = &this.control {
                                control.stop(engine::CANCEL);
                            } else {
                                window.close_dialog(cx);
                            }
                        })),
                )
                .when(self.report.is_none(), |row| {
                    row.child(
                        action(
                            "publish-start",
                            if self.busy {
                                "上传中…"
                            } else {
                                "开始上传"
                            },
                        )
                        .primary()
                        .disabled(self.busy || self.provider.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| this.run(false, cx))),
                    )
                }),
        )
    }
}
struct PresetDialog {
    parent: WeakEntity<Tools>,
    root: PathBuf,
    name: Entity<InputState>,
    rows: Vec<(String, String, ProcessingPlan)>,
    error: Option<String>,
    busy: bool,
}
impl PresetDialog {
    fn new(
        parent: WeakEntity<Tools>,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let directory = root.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<Vec<(String, String, ProcessingPlan)>> {
                let catalog = Catalog::open(&directory)?;
                let mut rows = vec![];
                for entity in catalog.sync_entities("preset:")? {
                    let state = catalog.sync_entity(&entity)?;
                    if state.deleted || !state.conflicts.is_empty() {
                        continue;
                    }
                    if let (Some(name), Some(plan)) = (
                        state.fields.get("name").and_then(|value| value.as_str()),
                        state.fields.get("plan"),
                    ) && let Ok(plan) = serde_json::from_value::<ProcessingPlan>(plan.clone())
                    {
                        rows.push((entity, name.to_owned(), plan));
                    }
                }
                Ok(rows)
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(rows) => this.rows = rows,
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            parent,
            root,
            name: cx
                .new(|cx| InputState::new(window, cx).placeholder(crate::i18n::text("预设名称"))),
            rows: vec![],
            error: None,
            busy: false,
        }
    }
}
impl Render for PresetDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex().flex_col().gap(px(10.));
        let mut list = div()
            .id("preset-list")
            .max_h(px(240.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(6.));
        for (id, name, plan) in self.rows.clone() {
            let parent = self.parent.clone();
            list = list.child(
                action(SharedString::from(id), &name)
                    .ghost()
                    .w_full()
                    .on_click(move |_, window, cx| {
                        let _ = parent.update(cx, |parent, cx| {
                            parent.apply_preset(plan.clone(), window, cx)
                        });
                        window.close_dialog(cx);
                    }),
            );
        }
        body = body
            .child(list)
            .child(label("保存通用参数；每张图片的标注单独保留。", 12., MUTED))
            .child(Input::new(&self.name));
        if let Some(error) = &self.error {
            body = body.child(label(error.clone(), 12., RED));
        }
        body.child(
            action("preset-save", "保存当前设置")
                .primary()
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, _, cx| {
                    let name = this.name.read(cx).value().trim().to_owned();
                    if name.is_empty() || name.chars().count() > 64 {
                        this.error = Some("预设名称需要 1–64 个字符".into());
                        cx.notify();
                        return;
                    }
                    let plan = this
                        .parent
                        .upgrade()
                        .map(|parent| parent.read(cx).recipe(false, cx));
                    let Some(Ok(mut plan)) = plan else {
                        this.error = Some("请先修正处理参数".into());
                        cx.notify();
                        return;
                    };
                    plan.annotations.clear();
                    let root = this.root.clone();
                    this.busy = true;
                    let task = cx.background_executor().spawn(async move {
                        (|| -> anyhow::Result<(String, String, ProcessingPlan)> {
                            let mut catalog = Catalog::open(&root)?;
                            let id = format!("preset:{}", uuid::Uuid::new_v4());
                            catalog.sync_set_fields(
                                &id,
                                &[
                                    ("name", name.clone().into()),
                                    ("plan", serde_json::to_value(&plan)?),
                                ],
                            )?;
                            Ok((id, name, plan))
                        })()
                    });
                    cx.spawn(async move |this, cx| {
                        let result = task.await;
                        let _ = this.update(cx, |this, cx| {
                            this.busy = false;
                            match result {
                                Ok(row) => this.rows.push(row),
                                Err(error) => this.error = Some(error.to_string()),
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                    cx.notify();
                })),
        )
    }
}
impl Tools {
    fn apply_preset(&mut self, plan: ProcessingPlan, window: &mut Window, cx: &mut Context<Self>) {
        let set = |this: &mut Self,
                   key: &str,
                   value: String,
                   window: &mut Window,
                   cx: &mut Context<Self>| {
            this.fields[key].update(cx, |input, cx| input.set_value(value, window, cx));
        };
        self.compression_mode = match plan.encoding.compression {
            plan::Compression::Quality { quality } => {
                set(self, "quality", quality.to_string(), window, cx);
                0
            }
            plan::Compression::Target { bytes } => {
                set(self, "target", bytes.div_ceil(1024).to_string(), window, cx);
                1
            }
            plan::Compression::Lossless => 2,
        };
        set(
            self,
            "jpeg_bg",
            format!(
                "#{:02x}{:02x}{:02x}",
                plan.encoding.jpeg_background[0],
                plan.encoding.jpeg_background[1],
                plan.encoding.jpeg_background[2]
            ),
            window,
            cx,
        );
        self.resize_mode = if let Some(resize) = &plan.geometry.resize {
            set(self, "edge", resize.max_edge.to_string(), window, cx);
            set(self, "width", resize.width.to_string(), window, cx);
            set(self, "height", resize.height.to_string(), window, cx);
            if resize.max_edge > 0 { 1 } else { 2 }
        } else {
            0
        };
        self.split_mode = match &plan.split {
            Some(plan::Split::Height { height }) => {
                set(self, "split_height", height.to_string(), window, cx);
                1
            }
            Some(plan::Split::Grid { rows, columns }) => {
                set(self, "rows", rows.to_string(), window, cx);
                set(self, "cols", columns.to_string(), window, cx);
                2
            }
            None => 0,
        };
        if let Some(stitch) = &plan.stitch {
            set(
                self,
                "stitch_bg",
                format!(
                    "#{:02x}{:02x}{:02x}",
                    stitch.background[0], stitch.background[1], stitch.background[2]
                ),
                window,
                cx,
            );
            set(self, "spacing", stitch.spacing.to_string(), window, cx);
            set(
                self,
                "cross_size",
                stitch.cross_size.unwrap_or(0).to_string(),
                window,
                cx,
            );
        }
        if let Some(mark) = &plan.watermark {
            set(self, "margin", mark.margin.to_string(), window, cx);
            set(
                self,
                "watermark_scale",
                format!("{}", (mark.scale * 100.).round()),
                window,
                cx,
            );
            set(
                self,
                "opacity",
                format!("{}", (mark.opacity * 100.).round()),
                window,
                cx,
            );
            if self
                .watermark
                .as_ref()
                .is_none_or(|(_, hash)| hash != &mark.resource)
            {
                self.watermark = None;
                self.error = Some("此预设需要水印图片，请在本机选择同一张图片。".into());
            }
        }
        self.plan = plan;
        self.crop_editing = false;
        self.schedule_preview(cx);
    }
}
