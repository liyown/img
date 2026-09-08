use super::*;
use img_records::targets::TargetPlan;
pub(super) struct CopyDialog {
    parent: WeakEntity<Library>,
    root: PathBuf,
    engine: PathBuf,
    targets: TargetPlan,
    hidden: usize,
    providers: Vec<String>,
    provider: String,
    prefix: Entity<InputState>,
    plan: Option<(PathBuf, serde_json::Value)>,
    report: Option<serde_json::Value>,
    busy: bool,
    error: Option<String>,
    control: Option<Control>,
}
impl CopyDialog {
    pub fn new(
        parent: WeakEntity<Library>,
        targets: TargetPlan,
        hidden: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let library = parent
            .upgrade()
            .expect("library exists while opening dialog");
        let library = library.read(cx);
        let mut providers = library.manageable.iter().cloned().collect::<Vec<_>>();
        providers.sort();
        let provider = providers
            .iter()
            .find(|provider| {
                !targets
                    .targets
                    .iter()
                    .all(|target| target.location.provider == **provider)
            })
            .or(providers.first())
            .cloned()
            .unwrap_or_default();
        let root = library.root.clone();
        let engine = library.engine.clone();
        let prefix = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("copies")
                .placeholder(crate::i18n::text("目标目录，例如 posts/"))
        });
        Self {
            parent,
            root,
            engine,
            targets,
            hidden,
            providers,
            provider,
            prefix,
            plan: None,
            report: None,
            busy: false,
            error: None,
            control: None,
        }
    }
    fn run(&mut self, apply: bool, cx: &mut Context<Self>) {
        if self.busy || self.provider.is_empty() {
            return;
        }
        let prefix = self.prefix.read(cx).value().trim().to_owned();
        if !apply
            && (prefix.starts_with('/')
                || prefix.contains('\\')
                || prefix.chars().any(char::is_control)
                || (!prefix.is_empty()
                    && prefix
                        .trim_end_matches('/')
                        .split('/')
                        .any(|p| p.is_empty() || p == "." || p == "..")))
        {
            self.error = Some("请输入相对目录，例如 photos/，不要以 / 开头或包含 ..".into());
            cx.notify();
            return;
        }
        self.error = None;
        self.busy = true;
        let control = Control::default();
        self.control = Some(control.clone());
        let _ = self.parent.update(cx, |parent, cx| {
            parent.busy = true;
            parent.control = Some(control.clone());
            cx.notify();
        });
        let root = self.root.clone();
        let engine = self.engine.clone();
        let targets = self.targets.clone();
        let provider = self.provider.clone();
        let saved = self.plan.as_ref().map(|(path, _)| path.clone());
        let parent = self.parent.clone();
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<(PathBuf, serde_json::Value)> {
                use std::io::Write;
                let directory = root.join("migration-plans");
                std::fs::create_dir_all(&directory)?;
                let mut selection = tempfile::NamedTempFile::new_in(&directory)?;
                selection.write_all(&serde_json::to_vec(&targets)?)?;
                selection.as_file().sync_all()?;
                let path = saved
                    .unwrap_or_else(|| directory.join(format!("{}.json", uuid::Uuid::new_v4())));
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .env("IMG_DATA_DIR", &root)
                    .arg("migrate");
                if apply {
                    command.arg("apply").arg(&path);
                } else {
                    command
                        .arg("plan")
                        .arg("--selection")
                        .arg(selection.path())
                        .arg("--to")
                        .arg(provider)
                        .arg("--prefix")
                        .arg(prefix)
                        .arg("--output")
                        .arg(&path);
                }
                let output = engine::run(command, &control)?;
                anyhow::ensure!(output.stopped == 0, "操作已取消，已保存的进度可继续执行");
                anyhow::ensure!(!output.stdout.is_empty(), "无法完成复制，请检查连接后重试");
                let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
                if let Some(error) = result["error"].as_str() {
                    anyhow::bail!("{error}");
                }
                Ok((path, result))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = parent.update(cx, |parent, cx| {
                parent.busy = false;
                parent.control = None;
                parent.stamp.clear();
                parent.refresh(cx);
                cx.notify();
            });
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                match result {
                    Ok((path, value)) => {
                        if apply {
                            this.report = Some(value);
                        } else {
                            this.plan = Some((path, value));
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
}
impl Render for CopyDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex().flex_col().gap(px(12.)).child(label(
            format!(
                "复制 {} 张图片，其中 {} 张不在当前结果中",
                self.targets.targets.len(),
                self.hidden
            ),
            12.,
            MUTED,
        ));
        let weak = cx.weak_entity();
        if let Some((_, plan)) = &self.plan {
            body = body.child(label(
                format!("目标存储源：{}", plan["provider"].as_str().unwrap_or("")),
                13.,
                TEXT,
            ));
            let destinations = plan["destinations"].as_array().cloned().unwrap_or_default();
            let mut preview = div()
                .id("copy-paths")
                .max_h(px(200.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(5.));
            for destination in destinations.iter().take(30) {
                preview = preview.child(
                    label(
                        destination["path"].as_str().unwrap_or("").to_owned(),
                        11.,
                        MUTED,
                    )
                    .text_ellipsis(),
                );
            }
            if destinations.len() > 30 {
                preview = preview.child(label(
                    format!(
                        "另有 {} 个文件，完整清单可在计划文件中查看",
                        destinations.len() - 30
                    ),
                    11.,
                    MUTED,
                ));
            }
            body = body.child(preview);
            if let Some(report) = &self.report {
                let files = report["files"].as_array();
                let success = files
                    .map(|rows| rows.iter().filter(|r| r["success"] == true).count())
                    .unwrap_or(0);
                body = body.child(label(
                    format!(
                        "已验证 {} 项，共 {} 项",
                        success,
                        self.targets.targets.len()
                    ),
                    13.,
                    TEXT,
                ));
                if let Some(files) = files {
                    let mut errors = div()
                        .id("copy-failures")
                        .max_h(px(120.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(6.));
                    for item in files.iter().filter(|r| r["success"] != true).take(20) {
                        errors = errors.child(label(
                            item["error"].as_str().unwrap_or("等待重试").to_owned(),
                            12.,
                            RED,
                        ));
                    }
                    body = body.child(errors);
                }
            }
        } else {
            let providers = self.providers.clone();
            body = body
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(label("目标存储源", 12., TEXT))
                        .child(
                            action(
                                "copy-target",
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
                                    menu =
                                        menu.item(PopupMenuItem::new(provider.clone()).on_click(
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
                        ),
                )
                .child(label("目标目录", 12., TEXT))
                .child(Input::new(&self.prefix).disabled(self.busy));
        }
        body = body.child(label(
            "源文件保留。上传后校验内容，通过后才生成新链接映射。",
            12.,
            MUTED,
        ));
        if let Some(error) = &self.error {
            body = body.child(label(error.clone(), 12., RED));
        }
        let complete = self
            .report
            .as_ref()
            .and_then(|r| r["files"].as_array())
            .is_some_and(|rows| rows.iter().all(|r| r["success"] == true));
        body.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .when(self.plan.is_some(), |row| {
                    row.child(
                        action("copy-export", "查看计划")
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some((path, _)) = &this.plan {
                                    cx.open_url(&format!("file://{}", path.display()));
                                }
                            })),
                    )
                })
                .when(self.report.is_some(), |row| {
                    row.child(
                        action("copy-fix-references", "维护文章链接")
                            .ghost()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(id) = this
                                    .report
                                    .as_ref()
                                    .and_then(|r| r["plan"]["task_id"].as_str())
                                    .map(str::to_owned)
                                {
                                    window.close_dialog(cx);
                                    let _ = this.parent.update(cx, |_, cx| {
                                        cx.emit(PreferenceChanged::References(id))
                                    });
                                }
                            })),
                    )
                    .child(
                        action("copy-mapping", "复制链接映射")
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(report) = &this.report {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        serde_json::to_string_pretty(&report["mapping"])
                                            .unwrap_or_default(),
                                    ));
                                }
                            })),
                    )
                })
                .child(div().flex_1())
                .child(
                    action("copy-close", if self.busy { "取消任务" } else { "关闭" })
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(control) = &this.control {
                                control.stop(engine::CANCEL);
                            } else {
                                window.close_dialog(cx);
                            }
                        })),
                )
                .when(!complete, |row| {
                    row.child(
                        action(
                            "copy-start",
                            if self.busy {
                                "处理中…"
                            } else if self.report.is_some() {
                                "重试失败项"
                            } else if self.plan.is_some() {
                                "开始复制"
                            } else {
                                "预览目标"
                            },
                        )
                        .primary()
                        .disabled(self.busy || self.provider.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| this.run(this.plan.is_some(), cx))),
                    )
                }),
        )
    }
}
