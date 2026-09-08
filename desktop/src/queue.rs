use super::*;

pub(super) struct UploadBatch {
    pub(super) id: String,
    pub(super) pending: VecDeque<String>,
    pub(super) completed: Vec<Item>,
    pub(super) failed: usize,
    pub(super) target: String,
    pub(super) preferences: Preferences,
    pub(super) options: UploadOptions,
    pub(super) configuration: Option<std::sync::Arc<model::UploadConfiguration>>,
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
    pub(super) auxiliary: Vec<Control>,
    pub(super) submitted: VecDeque<UploadBatch>,
    pub(super) quick: VecDeque<super::quick_upload::QuickRequest>,
    pub(super) quit_when_done: bool,
    pub(super) persistence_ok: bool,
    pub(super) persistence_generation: u64,
    pub(super) queue_store: crate::queue_store::QueueStore,
}
impl UploadController {
    fn removable_ids(&self, ids: &[String]) -> Vec<String> {
        let requested: std::collections::HashSet<_> = ids.iter().collect();
        let mut protected: std::collections::HashSet<&String> = self.active.keys().collect();
        for batch in &self.submitted {
            protected.extend(batch.order.iter());
        }
        if let Some(batch) = &self.batch {
            protected.extend(batch.pending.iter());
        }
        self.items
            .iter()
            .filter(|item| {
                requested.contains(&item.id)
                    && item.status != Status::Running
                    && !protected.contains(&item.id)
            })
            .map(|item| item.id.clone())
            .collect()
    }
    pub(super) fn manual_ids(&self, statuses: &[Status]) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| {
                !item.simulated
                    && statuses.contains(&item.status)
                    && !self
                        .submitted
                        .iter()
                        .any(|batch| batch.order.contains(&item.id))
            })
            .map(|item| item.id.clone())
            .collect()
    }
    pub(super) fn pop_prepared(&mut self) -> Option<super::quick_upload::QuickRequest> {
        self.quick
            .front()
            .is_some_and(|request| request.prepared.is_some())
            .then(|| self.quick.pop_front().unwrap())
    }
    pub(super) fn new(root: PathBuf, items: Vec<Item>, persistence_ok: bool) -> Self {
        Self {
            items,
            batch: None,
            active: HashMap::new(),
            auxiliary: vec![],
            submitted: VecDeque::new(),
            quick: VecDeque::new(),
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
        let ids = self.queue.manual_ids(&[Status::Ready]);
        self.begin_upload(ids, cx);
    }
    pub(super) fn begin_upload(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        if self.shutting_down {
            return;
        }
        if self.queue.batch.is_some() {
            self.message("请先完成、暂停或取消当前批次", false, cx);
            return;
        }
        if ids.is_empty() {
            self.message("没有可以上传的图片", false, cx);
            return;
        }
        if !self.providers.iter().any(|(p, _)| p == &self.provider) {
            self.message("请在存储源页面添加并选择图床，再开始上传", true, cx);
            return;
        }
        self.queue.batch = Some(UploadBatch {
            id: uuid::Uuid::new_v4().to_string(),
            pending: ids.clone().into(),
            order: ids,
            completed: vec![],
            failed: 0,
            target: self.provider.clone(),
            preferences: self.preferences,
            options: self.upload_options.clone(),
            configuration: None,
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
        self.notify_batch(batch.id, batch.order, count, batch.failed, cx);
        if self.queue.quit_when_done {
            self.begin_shutdown(cx);
        } else {
            self.start_submitted(cx);
        }
    }
    pub(super) fn upload_next(&mut self, cx: &mut Context<Self>) {
        if self.shutting_down {
            return;
        }
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
            let configuration = batch.configuration.clone();
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
                                if this.visible {
                                    cx.notify();
                                }
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
                let result = match crate::queue_store::acknowledged(saved).await {
                    Ok(()) => model::upload(
                        &item,
                        &root,
                        &engine,
                        &options,
                        &control,
                        configuration.as_deref(),
                    ),
                    Err(error) => Err(error),
                };
                // Setup and persistence can fail before engine::run creates its finish guard.
                control.finished.store(true, Ordering::SeqCst);
                result
            });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    let active = this.queue.active.remove(&id);
                    if this.shutting_down {
                        return;
                    }
                    if let Some(item) = this.queue.items.iter_mut().find(|i| i.id == id) {
                        match result {
                            Ok(model::UploadOutcome::Done(result)) => {
                                item.status = Status::Done;
                                item.progress = Some(100);
                                item.url = Some(result.url);
                                item.uploaded_size = result.size;
                                item.catalog_asset_id = result.asset_id;
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
            let ids = self.queue.manual_ids(&[Status::Ready, Status::Paused]);
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
        self.remove_records_with_hidden(ids, 0, window, cx);
    }
    pub(super) fn remove_records_with_hidden(
        &mut self,
        ids: Vec<String>,
        hidden: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = self.queue.removable_ids(&ids);
        let hidden = if hidden > 0 {
            ids.iter()
                .filter(|id| !self.record_index.borrow().visible(id))
                .count()
        } else {
            0
        };
        if ids.is_empty() {
            self.message("没有可清理的记录，请先取消正在等待的任务", false, cx);
            return;
        }
        let prompt = crate::i18n::prompt(
            window,
            PromptLevel::Warning,
            &format!(
                "清理 {} 条记录？其中 {hidden} 项不在当前搜索结果中",
                ids.len()
            ),
            Some("删除这些记录和应用内的图片缓存。你选择的原始文件与远端图片会保留。"),
            &["取消", "清理"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                let ids = this.queue.removable_ids(&ids);
                if ids.is_empty() {
                    this.message("所选记录已不存在或正在上传，没有清理记录", false, cx);
                    return;
                }
                let ids: std::collections::HashSet<_> = ids.into_iter().collect();
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
                    match cx
                        .background_executor()
                        .spawn(crate::queue_store::finish_removal(saved, root, removed))
                        .await
                    {
                        Ok(count) => {
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
        if self.shutdown_saved {
            return (vec![], self.queue.queue_store.barrier());
        }
        let mut controls: Vec<_> = self
            .queue
            .active
            .values()
            .map(|a| a.control.clone())
            .collect();
        controls.extend(
            self.queue
                .auxiliary
                .iter()
                .filter(|control| !control.finished.load(Ordering::SeqCst))
                .cloned(),
        );
        for request in &self.queue.quick {
            let mut control = request.control.clone();
            control.finished = request.finished.clone();
            controls.push(control);
        }
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
        if self.shutting_down {
            return;
        }
        if window.has_active_prompt() {
            return;
        }
        if !self.queue.persistence_ok {
            let prompt = crate::i18n::prompt(
                window,
                PromptLevel::Warning,
                "本地队列尚未保存",
                Some(
                    "退出会保留原队列和图片副本；本次未能保存的记录需要重新导入。也可以继续修复队列。",
                ),
                &["继续修复", "退出并保留原文件"],
                cx,
            );
            cx.spawn(async move |this, cx| {
                if prompt.await.ok() == Some(1) {
                    let _ = this.update(cx, |this, cx| this.shutdown_with_mode(true, cx));
                }
            })
            .detach();
            return;
        }
        if self.queue.active.is_empty() {
            self.begin_shutdown(cx);
            return;
        }
        let prompt = crate::i18n::prompt(
            window,
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
                        this.begin_shutdown(cx);
                    } else {
                        this.message("本批上传完成后自动退出", false, cx);
                    }
                }
                Some(2) => this.begin_shutdown(cx),
                _ => {}
            });
        })
        .detach();
    }
    pub(super) fn begin_shutdown(&mut self, cx: &mut Context<Self>) {
        self.shutdown_with_mode(false, cx);
    }
    fn shutdown_with_mode(&mut self, allow_unsaved: bool, cx: &mut Context<Self>) {
        if self.shutting_down {
            return;
        }
        self.queue
            .auxiliary
            .extend(self.tools.update(cx, |tools, _| tools.stop()));
        self.queue
            .auxiliary
            .extend(self.catalog.update(cx, |library, _| library.stop()));
        self.queue
            .auxiliary
            .extend(self.tasks.update(cx, |panel, cx| panel.stop(cx)));
        if let Some(control) = self.sync_panel.update(cx, |panel, _| panel.stop()) {
            self.queue.auxiliary.push(control);
        }
        self.shutting_down = true;
        let (controls, saved) = self.prepare_shutdown();
        self.message("正在保存队列并结束后台进程…", false, cx);
        cx.spawn(async move |this, cx| {
            let result = engine::wait_for_shutdown(saved, &controls, || {
                cx.background_executor().timer(Duration::from_millis(25))
            })
            .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    if this.pending_restore
                        && let Err(error) = crate::backup::restart(&this.root)
                    {
                        this.pending_restore = false;
                        this.shutting_down = false;
                        this.message(error.to_string(), true, cx);
                        return;
                    }
                    if let Some(install) = this.pending_install.take()
                        && let Err(error) = install.launch()
                    {
                        this.shutting_down = false;
                        this.update_notice = Some((error.to_string(), true));
                        cx.notify();
                        return;
                    }
                    this.shutdown_saved = true;
                    cx.quit();
                }
                Err(_) if allow_unsaved => {
                    this.shutdown_saved = true;
                    cx.quit();
                }
                Err(error) => {
                    this.pending_restore = false;
                    this.pending_install = None;
                    this.shutting_down = false;
                    this.persistence_failed(error, cx);
                }
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

#[cfg(test)]
mod tests {
    use super::{UploadBatch, UploadController};
    use crate::{
        engine::Control,
        model::{Item, Status},
        preferences::Preferences,
        upload_options::UploadOptions,
    };
    #[test]
    fn removal_rechecks_frozen_ids_and_upload_protection() {
        let root = tempfile::tempdir().unwrap();
        let mut items = Item::reference_items();
        for item in &mut items {
            item.status = Status::Done;
        }
        let mut queue = UploadController::new(root.path().into(), items.clone(), true);
        let frozen: Vec<_> = items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(queue.removable_ids(&frozen), frozen);
        queue.items[0].status = Status::Running;
        assert_eq!(queue.removable_ids(&frozen), vec![items[1].id.clone()]);
        queue.items.clear();
        assert!(queue.removable_ids(&frozen).is_empty());
        queue.items = items;
        queue.batch = Some(UploadBatch {
            id: "protected".into(),
            pending: frozen.clone().into(),
            order: frozen.clone(),
            completed: vec![],
            failed: 0,
            target: String::new(),
            preferences: Preferences::default(),
            options: UploadOptions::default(),
            configuration: None,
            paused: true,
            cancelled: false,
        });
        assert!(queue.removable_ids(&frozen).is_empty());
        queue.submitted.push_back(queue.batch.take().unwrap());
        assert!(queue.removable_ids(&frozen).is_empty());
    }
    #[test]
    fn quick_preparation_keeps_trigger_order_and_manual_records_separate() {
        let root = tempfile::tempdir().unwrap();
        let mut manual = Item::reference_items().remove(0);
        manual.id = "manual".into();
        manual.status = Status::Ready;
        manual.simulated = false;
        let mut quick = manual.clone();
        quick.id = "quick".into();
        let mut queue = UploadController::new(root.path().to_owned(), vec![manual, quick], true);
        queue.submitted.push_back(UploadBatch {
            id: "batch".into(),
            pending: vec!["quick".into()].into(),
            order: vec!["quick".into()],
            completed: vec![],
            failed: 0,
            target: "captured".into(),
            preferences: Preferences::default(),
            options: UploadOptions::default(),
            configuration: None,
            paused: false,
            cancelled: false,
        });
        assert_eq!(queue.manual_ids(&[Status::Ready]), vec!["manual"]);
        for (id, ready) in [("first", false), ("second", true)] {
            queue
                .quick
                .push_back(super::super::quick_upload::QuickRequest {
                    id: id.into(),
                    target: "captured".into(),
                    preferences: Preferences::default(),
                    options: UploadOptions::default(),
                    configuration: None,
                    prepared: ready.then(|| (vec![], vec![])),
                    control: Control::default(),
                    finished: Default::default(),
                });
        }
        assert!(queue.pop_prepared().is_none());
        queue.quick.front_mut().unwrap().prepared = Some((vec![], vec![]));
        assert_eq!(queue.pop_prepared().unwrap().id, "first");
        assert_eq!(queue.pop_prepared().unwrap().id, "second");
        assert!(queue.pop_prepared().is_none());
    }
}
