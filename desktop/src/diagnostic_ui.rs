use super::*;

impl ImgDesktop {
    pub(super) fn persistence_failed(&mut self, error: anyhow::Error, cx: &mut Context<Self>) {
        self.queue.persistence_ok = false;
        if let Some(batch) = &mut self.queue.batch {
            batch.paused = true;
        }
        for active in self.queue.active.values() {
            active.control.stop(engine::PAUSE);
        }
        self.message(
            format!("队列未保存：{error}。请到设置中恢复队列。"),
            true,
            cx,
        );
    }
    pub(super) fn recovery_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(label("诊断与恢复", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(label(
                if self.queue.persistence_ok {
                    "队列正常；保留最近两份有效备份。诊断文件不包含图片与凭据。"
                } else {
                    "队列未能安全保存，上传已停止。恢复前会保留当前队列文件。"
                },
                12.,
                if self.queue.persistence_ok {
                    MUTED
                } else {
                    RED
                },
            ))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        action("export-diagnostics", "导出诊断")
                            .on_click(cx.listener(|this, _, _, cx| this.export_diagnostics(cx))),
                    )
                    .child(
                        action("restore-queue", "从备份恢复")
                            .disabled(!self.queue.active.is_empty() || self.preparing)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.recover_queue(false, window, cx)
                            })),
                    )
                    .child(
                        action("reset-queue", "重建空队列")
                            .disabled(!self.queue.active.is_empty() || self.preparing)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.recover_queue(true, window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
    fn export_diagnostics(&mut self, cx: &mut Context<Self>) {
        let bytes = crate::diagnostics::report(&self.queue.items);
        let selected = cx.prompt_for_new_path(&self.root, Some("img-diagnostics.json"));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = selected.await {
                let result = cx
                    .background_executor()
                    .spawn(async move { std::fs::write(path, bytes) })
                    .await;
                let _ = this.update(cx, |this, cx| match result {
                    Ok(()) => this.message("诊断已导出", false, cx),
                    Err(_) => this.message("无法保存诊断文件，请检查目录权限", true, cx),
                });
            }
        })
        .detach();
    }
    fn recover_queue(&mut self, empty: bool, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = window.prompt(
            PromptLevel::Warning,
            if empty {
                "保留当前队列文件并重建空队列？"
            } else {
                "使用最近的有效备份恢复队列？"
            },
            Some("当前队列会另存保留；原始图片、应用内副本和远端图片都不会删除。"),
            &["取消", "继续"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                return;
            }
            let pending = this
                .update(cx, |this, _| {
                    if !this.queue.active.is_empty() || this.preparing {
                        return None;
                    }
                    this.queue.persistence_ok = false;
                    this.queue.persistence_generation += 1;
                    this.preparing = true;
                    this.queue.batch = None;
                    Some(this.queue.queue_store.recover(empty))
                })
                .ok()
                .flatten();
            let Some(pending) = pending else {
                return;
            };
            let result = crate::queue_store::acknowledged(pending).await;
            let _ = this.update(cx, |this, cx| {
                this.preparing = false;
                match result {
                    Ok(items) => {
                        this.queue.items = items;
                        this.records_revision += 1;
                        this.queue.persistence_ok = true;
                        this.storage_settings.update(cx, |settings, cx| {
                            settings.uploading = false;
                            cx.notify();
                        });
                        this.message("队列已恢复，上传任务需要手动继续", false, cx);
                    }
                    Err(e) => this.message(format!("恢复未完成：{e}"), true, cx),
                }
            });
        })
        .detach();
    }
    pub(super) fn failure_details(
        &mut self,
        item: Item,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let failure = crate::diagnostics::Failure::from_json(
            &serde_json::json!({"error_code":item.error_code,"http_status":item.http_status,"retryable":item.retryable}),
        );
        let action = if failure.needs_file() {
            "重新选择文件"
        } else if failure.needs_settings() {
            "修改设置"
        } else {
            "重试"
        };
        let detail = format!(
            "{}\n错误分类：{}{}",
            failure.summary(),
            failure.error_code,
            failure
                .http_status
                .map(|s| format!(" · HTTP {s}"))
                .unwrap_or_default()
        );
        let prompt = window.prompt(
            PromptLevel::Warning,
            "上传未完成",
            Some(&detail),
            &["关闭", action],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                if failure.needs_file() {
                    this.pick_files(&ChooseFiles, window, cx);
                } else if failure.needs_settings() {
                    this.navigate(Page::Settings, window, cx);
                } else {
                    this.start_upload(&item.id, cx);
                }
            });
        })
        .detach();
    }
}
