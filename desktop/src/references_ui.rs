use super::*;
pub(super) struct References {
    root: PathBuf,
    engine: PathBuf,
    directory: Option<PathBuf>,
    migration: Option<String>,
    mapping: Option<PathBuf>,
    migrations: Vec<(String, String)>,
    plan: Option<serde_json::Value>,
    report: Option<serde_json::Value>,
    selected: usize,
    busy: bool,
    error: Option<String>,
    control: Option<Control>,
    stopping: bool,
}
impl References {
    pub fn new(
        root: PathBuf,
        engine: PathBuf,
        migration: Option<String>,
        task_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let directory = root.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<Vec<(String, String)>> {
                let c = img_records::catalog::Catalog::open(&directory)?;
                Ok(c.tasks("migrate")?
                    .into_iter()
                    .filter_map(|(id, _, body)| {
                        let value: serde_json::Value = serde_json::from_str(&body).ok()?;
                        let done = value["files"]
                            .as_array()?
                            .iter()
                            .filter(|f| f["success"] == true)
                            .count();
                        (done > 0).then(|| {
                            (
                                id.trim_start_matches("migrate:").to_owned(),
                                format!(
                                    "{} · {}",
                                    value["plan"]["provider"].as_str().unwrap_or(""),
                                    done
                                ),
                            )
                        })
                    })
                    .collect())
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(rows) => this.migrations = rows,
                    Err(error) => this.error = Some(error.to_string()),
                }
                if let Some(id) = task_id {
                    this.run(vec!["show".into(), id.into()], cx);
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            root,
            engine,
            directory: None,
            migration,
            mapping: None,
            migrations: vec![],
            plan: None,
            report: None,
            selected: 0,
            busy: false,
            error: None,
            control: None,
            stopping: false,
        }
    }
    pub fn stop(&mut self) -> Option<Control> {
        self.stopping = true;
        let control = self.control.take();
        if let Some(control) = &control {
            control.stop(engine::CANCEL)
        }
        control
    }
    fn run(&mut self, args: Vec<std::ffi::OsString>, cx: &mut Context<Self>) {
        if self.busy || self.stopping {
            return;
        }
        self.busy = true;
        self.error = None;
        let operation = args[0].to_string_lossy().into_owned();
        let root = self.root.clone();
        let engine = self.engine.clone();
        let control = Control::default();
        self.control = Some(control.clone());
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<serde_json::Value> {
                let mut command = std::process::Command::new(engine);
                command
                    .arg("references")
                    .args(args)
                    .env("IMG_DATA_DIR", root);
                let output = engine::run_json(command, &control.child())?;
                anyhow::ensure!(output.stopped == 0, "任务已暂停，可在任务面板查看进度");
                let value: serde_json::Value = serde_json::from_slice(&output.stdout)
                    .map_err(|_| anyhow::anyhow!("无法完成文章维护，请检查目录和连接"))?;
                if let Some(error) = value["error"].as_str() {
                    anyhow::bail!("{error}")
                }
                Ok(value)
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                match result {
                    Ok(value) => {
                        if operation == "export" {
                            this.error = Some("报告与备份已导出".into());
                        } else if value["plan"].is_object() {
                            this.directory = value["plan"]["root"].as_str().map(PathBuf::from);
                            this.plan = Some(value["plan"].clone());
                            this.report = Some(value);
                        } else {
                            this.directory = value["root"].as_str().map(PathBuf::from);
                            this.plan = Some(value);
                            this.report = None;
                            this.selected = 0;
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
    fn scan(&mut self, cx: &mut Context<Self>) {
        let Some(directory) = &self.directory else {
            return;
        };
        let mut args = vec!["scan".into(), directory.as_os_str().to_owned()];
        if let Some(id) = &self.migration {
            args.extend(["--migration".into(), id.into()]);
        } else if let Some(mapping) = &self.mapping {
            args.extend(["--mapping".into(), mapping.as_os_str().to_owned()]);
        }
        self.run(args, cx);
    }
    fn choose(&mut self, mapping: bool, cx: &mut Context<Self>) {
        let task = cx.prompt_for_paths(PathPromptOptions {
            files: mapping,
            directories: !mapping,
            multiple: false,
            prompt: Some(crate::i18n::text(if mapping {
                "选择链接映射 JSON"
            } else {
                "选择 Markdown 目录"
            })),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = task.await
                && let Some(path) = paths.first()
            {
                let path = path.clone();
                let _ = this.update(cx, |this, cx| {
                    if mapping {
                        this.mapping = Some(path);
                        this.migration = None
                    } else {
                        this.directory = Some(path)
                    }
                    this.plan = None;
                    this.report = None;
                    cx.notify();
                });
            }
        })
        .detach();
    }
    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self
            .plan
            .as_ref()
            .and_then(|plan| plan["task_id"].as_str())
            .map(str::to_owned)
        else {
            return;
        };
        let task = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::i18n::text("选择报告保存目录")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = task.await
                && let Some(path) = paths.first()
            {
                let target = path.join(format!("img-references-{}", uuid::Uuid::new_v4()));
                let _ = this.update(cx, |this, cx| {
                    this.run(
                        vec!["export".into(), id.into(), target.into_os_string()],
                        cx,
                    )
                });
            }
        })
        .detach();
    }
}
impl Drop for References {
    fn drop(&mut self) {
        self.stop();
    }
}
impl Render for References {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex().flex_col().gap(px(12.));
        body = body.child(label(
            "只检查所选 Markdown 目录。目录外的引用未知，不能据此判断原图可以删除。",
            12.,
            MUTED,
        ));
        let mut options = div().flex().items_center().gap(px(8.)).child(
            action("references-directory", "选择文章目录")
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, _, cx| this.choose(false, cx))),
        );
        let current = self.migration.clone();
        let rows = self.migrations.clone();
        let weak = cx.weak_entity();
        options = options
            .child(
                action(
                    "references-mapping",
                    if current.is_some() {
                        "迁移结果"
                    } else if self.mapping.is_some() {
                        "链接映射文件"
                    } else {
                        "仅检查引用"
                    },
                )
                .disabled(self.busy)
                .dropdown_menu(move |mut menu, _, _| {
                    let weak_clear = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(crate::i18n::text("仅检查引用"))
                            .checked(current.is_none())
                            .on_click(move |_, _, cx| {
                                let _ = weak_clear.update(cx, |this, cx| {
                                    this.migration = None;
                                    this.mapping = None;
                                    this.plan = None;
                                    this.report = None;
                                    cx.notify();
                                });
                            }),
                    );
                    for (id, title) in &rows {
                        let id = id.clone();
                        let weak = weak.clone();
                        menu = menu.item(
                            PopupMenuItem::new(title.clone())
                                .checked(current.as_ref() == Some(&id))
                                .on_click(move |_, _, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.migration = Some(id.clone());
                                        this.mapping = None;
                                        this.plan = None;
                                        this.report = None;
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    let weak = weak.clone();
                    menu.item(
                        PopupMenuItem::new(crate::i18n::text("导入链接映射…")).on_click(
                            move |_, _, cx| {
                                let _ = weak.update(cx, |this, cx| this.choose(true, cx));
                            },
                        ),
                    )
                }),
            )
            .child(div().flex_1())
            .child(
                action(
                    "references-scan",
                    if self.busy {
                        "处理中…"
                    } else {
                        "扫描并预览"
                    },
                )
                .disabled(self.busy || self.directory.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.scan(cx))),
            );
        body = body.child(options);
        if let Some(directory) = &self.directory {
            body = body
                .child(label(directory.to_string_lossy().into_owned(), 11., MUTED).text_ellipsis());
        }
        if let Some(plan) = &self.plan {
            let files = plan["files"].as_array().cloned().unwrap_or_default();
            let changed = files
                .iter()
                .filter(|f| f["changes"].as_array().is_some_and(|c| !c.is_empty()))
                .count();
            let changes = files
                .iter()
                .filter_map(|f| f["changes"].as_array())
                .flatten()
                .map(|c| c["occurrences"].as_u64().unwrap_or(1))
                .sum::<u64>();
            let height = (f32::from(window.viewport_size().height) - 360.).clamp(150., 420.);
            let list = uniform_list(
                "reference-file-list",
                files.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let files = this.plan.as_ref().and_then(|p| p["files"].as_array());
                    range
                        .filter_map(|index| {
                            let file = files?.get(index)?;
                            Some(
                                div()
                                    .id(SharedString::from(format!("reference-file-{index}")))
                                    .h(px(42.))
                                    .px(px(7.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(5.))
                                    .when(this.selected == index, |row| {
                                        row.bg(crate::theme::color(BORDER))
                                    })
                                    .child(
                                        label(file["relative"].as_str().unwrap_or(""), 12., TEXT)
                                            .text_ellipsis(),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.selected = index;
                                        cx.notify();
                                    }))
                                    .into_any_element(),
                            )
                        })
                        .collect()
                }),
            )
            .w(px(174.))
            .h(px(height));
            let mut detail = div()
                .id("reference-changes")
                .flex_1()
                .min_w_0()
                .h(px(height))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(9.));
            if let Some(file) = files.get(self.selected) {
                detail = detail.child(label(file["relative"].as_str().unwrap_or(""), 13., TEXT));
                if let Some(error) = file["error"].as_str() {
                    detail = detail.child(label(error.to_owned(), 12., RED));
                }
                if let Some(changes) = file["changes"].as_array() {
                    if changes.is_empty() {
                        detail = detail.child(label("没有需要替换的链接", 12., MUTED));
                    }
                    for (index, change) in changes.iter().enumerate().take(200) {
                        detail = detail.child(
                            div()
                                .border_b_1()
                                .border_color(crate::theme::color(BORDER))
                                .pb(px(8.))
                                .child(label(
                                    format!(
                                        "第 {} 行 · {} 处引用",
                                        change["line"], change["occurrences"]
                                    ),
                                    11.,
                                    MUTED,
                                ))
                                .child(label(change["old_url"].as_str().unwrap_or(""), 12., MUTED))
                                .child(label(
                                    format!("→ {}", change["new_url"].as_str().unwrap_or("")),
                                    12.,
                                    TEXT,
                                ))
                                .when(plan["restore_of"].is_string(), |row| {
                                    row.child(label(
                                        "恢复此文件的原始内容，并再次备份当前文章。",
                                        12.,
                                        TEXT,
                                    ))
                                })
                                .id(SharedString::from(format!("reference-change-{index}"))),
                        );
                    }
                    if changes.len() > 200 {
                        detail = detail.child(label("其余修改见导出的完整报告。", 11., MUTED));
                    }
                }
                let unmatched = file["unmatched"].as_array().map(Vec::len).unwrap_or(0);
                if unmatched > 0 {
                    detail = detail.child(label(
                        format!("{} 个链接没有对应映射，将保留原引用。", unmatched),
                        11.,
                        MUTED,
                    ));
                }
                if let Some(result) = self
                    .report
                    .as_ref()
                    .and_then(|report| report["files"].as_array())
                    .and_then(|rows| rows.iter().find(|r| r["relative"] == file["relative"]))
                {
                    detail = detail.child(label(
                        result["error"].as_str().unwrap_or("已保存并备份"),
                        12.,
                        if result["success"] == true {
                            MUTED
                        } else {
                            RED
                        },
                    ));
                }
            }
            body = body.child(div().flex().gap(px(14.)).child(list).child(detail));
            body = body.child(label(
                format!(
                    "将修改 {} 个文件，更新 {} 处引用。先备份，再替换；原图保留。",
                    changed, changes
                ),
                12.,
                MUTED,
            ));
            let id = plan["task_id"].as_str().unwrap_or("").to_owned();
            let apply_id = id.clone();
            let restore_id = id.clone();
            let complete = self.report.as_ref().is_some_and(|r| r["complete"] == true);
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        action("references-export", "导出报告与备份")
                            .ghost()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                    )
                    .when(
                        self.report.as_ref().is_some_and(|r| {
                            r["files"]
                                .as_array()
                                .is_some_and(|files| files.iter().any(|f| f["success"] == true))
                        }),
                        |row| {
                            row.child(
                                action("references-restore", "预览恢复")
                                    .ghost()
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run(
                                            vec!["restore".into(), restore_id.clone().into()],
                                            cx,
                                        )
                                    })),
                            )
                        },
                    )
                    .child(div().flex_1())
                    .child(
                        action(
                            "references-apply",
                            if complete {
                                "已完成"
                            } else if plan["restore_of"].is_string() {
                                "备份并恢复"
                            } else {
                                "备份并应用"
                            },
                        )
                        .primary()
                        .disabled(self.busy || changed == 0 || complete)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.run(
                                vec!["apply".into(), apply_id.clone().into(), "--yes".into()],
                                cx,
                            )
                        })),
                    ),
            );
        }
        if let Some(error) = &self.error {
            body = body.child(
                div()
                    .id("reference-error")
                    .max_h(px(70.))
                    .overflow_y_scroll()
                    .child(label(error.clone(), 12., RED)),
            );
        }
        body.when(self.busy, |view| {
            view.child(
                action("references-pause", "暂停当前任务")
                    .ghost()
                    .on_click(cx.listener(|this, _, _, _| {
                        if let Some(control) = &this.control {
                            control.stop(engine::CANCEL)
                        }
                    })),
            )
        })
    }
}
