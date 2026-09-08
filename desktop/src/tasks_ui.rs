use super::*;
use img_records::catalog::Catalog;
#[derive(Clone)]
struct Row {
    id: String,
    kind: String,
    body: serde_json::Value,
    total: usize,
    done: usize,
    active: bool,
}
pub(super) enum TaskEvent {
    Uploads,
}
impl EventEmitter<TaskEvent> for TaskPanel {}
pub(super) struct TaskPanel {
    root: PathBuf,
    engine: PathBuf,
    rows: Vec<Row>,
    loading: bool,
    control: Option<Control>,
    busy: bool,
    selected: Option<String>,
    notice: Option<String>,
    uploads: usize,
    stopping: bool,
}
impl TaskPanel {
    pub fn new(root: PathBuf, engine: PathBuf, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        })
        .detach();
        Self {
            root,
            engine,
            rows: vec![],
            loading: false,
            control: None,
            busy: false,
            selected: None,
            notice: None,
            uploads: 0,
            stopping: false,
        }
    }
    pub fn stop(&mut self) -> Option<Control> {
        self.stopping = true;
        let control = self.control.take();
        if let Some(control) = &control {
            control.stop(engine::CANCEL);
        }
        control
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.stopping {
            return;
        }
        self.loading = true;
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            (|| -> anyhow::Result<(Vec<Row>, usize)> {
                let catalog = Catalog::open(&root)?;
                let mut rows = vec![];
                for (id, kind, body) in catalog.tasks("")? {
                    let value: serde_json::Value = serde_json::from_str(&body)?;
                    let files = value["files"].as_array().or(value.as_array());
                    let (total, done) = if kind == "index" {
                        let found = value["seen"].as_array().map(Vec::len).unwrap_or(0);
                        (found, if value["complete"] == true { found } else { 0 })
                    } else {
                        let total = if kind == "process" {
                            if value["plan"]["stitch"].is_object() {
                                1
                            } else {
                                value["inputs"].as_array().map(Vec::len).unwrap_or(0)
                            }
                        } else {
                            files.map(Vec::len).unwrap_or(0)
                        };
                        (
                            total,
                            files
                                .map(|files| {
                                    files
                                        .iter()
                                        .filter(|file| {
                                            file["success"] == true || file["status"] == "complete"
                                        })
                                        .count()
                                })
                                .unwrap_or(0),
                        )
                    };
                    let namespace = match kind.as_str() {
                        "process" => "processing-task",
                        "publish" => "publishing-task",
                        "migrate" => "migration-task",
                        "index" => "index-scope",
                        _ => "",
                    };
                    let active = !namespace.is_empty()
                        && img_records::remote_lock::is_active(
                            &root,
                            namespace,
                            id.split_once(':').map(|(_, id)| id).unwrap_or(""),
                        )?;
                    rows.push(Row {
                        id,
                        kind,
                        body: value,
                        total,
                        done,
                        active,
                    });
                }
                let uploads = model::load(&root)?
                    .iter()
                    .filter(|item| {
                        matches!(
                            item.status,
                            Status::Ready | Status::Running | Status::Paused | Status::Failed
                        )
                    })
                    .count();
                Ok((rows, uploads))
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((rows, uploads)) => {
                        this.rows = rows;
                        this.uploads = uploads;
                    }
                    Err(_) => this.notice = Some("无法读取任务记录，请稍后重试".into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn retry(&mut self, id: String, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let root = self.root.clone();
        let engine = self.engine.clone();
        let control = Control::default();
        self.control = Some(control.clone());
        self.busy = true;
        self.notice = None;
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            (|| -> anyhow::Result<String> {
                let mut command = std::process::Command::new(engine);
                command
                    .arg("--config")
                    .arg(storage::config_path()?)
                    .args(["tasks", "retry", &id])
                    .env("IMG_DATA_DIR", root);
                let output = engine::run(command, &control.child())?;
                if output.stopped != 0 {
                    return Ok("任务已暂停，进度已保留".into());
                }
                let value: serde_json::Value = serde_json::from_slice(&output.stdout)
                    .map_err(|_| anyhow::anyhow!("无法继续任务，请检查来源文件和连接"))?;
                if let Some(error) = value["error"].as_str() {
                    anyhow::bail!("{error}");
                }
                Ok(if output.success {
                    "任务已完成"
                } else {
                    "部分项目未完成，请查看详情"
                }
                .into())
            })()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.control = None;
                this.notice = Some(result.unwrap_or_else(|error| error.to_string()));
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn export_report(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|row| row.id == id).cloned() else {
            return;
        };
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::i18n::text("选择报告保存目录")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.first()
            {
                let path = path.join(format!("img-task-{}.json", row.id.replace(':', "-")));
                let task = cx.background_executor().spawn(async move {
                    (|| -> anyhow::Result<()> {
                        use std::io::Write;
                        let mut file = std::fs::File::options()
                            .create_new(true)
                            .write(true)
                            .open(path)?;
                        file.write_all(&serde_json::to_vec_pretty(&row.body)?)?;
                        file.sync_all()?;
                        Ok(())
                    })()
                });
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(match result {
                        Ok(()) => "报告已保存".into(),
                        Err(error) => error.to_string(),
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
impl Render for TaskPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body =
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(label(format!("上传队列 · {} 项", self.uploads), 12., MUTED))
                        .child(div().flex_1())
                        .child(action("task-show-uploads", "查看上传").ghost().on_click(
                            cx.listener(|_, _, window, cx| {
                                cx.emit(TaskEvent::Uploads);
                                window.close_dialog(cx);
                            }),
                        )),
                )
                .when(self.busy, |view| {
                    view.child(action("task-stop", "暂停当前任务").on_click(cx.listener(
                        |this, _, _, _| {
                            if let Some(control) = &this.control {
                                control.stop(engine::CANCEL);
                            }
                        },
                    )))
                });
        if self.rows.is_empty() {
            body = body.child(label("处理、索引和迁移任务会显示在这里。", 12., MUTED));
        }
        let list = uniform_list(
            "task-list",
            self.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = this.rows[index].clone();
                        let title = match row.kind.as_str() {
                            "process" => "图片处理",
                            "publish" => "上传处理结果",
                            "migrate" => "跨图床复制",
                            "index" => "远端索引",
                            "delete" => "远端删除",
                            _ => "任务",
                        };
                        let resumable = matches!(
                            row.kind.as_str(),
                            "process" | "publish" | "migrate" | "index"
                        );
                        let id = row.id.clone();
                        let selected = this.selected.as_ref() == Some(&id);
                        div()
                            .id(SharedString::from(format!("task-row-{id}")))
                            .h(px(58.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(6.))
                            .when(selected, |view| view.bg(crate::theme::color(BORDER)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(label(title, 13., TEXT))
                                    .child(label(
                                        format!(
                                            "{} · {}",
                                            if row.kind == "index" {
                                                format!("已索引 {} 张", row.total)
                                            } else {
                                                format!("{} / {}", row.done, row.total)
                                            },
                                            crate::i18n::text(if row.active {
                                                "进行中"
                                            } else if if row.kind == "index" {
                                                row.body["complete"] == true
                                            } else {
                                                row.done == row.total && row.total > 0
                                            } {
                                                "已完成"
                                            } else {
                                                "可继续"
                                            })
                                        ),
                                        11.,
                                        MUTED,
                                    )),
                            )
                            .child(
                                action(SharedString::from(format!("task-details-{id}")), "详情")
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.selected =
                                            if selected { None } else { Some(id.clone()) };
                                        cx.notify();
                                    })),
                            )
                            .when(
                                resumable
                                    && (if row.kind == "index" {
                                        row.body["complete"] != true
                                    } else {
                                        row.done < row.total
                                    }),
                                |view| {
                                    view.child(
                                        action(
                                            SharedString::from(format!("task-retry-{}", row.id)),
                                            "继续",
                                        )
                                        .disabled(row.active || this.busy)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.retry(row.id.clone(), cx)
                                            }),
                                        ),
                                    )
                                },
                            )
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .h(px((self.rows.len() as f32 * 58.).clamp(58., 290.)));
        body = body.child(list);
        if let Some(row) = self
            .selected
            .as_ref()
            .and_then(|id| self.rows.iter().find(|row| &row.id == id))
            .cloned()
        {
            let mut detail = div()
                .id("task-details")
                .max_h(px(150.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(7.));
            if let Some(files) = row.body["files"].as_array().or(row.body.as_array()) {
                for (index, file) in files.iter().enumerate().take(100) {
                    let text = file["error"]
                        .as_str()
                        .or(file["url"].as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            crate::i18n::text(
                                if file["success"] == true || file["status"] == "complete" {
                                    "已完成"
                                } else {
                                    "等待继续"
                                },
                            )
                            .to_string()
                        });
                    detail = detail.child(label(
                        format!("{}. {}", index + 1, text),
                        11.,
                        if file["error"].as_str().is_some() {
                            RED
                        } else {
                            MUTED
                        },
                    ));
                }
            }
            let id = row.id.clone();
            let mut actions = div().flex().gap(px(8.)).child(
                action("task-export-report", "导出报告").ghost().on_click(
                    cx.listener(move |this, _, _, cx| this.export_report(id.clone(), cx)),
                ),
            );
            if matches!(row.kind.as_str(), "publish" | "migrate") {
                let files = row.body["files"].as_array().cloned().unwrap_or_default();
                actions = actions.child(
                    action("task-copy-links", "复制已验证链接")
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let format = Preferences::load(&this.root)
                                .unwrap_or_default()
                                .copy_format;
                            let text = files
                                .iter()
                                .filter(|f| f["success"] == true)
                                .filter_map(|f| {
                                    f["url"].as_str().map(|url| format.render("image", url))
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            if !text.is_empty() {
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                            }
                        })),
                );
            }
            body = body.child(detail).child(actions);
        }
        body.when_some(self.notice.as_ref(), |body, message| {
            body.child(label(message.clone(), 12., MUTED))
        })
    }
}
