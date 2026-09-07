use super::*;
use crate::desktop_runtime::{DesktopEvent, DesktopRuntime};

pub(super) enum QuickInput {
    Paths(Vec<PathBuf>),
    Bytes(Vec<u8>),
    Urls(Vec<String>),
    Screenshot,
}
pub(super) struct QuickRequest {
    pub id: String,
    pub target: String,
    pub preferences: Preferences,
    pub options: UploadOptions,
    pub configuration: Option<std::sync::Arc<model::UploadConfiguration>>,
    pub prepared: Option<(Vec<Item>, Vec<String>)>,
    pub control: Control,
    pub finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl ImgDesktop {
    pub fn desktop_notice(&mut self, text: String, cx: &mut Context<Self>) {
        self.message(text, true, cx);
    }
    pub fn hide_to_background(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !DesktopRuntime::can_hide(cx) {
            if cfg!(target_os = "macos") {
                self.message("菜单栏不可用，窗口保持打开；可按 ⌘Q 退出。", true, cx);
            } else {
                self.request_close(window, cx);
            }
            return;
        }
        self.visible = false;
        self.thumbnails
            .update(cx, |cache, cx| cache.clear(window, cx));
        if cfg!(target_os = "macos") {
            cx.hide();
        } else {
            window.minimize_window();
        }
    }
    pub fn show_from_background(
        &mut self,
        tag: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.visible = true;
        crate::native::restore_windows();
        cx.activate(true);
        window.activate_window();
        // Reopening must also paint a root that has not established entity
        // dependencies yet (for example, after nested AppKit startup events).
        window.refresh();
        if let Some(tag) = tag {
            self.show_page(Page::Queue, window, cx);
            if let Some(ids) = self.notification_batches.get(&tag) {
                let rows = self.filtered(cx);
                if let Some(index) = rows.iter().position(|id| ids.contains(id)) {
                    self.list_scroll.scroll_to_item(index, ScrollStrategy::Top);
                }
            }
        }
        cx.notify();
    }
    pub fn desktop_event(
        &mut self,
        event: DesktopEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shutting_down {
            return;
        }
        match event {
            #[cfg(target_os = "linux")]
            DesktopEvent::Notice(message) => self.message(message, false, cx),
            DesktopEvent::Open => self.show_from_background(None, window, cx),
            DesktopEvent::Hide => self.hide_to_background(window, cx),
            DesktopEvent::Quit => {
                self.show_from_background(None, window, cx);
                self.request_close(window, cx);
            }
            DesktopEvent::TogglePause => {
                if self.queue.batch.as_ref().is_some_and(|b| b.paused) {
                    self.resume_all(cx)
                } else if self.queue.batch.is_some() {
                    self.pause_all(cx)
                } else {
                    self.message("没有正在执行的批次", false, cx)
                }
            }
            DesktopEvent::Paste => {
                let Some(clipboard) = cx.read_from_clipboard() else {
                    self.message("剪贴板中没有图片或链接", true, cx);
                    return;
                };
                let input = clipboard
                    .entries()
                    .iter()
                    .find_map(|entry| match entry {
                        ClipboardEntry::ExternalPaths(paths) => {
                            Some(QuickInput::Paths(paths.0.to_vec()))
                        }
                        ClipboardEntry::Image(image) => {
                            Some(QuickInput::Bytes(image.bytes().to_vec()))
                        }
                        _ => None,
                    })
                    .or_else(|| {
                        clipboard.text().map(|text| {
                            QuickInput::Urls(
                                text.lines()
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                                    .collect(),
                            )
                        })
                    });
                if let Some(input) = input {
                    self.quick_import(input, window, cx)
                } else {
                    self.message("剪贴板中没有图片或链接", true, cx)
                }
            }
            DesktopEvent::Capture => {
                if self.quick_capture {
                    return;
                }
                self.quick_capture = true;
                self.quick_import(QuickInput::Screenshot, window, cx);
            }
            DesktopEvent::Hotkey(..) => {}
        }
    }
    fn quick_import(&mut self, input: QuickInput, window: &mut Window, cx: &mut Context<Self>) {
        let screenshot = matches!(input, QuickInput::Screenshot);
        if self.queue.quick.len() >= 50 {
            self.quick_capture = false;
            self.message("已有 50 批快捷操作等待处理，请稍后再试", true, cx);
            return;
        }
        let mut target = storage::configured_providers()
            .ok()
            .and_then(|(providers, default)| {
                providers
                    .iter()
                    .any(|(name, _)| name == &default)
                    .then_some(default)
            })
            .unwrap_or_default();
        let configuration = if target.is_empty() {
            None
        } else {
            match model::UploadConfiguration::capture(&target) {
                Ok(configuration) => Some(std::sync::Arc::new(configuration)),
                Err(_) => {
                    target.clear();
                    None
                }
            }
        };
        let id = uuid::Uuid::new_v4().to_string();
        let control = Control::default();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut preferences = self.preferences;
        preferences.auto_copy = true;
        let options = self.upload_options.clone();
        self.queue.quick.push_back(QuickRequest {
            id: id.clone(),
            target: target.clone(),
            preferences,
            options: options.clone(),
            configuration,
            prepared: None,
            control: control.clone(),
            finished: finished.clone(),
        });
        let root = self.root.clone();
        let binary = self.engine.clone();
        if screenshot && cfg!(target_os = "macos") {
            window.minimize_window();
        }
        let task = cx.background_executor().spawn(async move {
            struct Finished(std::sync::Arc<std::sync::atomic::AtomicBool>);
            impl Drop for Finished {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst)
                }
            }
            let _finished = Finished(finished);
            let result = (|| -> anyhow::Result<Vec<anyhow::Result<Item>>> {
                let prepare = |bytes| {
                    model::prepare_bytes_with_limit(bytes, "", &root, &target, options.max_bytes())
                };
                Ok(match input {
                    QuickInput::Bytes(bytes) => vec![prepare(bytes)],
                    QuickInput::Paths(paths) => {
                        anyhow::ensure!(
                            !paths.is_empty() && paths.len() <= model::MAX_BATCH,
                            "单次最多导入 50 张图片"
                        );
                        paths
                            .iter()
                            .map(|path| {
                                model::prepare_file_with_limit(
                                    path,
                                    &root,
                                    &target,
                                    options.max_bytes(),
                                )
                            })
                            .collect()
                    }
                    QuickInput::Urls(urls) => {
                        anyhow::ensure!(
                            !urls.is_empty()
                                && urls.len() <= model::MAX_BATCH
                                && urls.iter().all(|url| url::Url::parse(url)
                                    .is_ok_and(|u| matches!(u.scheme(), "https" | "http"))),
                            "请复制图片、图片文件或有效的图片链接"
                        );
                        urls.iter()
                            .map(|url| {
                                model::prepare_url_controlled(
                                    url, &root, &target, &binary, &options, &control,
                                )
                            })
                            .collect()
                    }
                    QuickInput::Screenshot => {
                        let dir = tempfile::tempdir()?;
                        let path = dir.path().join("screenshot.png");
                        let command = crate::platform::capture_command(&binary, &path);
                        let output = engine::run(command, &control)?;
                        if !path.is_file() || output.stopped != 0 {
                            vec![]
                        } else {
                            anyhow::ensure!(output.success, "截图失败，请检查屏幕录制权限");
                            vec![prepare(std::fs::read(path)?)]
                        }
                    }
                })
            })();
            let results = result.unwrap_or_else(|error| vec![Err(error)]);
            let mut items = vec![];
            let mut errors = vec![];
            for result in results {
                match result {
                    Ok(item) => items.push(item),
                    Err(_) => {
                        errors.push("图片导入失败，请检查格式、地址、大小或读取权限。".into())
                    }
                }
            }
            (items, errors)
        });
        cx.spawn_in(window, async move |this, cx| {
            let prepared = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.shutting_down {
                    return;
                }
                if screenshot {
                    this.quick_capture = false;
                    if this.visible {
                        crate::native::restore_windows();
                        window.activate_window();
                        cx.activate(true);
                    }
                }
                if let Some(request) = this.queue.quick.iter_mut().find(|request| request.id == id)
                {
                    request.prepared = Some(prepared);
                }
                this.accept_quick_imports(window, cx);
            });
        })
        .detach();
        self.message("正在准备快捷上传…", false, cx);
    }
    fn accept_quick_imports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(mut request) = self.queue.pop_prepared() {
            let (items, errors) = request.prepared.take().unwrap();
            let ids = items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
            self.queue.items.extend(items);
            if ids.is_empty() {
                if !errors.is_empty() {
                    self.message(errors.join("；"), true, cx);
                    self.notify_batch(request.id, vec![], 0, errors.len(), cx);
                }
                continue;
            }
            if !self.persist(cx) {
                continue;
            }
            if request.target.is_empty() {
                self.show_from_background(None, window, cx);
                self.show_page(Page::Settings, window, cx);
                self.message(
                    "图片已保留。请检查默认存储源与凭据，再到队列点击上传。",
                    true,
                    cx,
                );
                continue;
            }
            self.queue.submitted.push_back(queue::UploadBatch {
                id: request.id,
                pending: ids.clone().into(),
                order: ids,
                completed: vec![],
                failed: errors.len(),
                target: request.target,
                preferences: request.preferences,
                options: request.options,
                configuration: request.configuration,
                paused: false,
                cancelled: false,
            });
            self.message("快捷上传已提交，将按批次顺序执行", false, cx);
        }
        self.start_submitted(cx);
    }
    pub(super) fn start_submitted(&mut self, cx: &mut Context<Self>) {
        if self.queue.batch.is_none()
            && self.queue.persistence_ok
            && !self.queue.quit_when_done
            && let Some(batch) = self.queue.submitted.pop_front()
        {
            self.queue.batch = Some(batch);
            self.storage_settings.update(cx, |settings, cx| {
                settings.uploading = true;
                cx.notify();
            });
            self.upload_next(cx);
        }
    }
    pub(super) fn notify_batch(
        &mut self,
        id: String,
        order: Vec<String>,
        count: usize,
        failed: usize,
        cx: &mut Context<Self>,
    ) {
        if self.notification_batches.len() >= 100 {
            self.notification_batches.clear();
        }
        self.notification_batches.insert(id.clone(), order);
        cx.show_system_notification(SystemNotification {
            tag: id.into(),
            title: crate::i18n::text(if failed > 0 {
                "img · 上传有失败项"
            } else {
                "img · 上传完成"
            }),
            body: crate::i18n::text(format!(
                "{count} 张上传成功，{failed} 张失败。点击查看队列。"
            )),
            actions: vec![],
        });
    }
}
