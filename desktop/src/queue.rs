use super::*;

pub(super) struct UploadBatch {
    pub(super) pending: VecDeque<String>,
    pub(super) completed: Vec<Item>,
    pub(super) failed: usize,
    pub(super) target: String,
    pub(super) preferences: Preferences,
    pub(super) options: UploadOptions,
    pub(super) paused: bool,
    pub(super) cancelled: bool,
    pub(super) order: Vec<String>,
}

pub(super) struct ActiveUpload {
    pub(super) control: Control,
    pub(super) requeue: bool,
}

pub(super) struct UploadController {
    pub(super) items: Vec<Item>,
    pub(super) batch: Option<UploadBatch>,
    pub(super) active: HashMap<String, ActiveUpload>,
    pub(super) quit_when_done: bool,
    pub(super) persistence_ok: bool,
    pub(super) persistence_generation: u64,
    pub(super) queue_store: crate::queue_store::QueueStore,
}
impl UploadController {
    pub(super) fn new(root: PathBuf, items: Vec<Item>, persistence_ok: bool) -> Self {
        Self {
            items,
            batch: None,
            active: HashMap::new(),
            quit_when_done: false,
            persistence_ok,
            persistence_generation: 0,
            queue_store: crate::queue_store::QueueStore::new(root),
        }
    }
}

impl ImgDesktop {
    pub(super) fn start_upload(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.queue.items.iter().any(|i| i.id == id && i.simulated) {
            self.simulate(cx);
            return;
        }
        self.begin_upload(vec![id.to_owned()], cx);
    }
    pub(super) fn start_all_uploads(&mut self, cx: &mut Context<Self>) {
        let ids = self
            .queue
            .items
            .iter()
            .filter(|i| !i.simulated && i.status == Status::Ready)
            .map(|i| i.id.clone())
            .collect::<Vec<_>>();
        self.begin_upload(ids, cx);
    }
    pub(super) fn begin_upload(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        if self.queue.batch.is_some() {
            self.message("请先完成、暂停或取消当前批次", false, cx);
            return;
        }
        if ids.is_empty() {
            self.message("没有可以上传的图片", false, cx);
            return;
        }
        if !self.providers.iter().any(|(p, _)| p == &self.provider) {
            self.message("请在设置中添加并选择存储源，再开始上传", true, cx);
            return;
        }
        self.queue.batch = Some(UploadBatch {
            pending: ids.clone().into(),
            order: ids,
            completed: vec![],
            failed: 0,
            target: self.provider.clone(),
            preferences: self.preferences,
            options: self.upload_options.clone(),
            paused: false,
            cancelled: false,
        });
        self.storage_settings.update(cx, |settings, cx| {
            settings.uploading = true;
            cx.notify();
        });
        self.upload_next(cx);
    }
    pub(super) fn finish_uploads(&mut self, cx: &mut Context<Self>) {
        let Some(mut batch) = self.queue.batch.take() else {
            return;
        };
        self.storage_settings.update(cx, |settings, cx| {
            settings.uploading = false;
            cx.notify();
        });
        batch.completed.sort_by_key(|item| {
            batch
                .order
                .iter()
                .position(|id| id == &item.id)
                .unwrap_or(usize::MAX)
        });
        let count = batch.completed.len();
        let copied = batch.preferences.auto_copy && count > 0;
        if copied {
            let links = batch
                .completed
                .iter()
                .filter_map(|item| {
                    item.url
                        .as_ref()
                        .map(|url| batch.preferences.copy_format.render(&item.name, url))
                })
                .collect::<Vec<_>>()
                .join("\n");
            cx.write_to_clipboard(ClipboardItem::new_string(links));
            self.copied = batch.completed.last().map(|item| item.id.clone());
        }
        self.message(
            format!(
                "已上传 {count} 张{}{}{}",
                if copied {
                    format!(" · 已复制为 {}", batch.preferences.copy_format.label())
                } else {
                    String::new()
                },
                if batch.failed > 0 {
                    format!(" · {} 张失败，可批量重试", batch.failed)
                } else {
                    String::new()
                },
                if batch.cancelled {
                    " · 本批剩余上传已取消"
                } else {
                    ""
                }
            ),
            batch.failed > 0,
            cx,
        );
        if self.queue.quit_when_done {
            cx.quit();
        }
    }
    pub(super) fn upload_next(&mut self, cx: &mut Context<Self>) {
        loop {
            let Some(batch) = &mut self.queue.batch else {
                return;
            };
            if batch.paused || self.queue.active.len() >= batch.options.concurrency {
                return;
            }
            let Some(id) = batch.pending.pop_front() else {
                if self.queue.active.is_empty() {
                    self.finish_uploads(cx);
                }
                return;
            };
            let Some(index) = self.queue.items.iter().position(|i| {
                i.id == id
                    && !i.simulated
                    && matches!(
                        i.status,
                        Status::Ready | Status::Failed | Status::Paused | Status::Cancelled
                    )
            }) else {
                continue;
            };
            let options = batch.options.clone();
            self.queue.items[index].target = batch.target.clone();
            self.queue.items[index].status = Status::Running;
            self.queue.items[index].progress = None;
            self.queue.items[index].error = None;
            self.queue.items[index].error_code = None;
            self.queue.items[index].http_status = None;
            self.queue.items[index].retryable = None;
            if !self.persist(cx) {
                self.queue.items[index].status = Status::Paused;
                self.pause_all(cx);
                return;
            }
            let item = self.queue.items[index].clone();
            let root = self.root.clone();
            let engine = self.engine.clone();
            let control = Control::default();
            self.queue.active.insert(
                id.clone(),
                ActiveUpload {
                    control: control.clone(),
                    requeue: false,
                },
            );
            let progress_id = id.clone();
            let meter = control.clone();
            cx.spawn(async move |this, cx| {
                while !meter.finished.load(Ordering::SeqCst) {
                    cx.background_executor()
                        .timer(Duration::from_millis(80))
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            if let Some(item) = this
                                .queue
                                .items
                                .iter_mut()
                                .find(|i| i.id == progress_id && i.status == Status::Running)
                            {
                                item.progress = meter.progress().percent();
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            let saved = self.queue.queue_store.barrier();
            let task = cx.background_executor().spawn(async move {
                crate::queue_store::acknowledged(saved).await?;
                model::upload(&item, &root, &engine, &options, &control)
            });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    let active = this.queue.active.remove(&id);
                    if let Some(item) = this.queue.items.iter_mut().find(|i| i.id == id) {
                        match result {
                            Ok(model::UploadOutcome::Done(result)) => {
                                item.status = Status::Done;
                                item.progress = Some(100);
                                item.url = Some(result.url);
                                item.uploaded_size = result.size;
                                item.error = None;
                                if let Some(batch) = &mut this.queue.batch {
                                    batch.completed.push(item.clone());
                                }
                            }
                            Ok(model::UploadOutcome::Paused) => {
                                item.status = Status::Paused;
                                item.progress = None;
                                item.error = Some(
                                    "已暂停，继续时重新上传此文件；中断前服务端可能已收到图片。"
                                        .into(),
                                );
                                if active.is_some_and(|a| a.requeue)
                                    && let Some(batch) = &mut this.queue.batch
                                    && !batch.cancelled
                                {
                                    batch.pending.push_front(id.clone());
                                }
                            }
                            Ok(model::UploadOutcome::Cancelled) => {
                                item.status = Status::Cancelled;
                                item.progress = None;
                                item.error =
                                    Some("已取消本地请求；不会删除服务端可能已接收的图片。".into());
                            }
                            Err(error) => {
                                item.status = Status::Failed;
                                item.progress = None;
                                let failure = crate::diagnostics::Failure::from_error(&error);
                                item.error = Some(failure.summary().into());
                                item.error_code = Some(failure.error_code);
                                item.http_status = failure.http_status;
                                item.retryable = failure.retryable;
                                if let Some(batch) = &mut this.queue.batch {
                                    batch.failed += 1;
                                }
                            }
                        }
                    }
                    if this.persist(cx) {
                        let saved = this.queue.queue_store.barrier();
                        cx.spawn(async move |this, cx| {
                            match crate::queue_store::acknowledged(saved).await {
                                Ok(()) => {
                                    let _ = this.update(cx, |this, cx| this.upload_next(cx));
                                }
                                Err(e) => {
                                    let _ =
                                        this.update(cx, |this, cx| this.persistence_failed(e, cx));
                                }
                            }
                        })
                        .detach();
                    } else {
                        this.pause_all(cx);
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
        }
    }
    pub(super) fn pause_all(&mut self, cx: &mut Context<Self>) {
        if let Some(batch) = &mut self.queue.batch {
            batch.paused = true;
        }
        for active in self.queue.active.values_mut() {
            active.requeue = true;
            active.control.stop(engine::PAUSE);
        }
        self.message("队列已暂停，继续时重新上传被中断的文件", false, cx);
    }
    pub(super) fn resume_all(&mut self, cx: &mut Context<Self>) {
        if let Some(batch) = &mut self.queue.batch {
            batch.paused = false;
            self.upload_next(cx);
        } else {
            let ids = self
                .queue
                .items
                .iter()
                .filter(|i| !i.simulated && matches!(i.status, Status::Ready | Status::Paused))
                .map(|i| i.id.clone())
                .collect();
            self.begin_upload(ids, cx);
        }
        cx.notify();
    }
    pub(super) fn cancel_all(&mut self, cx: &mut Context<Self>) {
        if let Some(batch) = &mut self.queue.batch {
            batch.cancelled = true;
            batch.paused = false;
            for id in batch.pending.drain(..) {
                if let Some(item) = self.queue.items.iter_mut().find(|i| i.id == id) {
                    item.status = Status::Cancelled;
                    item.progress = None;
                }
            }
        }
        for active in self.queue.active.values_mut() {
            active.requeue = false;
            active.control.stop(engine::CANCEL);
        }
        self.persist(cx);
        self.upload_next(cx);
        cx.notify();
    }
    pub(super) fn stop_item(&mut self, id: &str, reason: u8, cx: &mut Context<Self>) {
        if let Some(active) = self.queue.active.get_mut(id) {
            active.requeue = false;
            active.control.stop(reason);
        } else {
            if let Some(batch) = &mut self.queue.batch {
                batch.pending.retain(|p| p != id);
            }
            if let Some(item) = self.queue.items.iter_mut().find(|i| i.id == id) {
                item.status = if reason == engine::PAUSE {
                    Status::Paused
                } else {
                    Status::Cancelled
                };
                item.progress = None;
                item.error = None;
            }
            self.persist(cx);
            self.upload_next(cx);
        }
        cx.notify();
    }
    pub(super) fn retry_failed(&mut self, cx: &mut Context<Self>) {
        let ids = self
            .queue
            .items
            .iter()
            .filter(|i| !i.simulated && i.status == Status::Failed)
            .map(|i| i.id.clone())
            .collect();
        self.begin_upload(ids, cx);
    }
    pub(super) fn remove_records(
        &mut self,
        ids: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<_> = ids
            .into_iter()
            .filter(|id| {
                !self.queue.active.contains_key(id)
                    && !self
                        .queue
                        .batch
                        .as_ref()
                        .is_some_and(|b| b.pending.contains(id))
            })
            .collect();
        if ids.is_empty() {
            self.message("没有可清理的记录，请先取消正在等待的任务", false, cx);
            return;
        }
        let prompt = window.prompt(
            PromptLevel::Warning,
            &format!("清理 {} 条记录？", ids.len()),
            Some("删除这些记录和应用内的图片缓存。你选择的原始文件与远端图片会保留。"),
            &["取消", "清理"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                let ids: Vec<_> = ids
                    .into_iter()
                    .filter(|id| {
                        !this.queue.active.contains_key(id)
                            && !this
                                .queue
                                .batch
                                .as_ref()
                                .is_some_and(|b| b.pending.contains(id))
                    })
                    .collect();
                if !this.queue.persistence_ok {
                    this.message("本地队列无法读取，不能清理记录", true, cx);
                    return;
                }
                let removed: Vec<_> = this
                    .queue
                    .items
                    .iter()
                    .filter(|i| ids.contains(&i.id))
                    .cloned()
                    .collect();
                this.queue.items.retain(|i| !ids.contains(&i.id));
                this.records_revision += 1;
                let saved = this.queue.queue_store.save(this.queue.items.clone());
                let root = this.root.clone();
                cx.spawn(async move |this, cx| {
                    match crate::queue_store::acknowledged(saved).await {
                        Ok(()) => {
                            let count = removed.len();
                            cx.background_executor()
                                .spawn(async move {
                                    model::remove_cache(&root, &removed);
                                })
                                .await;
                            let _ = this.update(cx, |this, cx| {
                                this.message(format!("已清理 {count} 条记录和图片缓存"), false, cx)
                            });
                        }
                        Err(e) => {
                            let _ = this.update(cx, |this, cx| this.persistence_failed(e, cx));
                        }
                    }
                })
                .detach();
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn prepare_shutdown(&mut self) -> (Vec<Control>, crate::queue_store::Pending<()>) {
        let controls: Vec<_> = self
            .queue
            .active
            .values()
            .map(|a| a.control.clone())
            .collect();
        for control in &controls {
            control.stop(engine::PAUSE);
        }
        for item in &mut self.queue.items {
            if item.status == Status::Running && !item.simulated {
                item.status = Status::Paused;
                item.progress = None;
                item.error = Some("退出时已中断上传，继续时将重新上传。".into());
            }
        }
        let saved = if self.queue.persistence_ok {
            self.queue.queue_store.save(self.queue.items.clone())
        } else {
            self.queue.queue_store.barrier()
        };
        (controls, saved)
    }
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_prompt() {
            return;
        }
        if self.queue.active.is_empty() {
            cx.quit();
            return;
        }
        let prompt = window.prompt(
            PromptLevel::Warning,
            "还有图片正在上传",
            Some("可以等本批完成后退出，或暂停上传并保存队列。"),
            &["继续使用", "上传完成后退出", "暂停并退出"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            let answer = prompt.await.ok();
            let _ = this.update(cx, |this, cx| match answer {
                Some(1) => {
                    this.queue.quit_when_done = true;
                    if this.queue.active.is_empty() {
                        cx.quit();
                    } else {
                        this.message("本批上传完成后自动退出", false, cx);
                    }
                }
                Some(2) => cx.quit(),
                _ => {}
            });
        })
        .detach();
    }
    pub(super) fn simulate(&mut self, cx: &mut Context<Self>) {
        if self.simulating {
            return;
        }
        if !self
            .queue
            .items
            .iter()
            .any(|i| i.simulated && i.status != Status::Done)
        {
            self.queue.items.push(Item::fixture(
                "banner_spring.webp",
                3_711_959,
                "SM.MS",
                "queue-coast.png",
                0,
            ));
        }
        for item in &mut self.queue.items {
            if item.simulated && item.status != Status::Done {
                item.status = Status::Running;
            }
        }
        self.simulating = true;
        self.message("模拟上传已开始，仅演示进度，不会上传图片", false, cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(180))
                    .await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        let mut active = false;
                        for item in &mut this.queue.items {
                            if item.simulated && item.status == Status::Running {
                                let next = item.progress.unwrap_or(0).saturating_add(3).min(100);
                                item.progress = Some(next);
                                if next == 100 {
                                    item.status = Status::Done;
                                    this.records_revision += 1;
                                    item.url = Some(format!("https://example.com/{}", item.name));
                                } else {
                                    active = true;
                                }
                            }
                        }
                        this.simulating = active;
                        cx.notify();
                        active
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }
}
