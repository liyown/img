#[path = "diagnostic_ui.rs"]
mod diagnostic_ui;
#[path = "gallery_ui.rs"]
mod gallery_ui;
#[cfg(feature = "perf")]
#[path = "performance.rs"]
mod performance;
#[path = "queue.rs"]
mod queue;
#[path = "quick_upload.rs"]
mod quick_upload;

use crate::{
    assets::icon,
    engine::{self, Control},
    model::{self, Item, Status},
    preferences::{CopyFormat, LibraryView, Preferences},
    settings::{EditorClosed, StorageChanged, StorageSettings},
    storage,
    theme::*,
    upload_options::UploadOptions,
    upload_settings::{UploadOptionsChanged, UploadSettings},
};
use gpui_kit::{
    component::{
        button::*,
        input::{Input, InputEvent, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        switch::Switch,
        *,
    },
    prelude::*,
    *,
};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::atomic::Ordering,
    time::Duration,
};

gpui_kit::actions!(
    aperture,
    [
        ChooseFiles,
        PasteImage,
        CaptureImage,
        Search,
        Dismiss,
        Quit,
        CloseWindow,
        ToggleSidebar,
        OpenWindow,
        QuickPaste,
        QuickCapture,
        ToggleQueue
    ]
);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Queue,
    Library,
    History,
    Settings,
}
#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Running,
    Done,
    Failed,
}

pub struct ImgDesktop {
    shutting_down: bool,
    shutdown_saved: bool,
    visible: bool,
    quick_capture: bool,
    notification_batches: HashMap<String, Vec<String>>,
    shortcut_settings: Entity<crate::shortcut_settings::ShortcutSettings>,
    focus: FocusHandle,
    search: Entity<InputState>,
    url_input: Entity<InputState>,
    show_url_input: bool,
    _subscriptions: Vec<Subscription>,
    queue: queue::UploadController,
    records_revision: u64,
    selection: crate::selection::Selection,
    record_index: std::cell::RefCell<crate::record_index::RecordIndex>,
    list_scroll: UniformListScrollHandle,
    #[cfg(feature = "perf")]
    legacy_scroll: ScrollHandle,
    thumbnails: Entity<crate::thumbnails::ThumbnailCache>,
    root: PathBuf,
    engine: PathBuf,
    providers: Vec<(String, String)>,
    provider: String,
    page: Page,
    back_stack: Vec<Page>,
    forward_stack: Vec<Page>,
    content_revision: usize,
    trace_motion: bool,
    filter: Filter,
    preferences: Preferences,
    storage_settings: Entity<StorageSettings>,
    upload_options: UploadOptions,
    upload_settings: Entity<UploadSettings>,
    update_checking: bool,
    update_downloading: bool,
    available_update: Option<crate::updates::Update>,
    update_file: Option<(PathBuf, String)>,
    pending_install: Option<crate::installer::PreparedInstall>,
    install_preparing: bool,
    update_notice: Option<(String, bool)>,
    reference: bool,
    preparing: bool,
    simulating: bool,
    notice: Option<(String, bool)>,
    copied: Option<String>,
}

// Render overlays from a separate entity so a dialog can read the desktop's
// current copy preferences without re-entering its active render borrow.
pub struct DesktopShell(pub Entity<ImgDesktop>);

impl Render for DesktopShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .child(self.0.clone())
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn label(text: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .font_weight(FontWeight::NORMAL)
        .text_color(rgb(color))
        .child(text.into())
}
fn mono(text: impl Into<SharedString>, size: f32, color: u32) -> Div {
    label(text, size, color).font_family("Menlo")
}
fn dot(color: u32) -> Div {
    div()
        .size(px(8.))
        .flex_shrink_0()
        .rounded_full()
        .bg(rgb(color))
}
fn action(id: impl Into<ElementId>, text: &str) -> Button {
    Button::new(id)
        .when(!text.is_empty(), |this| this.label(text.to_string()))
        .small()
        .font_weight(FontWeight::NORMAL)
        .h(px(36.))
        .px(px(16.))
        .rounded(px(10.))
        .text_size(px(13.))
}
fn progress(value: Option<u8>, color: u32) -> Div {
    div()
        .w_full()
        .h(px(6.))
        .flex_shrink_0()
        .bg(rgb(TRACK))
        .rounded_full()
        .overflow_hidden()
        .when_some(value, |this, value| {
            this.child(
                div()
                    .h_full()
                    .w(relative(value as f32 / 100.))
                    .rounded_full()
                    .bg(rgb(color))
                    .when(color == ORANGE || color == ORANGE_BRIGHT, |this| {
                        this.bg(linear_gradient(
                            90.,
                            linear_color_stop(rgb(ORANGE), 0.),
                            linear_color_stop(rgb(0xffbb00), 1.),
                        ))
                    }),
            )
        })
}
fn dashed_outline() -> impl IntoElement {
    // GPUI's stock dashed quad uses a short dotted cadence. The reference
    // uses 6 px dashes and 4 px gaps around a 16 px rounded rectangle.
    canvas(
        |bounds, _, _| bounds,
        |bounds, _, window, _| {
            let left = bounds.left() + px(1.);
            let top = bounds.top() + px(1.);
            let right = bounds.right() - px(1.);
            let bottom = bounds.bottom() - px(1.);
            let r = px(15.);
            let mut path = PathBuilder::stroke(px(1.5)).dash_array(&[px(6.), px(4.)]);
            path.move_to(point(left + r, top));
            path.line_to(point(right - r, top));
            path.curve_to(point(right, top + r), point(right, top));
            path.line_to(point(right, bottom - r));
            path.curve_to(point(right - r, bottom), point(right, bottom));
            path.line_to(point(left + r, bottom));
            path.curve_to(point(left, bottom - r), point(left, bottom));
            path.line_to(point(left, top + r));
            path.curve_to(point(left + r, top), point(left, top));
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, rgb(ORANGE_BORDER));
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

fn thumbnail(item: &Item, cache: &Entity<crate::thumbnails::ThumbnailCache>) -> AnyElement {
    let use_cache = !cfg!(feature = "perf") || std::env::var_os("IMG_PERF_BASELINE").is_none();
    if let Some(path) = &item.thumbnail {
        img(path.clone())
            .when(use_cache, |image| image.image_cache(cache))
            .size_full()
            .object_fit(ObjectFit::Cover)
            .rounded(px(10.))
            .into_any_element()
    } else if item.asset.is_empty() {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(DROP))
            .child(icon("image", 24.).text_color(rgb(MUTED)))
            .into_any_element()
    } else {
        img(item.asset.clone())
            .image_cache(cache)
            .size_full()
            .object_fit(ObjectFit::Cover)
            .rounded(px(10.))
            .into_any_element()
    }
}

impl ImgDesktop {
    pub fn new(
        root: PathBuf,
        engine: PathBuf,
        reference: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索文件、存储源或 URL..."));
        let url_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://example.com/image.png"));
        let subscription = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let loaded = model::load(&root);
        let persistence_ok = loaded.is_ok();
        let mut notice = loaded.as_ref().err().map(|e| (e.to_string(), true));
        let preferences = Preferences::load(&root).unwrap_or_else(|_| {
            notice = Some(("显示偏好无法读取，已使用默认设置".into(), true));
            Preferences::default()
        });
        let mut items = loaded.unwrap_or_default();
        if reference {
            let mut fixtures = Item::reference_items();
            fixtures.append(&mut items);
            items = fixtures;
        }
        let (providers, configured) = storage::configured_providers().unwrap_or_else(|e| {
            if notice.is_none() {
                notice = Some((e.to_string(), true));
            }
            Default::default()
        });
        let provider = if reference {
            "SM.MS".into()
        } else if providers.iter().any(|(p, _)| p == &configured) {
            configured
        } else {
            providers.first().map(|p| p.0.clone()).unwrap_or_default()
        };
        focus.focus(window, cx);
        let storage_settings = cx.new(|_| StorageSettings::new(engine.clone()));
        let upload_options = UploadOptions::load(&root).unwrap_or_else(|_| {
            notice = Some(("上传设置无法读取，已使用默认值".into(), true));
            UploadOptions::default()
        });
        let upload_settings =
            cx.new(|cx| UploadSettings::new(root.clone(), upload_options.clone(), window, cx));
        let upload_subscription = cx.subscribe(
            &upload_settings,
            |this, _, event: &UploadOptionsChanged, cx| {
                this.upload_options = event.0.clone();
                cx.notify();
            },
        );
        let shortcut_settings =
            cx.new(|cx| crate::shortcut_settings::ShortcutSettings::new(&root, window, cx));
        let quit_subscription = cx.on_app_quit(|this, cx| {
            let (controls, saved) = this.prepare_shutdown();
            let executor = cx.background_executor().clone();
            async move {
                let _ = crate::queue_store::acknowledged(saved).await;
                while controls.iter().any(|c| !c.finished.load(Ordering::SeqCst)) {
                    executor.timer(Duration::from_millis(25)).await;
                }
            }
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(3600))
                    .await;
                if this
                    .update(cx, |this, cx| this.startup_update_check(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let release_subscription = cx.on_release(|this, _| {
            let _ = this.prepare_shutdown();
        });
        let storage_focus_subscription = cx.subscribe_in(
            &storage_settings,
            window,
            |this, _, _: &EditorClosed, window, cx| {
                if this.page == Page::Settings {
                    this.focus.focus(window, cx);
                }
            },
        );
        let storage_subscription =
            cx.subscribe(&storage_settings, |this, _, _: &StorageChanged, cx| {
                match storage::configured_providers() {
                    Ok((providers, current)) => {
                        this.providers = providers;
                        this.provider = current;
                    }
                    Err(e) => this.message(e.to_string(), true, cx),
                }
                cx.notify();
            });
        let mut subscriptions = vec![
            subscription,
            storage_subscription,
            storage_focus_subscription,
            upload_subscription,
            quit_subscription,
            release_subscription,
        ];
        if std::env::var_os("APERTURE_TRACE_WINDOW").is_some() {
            eprintln!("window-bounds {:?}", window.window_bounds());
            subscriptions.push(cx.observe_window_bounds(window, |_, window, _| {
                eprintln!("window-bounds {:?}", window.window_bounds());
            }));
        }
        Self {
            shutting_down: false,
            shutdown_saved: false,
            visible: true,
            quick_capture: false,
            notification_batches: HashMap::new(),
            shortcut_settings,
            focus,
            search,
            url_input,
            show_url_input: false,
            _subscriptions: subscriptions,
            queue: queue::UploadController::new(root.clone(), items, persistence_ok),
            records_revision: 1,
            record_index: Default::default(),
            selection: Default::default(),
            list_scroll: UniformListScrollHandle::new(),
            #[cfg(feature = "perf")]
            legacy_scroll: ScrollHandle::new(),
            thumbnails: crate::thumbnails::ThumbnailCache::new(root.clone(), cx),
            root,
            engine,
            providers,
            provider,
            page: Page::Queue,
            back_stack: vec![],
            forward_stack: vec![],
            content_revision: 0,
            trace_motion: std::env::var_os("APERTURE_TRACE_MOTION").is_some(),
            filter: Filter::All,
            preferences,
            storage_settings,
            upload_options,
            upload_settings,
            update_checking: false,
            update_downloading: false,
            available_update: None,
            update_file: None,
            pending_install: None,
            install_preparing: false,
            update_notice: None,
            reference,
            preparing: false,
            simulating: false,
            notice,
            copied: None,
        }
    }
    fn message(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        let text = text.into();
        crate::desktop_runtime::DesktopRuntime::status(
            cx,
            &text,
            self.queue.batch.as_ref().is_some_and(|b| b.paused),
        );
        self.notice = Some((text, error));
        if self.visible {
            cx.notify();
        }
    }
    fn persist(&mut self, cx: &mut Context<Self>) -> bool {
        self.records_revision += 1;
        if !self.queue.persistence_ok {
            self.message("本地队列需要修复，当前无法保存或开始上传。", true, cx);
            return false;
        }
        let saved = self.queue.queue_store.save(self.queue.items.clone());
        let generation = self.queue.persistence_generation;
        cx.spawn(async move |this, cx| {
            if let Err(e) = crate::queue_store::acknowledged(saved).await {
                let _ = this.update(cx, |this, cx| {
                    if generation == this.queue.persistence_generation {
                        this.persistence_failed(e, cx);
                    }
                });
            }
        })
        .detach();
        true
    }
    fn navigate(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != page {
            self.back_stack.push(self.page);
            self.forward_stack.clear();
        }
        self.show_page(page, window, cx);
    }
    fn show_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.thumbnails
            .update(cx, |cache, cx| cache.clear(window, cx));
        self.list_scroll = UniformListScrollHandle::new();
        if self.page != page {
            self.content_revision = self.content_revision.wrapping_add(1);
        }
        if page != Page::Library {
            self.selection.finish();
        }
        self.page = page;
        self.filter = Filter::All;
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.focus.focus(window, cx);
        cx.notify();
    }
    fn pick_files(&mut self, _: &ChooseFiles, _: &mut Window, cx: &mut Context<Self>) {
        if self.preparing {
            return;
        }
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择图片".into()),
        });
        cx.spawn(async move |this, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                let _ = this.update(cx, |this, cx| this.import_paths(paths, cx));
            }
            Ok(Ok(None)) => {}
            _ => {
                let _ = this.update(cx, |this, cx| this.message("无法打开文件选择器", true, cx));
            }
        })
        .detach();
    }
    fn import_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.preparing {
            self.message("正在读取图片，请稍候", false, cx);
            return;
        }
        if paths.is_empty() {
            return;
        }
        if paths.len() > model::MAX_BATCH {
            self.message("单次最多选择 50 张图片", true, cx);
            return;
        }
        self.preparing = true;
        self.page = Page::Queue;
        self.filter = Filter::All;
        let root = self.root.clone();
        let target = self.provider.clone();
        let max_bytes = self.upload_options.max_bytes();
        let task = cx.background_executor().spawn(async move {
            paths
                .into_iter()
                .map(|path| {
                    model::prepare_file_with_limit(&path, &root, &target, max_bytes).map_err(|e| {
                        format!(
                            "{}：{e}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )
                    })
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let results = task.await;
            let _ = this.update(cx, |this, cx| {
                this.preparing = false;
                let mut errors = vec![];
                let mut count = 0;
                for result in results {
                    match result {
                        Ok(item) => {
                            this.queue.items.push(item);
                            count += 1;
                        }
                        Err(e) => errors.push(e),
                    }
                }
                if !this.persist(cx) {
                    return;
                }
                if errors.is_empty() {
                    this.message(
                        format!("已添加 {count} 张图片，可在队列中开始上传"),
                        false,
                        cx,
                    );
                } else {
                    this.message(
                        format!("已添加 {count} 张；{}", errors.join("；")),
                        true,
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn paste_image(&mut self, _: &PasteImage, _: &mut Window, cx: &mut Context<Self>) {
        if self.preparing {
            return;
        }
        let Some(clipboard) = cx.read_from_clipboard() else {
            self.message("剪贴板中没有图片", false, cx);
            return;
        };
        for entry in clipboard.entries() {
            if let ClipboardEntry::ExternalPaths(paths) = entry {
                self.import_paths(paths.0.to_vec(), cx);
                return;
            }
            if let ClipboardEntry::Image(image) = entry {
                let bytes = image.bytes().to_vec();
                let root = self.root.clone();
                let target = self.provider.clone();
                let max_bytes = self.upload_options.max_bytes();
                self.preparing = true;
                let task = cx.background_executor().spawn(async move {
                    model::prepare_bytes_with_limit(bytes, "", &root, &target, max_bytes)
                });
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = this.update(cx, |this, cx| {
                        this.preparing = false;
                        match result {
                            Ok(item) => {
                                this.queue.items.push(item);
                                this.page = Page::Queue;
                                this.filter = Filter::All;
                                if this.persist(cx) {
                                    this.message("剪贴板图片已添加到队列", false, cx);
                                }
                            }
                            Err(e) => this.message(e.to_string(), true, cx),
                        }
                    });
                })
                .detach();
                cx.notify();
                return;
            }
        }
        if let Some(text) = clipboard.text() {
            self.import_urls(&text, cx);
            return;
        }
        self.message("请先复制图片、图片文件或图片链接，再按 ⌘V 粘贴", false, cx);
    }
    fn import_urls(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.preparing {
            return;
        }
        let urls: Vec<_> = text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if urls.is_empty()
            || urls.len() > model::MAX_BATCH
            || urls.iter().any(|s| {
                url::Url::parse(s).is_err()
                    || !(s.starts_with("https://") || s.starts_with("http://"))
            })
        {
            self.message("请粘贴有效的图片链接，每行一个，最多 50 个", true, cx);
            return;
        }
        self.preparing = true;
        self.page = Page::Queue;
        self.filter = Filter::All;
        let root = self.root.clone();
        let target = self.provider.clone();
        let binary = self.engine.clone();
        let options = self.upload_options.clone();
        self.message("正在下载链接中的图片…", false, cx);
        let control = Control::default();
        self.queue
            .auxiliary
            .retain(|control| !control.finished.load(Ordering::SeqCst));
        self.queue.auxiliary.push(control.clone());
        let task = cx.background_executor().spawn(async move {
            let _completion = control.completion();
            urls.iter()
                .map(|url| {
                    model::prepare_url_controlled(
                        url,
                        &root,
                        &target,
                        &binary,
                        &options,
                        &control.child(),
                    )
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let results = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.shutting_down {
                    return;
                }
                this.preparing = false;
                let mut count = 0;
                let mut errors = vec![];
                for result in results {
                    match result {
                        Ok(item) => {
                            this.queue.items.push(item);
                            count += 1;
                        }
                        Err(e) => errors.push(e.to_string()),
                    }
                }
                if this.persist(cx) {
                    this.message(
                        format!(
                            "已导入 {count} 张链接图片{}",
                            if errors.is_empty() {
                                String::new()
                            } else {
                                format!("；{}", errors.join("；"))
                            }
                        ),
                        !errors.is_empty(),
                        cx,
                    );
                }
            });
        })
        .detach();
    }
    fn capture(&mut self, _: &CaptureImage, window: &mut Window, cx: &mut Context<Self>) {
        if self.preparing {
            return;
        }
        {
            self.preparing = true;
            if cfg!(target_os = "macos") {
                window.minimize_window();
            }
            let binary = self.engine.clone();
            let control = Control::default();
            self.queue
                .auxiliary
                .retain(|control| !control.finished.load(Ordering::SeqCst));
            self.queue.auxiliary.push(control.clone());
            let task = cx.background_executor().spawn(async move {
                let _completion = control.completion();
                let directory = tempfile::tempdir()?;
                let path = directory.path().join("screenshot.png");
                let command = crate::platform::capture_command(&binary, &path);
                let status = engine::run(command, &control)?;
                if !path.exists() {
                    return Ok::<_, anyhow::Error>(None);
                }
                anyhow::ensure!(status.success, "截屏未完成，请检查系统屏幕录制权限");
                Ok(Some(std::fs::read(path)?))
            });
            let root = self.root.clone();
            let target = self.provider.clone();
            let max_bytes = self.upload_options.max_bytes();
            cx.spawn(async move |this, cx| {
                let captured = task.await;
                let result = match captured {
                    Ok(Some(bytes)) => Some(
                        cx.background_executor()
                            .spawn(async move {
                                model::prepare_bytes_with_limit(
                                    bytes, "", &root, &target, max_bytes,
                                )
                            })
                            .await,
                    ),
                    Ok(None) => None,
                    Err(e) => Some(Err(e)),
                };
                let _ = this.update(cx, |this, cx| {
                    if this.shutting_down {
                        return;
                    }
                    this.preparing = false;
                    crate::native::restore_windows();
                    cx.activate(true);
                    match result {
                        Some(Ok(item)) => {
                            this.queue.items.push(item);
                            this.page = Page::Queue;
                            if this.persist(cx) {
                                this.message("截图已添加到队列", false, cx);
                            }
                        }
                        Some(Err(e)) => this.message(e.to_string(), true, cx),
                        None => cx.notify(),
                    }
                });
            })
            .detach();
        }
    }
    fn copy(&mut self, item: &Item, cx: &mut Context<Self>) {
        if let Some(url) = &item.url {
            let format = self.preferences.copy_format;
            cx.write_to_clipboard(ClipboardItem::new_string(format.render(&item.name, url)));
            self.copied = Some(item.id.clone());
            self.message(
                if item.simulated {
                    format!("已复制为 {} · 示例链接（example.com）", format.label())
                } else {
                    format!("已复制为 {}", format.label())
                },
                false,
                cx,
            );
        }
    }
    fn save_preferences(&mut self, cx: &mut Context<Self>) {
        if self.preferences.save(&self.root).is_err() {
            self.message("当前选择已生效，但偏好未能保存", true, cx);
        }
        cx.notify();
    }
    fn copy_actions(&self, item: &Item, scope: &str, cx: &mut Context<Self>) -> AnyElement {
        let copy = item.clone();
        let menu_item = item.clone();
        let selected = self.preferences.copy_format;
        let weak = cx.entity().downgrade();
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .child(
                Button::new(SharedString::from(format!("{scope}-copy-{}", item.id)))
                    .ghost()
                    .xsmall()
                    .h(px(26.))
                    .px(px(5.))
                    .text_color(rgb(ORANGE))
                    .text_size(px(11.))
                    .label(if self.copied.as_ref() == Some(&item.id) {
                        "已复制"
                    } else {
                        "复制链接"
                    })
                    .tooltip(format!("复制为 {}", selected.label()))
                    .on_click(cx.listener(move |this, _, _, cx| this.copy(&copy, cx))),
            )
            .child(
                Button::new(SharedString::from(format!("{scope}-format-{}", item.id)))
                    .accessibility_label(format!("复制格式：{}", item.name))
                    .ghost()
                    .xsmall()
                    .h(px(26.))
                    .w(px(22.))
                    .p_0()
                    .text_color(rgb(ORANGE))
                    .child(icon("caret-down", 10.).text_color(rgb(ORANGE)))
                    .tooltip("选择格式并复制")
                    .dropdown_menu(move |mut menu, _, _| {
                        menu = menu
                            .min_w(px(190.))
                            .item(PopupMenuItem::label("选择格式并复制"));
                        for format in CopyFormat::ALL {
                            let weak = weak.clone();
                            let item = menu_item.clone();
                            menu = menu.item(
                                PopupMenuItem::new(format.label())
                                    .checked(format == selected)
                                    .on_click(move |_, _, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.preferences.copy_format = format;
                                            this.copy(&item, cx);
                                            this.save_preferences(cx);
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
    fn filtered(&self, cx: &App) -> std::sync::Arc<Vec<String>> {
        self.record_index.borrow_mut().rows(
            &self.queue.items,
            self.records_revision,
            &self.search.read(cx).value(),
            self.page as u8,
            self.filter as u8,
        )
    }
    fn indexed_item(&self, id: &str) -> Option<Item> {
        self.record_index
            .borrow()
            .position(id)
            .and_then(|n| self.queue.items.get(n))
            .cloned()
    }
    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.preferences.sidebar_collapsed = !self.preferences.sidebar_collapsed;
        self.save_preferences(cx);
    }
    fn sidebar(&self, active_y: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let done = self
            .queue
            .items
            .iter()
            .filter(|i| i.status == Status::Done)
            .count();
        let mut nav = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(10.))
            .pt(px(12.))
            .child(
                div()
                    .absolute()
                    .top(active_y)
                    .left(px(10.))
                    .w(px(220.))
                    .h(px(36.))
                    .rounded(px(7.))
                    .bg(rgb(NAV_SELECTED)),
            );
        for (page, title, symbol, count) in [
            (
                Page::Library,
                "图库",
                "image",
                Some(if self.reference { 128 } else { done }),
            ),
            (
                Page::Queue,
                "上传队列",
                "upload-simple",
                Some(self.queue.items.len()),
            ),
            (Page::History, "历史记录", "clock-counter-clockwise", None),
            (Page::Settings, "设置", "gear", None),
        ] {
            let active = self.page == page;
            let color = if active { TEXT } else { NAV_TEXT };
            nav = nav.child(
                Button::new(SharedString::from(format!("nav-{title}")))
                    .accessibility_label(title)
                    .selected(active)
                    .ghost()
                    .w_full()
                    .h(px(36.))
                    .rounded(px(7.))
                    .px(px(12.))
                    .bg(rgba(0x00000000))
                    .text_color(rgb(color))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .w_full()
                            .child(icon(symbol, 18.).text_color(rgb(color)))
                            .child(label(title, 13., color).font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            }))
                            .child(div().flex_1())
                            .when_some(count, |this, count| {
                                this.child(mono(count.to_string(), 11., color))
                            }),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.navigate(page, window, cx)),
                    ),
            );
        }
        div()
            .w(px(240.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SIDEBAR))
            .flex()
            .flex_col()
            .child(nav)
            .child(div().flex_1())
            .child(
                div()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .p(px(10.))
                    .child(
                        Button::new("storage-summary")
                            .accessibility_label("管理存储源")
                            .tooltip("管理存储源")
                            .ghost()
                            .w_full()
                            .h(px(54.))
                            .px(px(10.))
                            .rounded(px(7.))
                            .child(icon("folder-open", 18.).text_color(rgb(NAV_TEXT)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(label("当前存储源", 10., MUTED))
                                    .child(
                                        label(
                                            if self.provider.is_empty() {
                                                "尚未配置".into()
                                            } else {
                                                self.provider.clone()
                                            },
                                            12.,
                                            TEXT,
                                        )
                                        .text_ellipsis(),
                                    ),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.navigate(Page::Settings, window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
    fn queue_filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut tabs = div()
            .h(px(34.))
            .p(px(3.))
            .gap(px(2.))
            .flex()
            .items_center()
            .border_1()
            .border_color(rgb(BORDER))
            .rounded(px(12.));
        for (filter, title) in [
            (Filter::All, "全部"),
            (Filter::Running, "上传中"),
            (Filter::Done, "已完成"),
            (Filter::Failed, "失败"),
        ] {
            let active = self.filter == filter;
            tabs = tabs.child(
                Button::new(SharedString::from(format!("filter-{title}")))
                    .selected(active)
                    .ghost()
                    .small()
                    .h(px(26.))
                    .px(px(10.))
                    .text_size(px(12.))
                    .rounded(px(8.))
                    .bg(rgb(if active { TEXT } else { CANVAS }))
                    .text_color(rgb(if active { CARD } else { MUTED }))
                    .label(title)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.filter != filter {
                            this.content_revision = this.content_revision.wrapping_add(1);
                        }
                        this.filter = filter;
                        cx.notify();
                    }))
                    .with_spring(
                        SharedString::from(format!("filter-transition-{title}")),
                        SpringAnimation::new(SpringConfig::new(700., 54., 1.))
                            .to(AnimationPhase(if active { 1. } else { 0. })),
                        |this, phase| {
                            this.bg(phase.interpolate_between_clamped(
                                0.0..=1.0,
                                rgb(CANVAS),
                                rgb(TEXT),
                            ))
                            .text_color(
                                phase.interpolate_between_clamped(0.0..=1.0, rgb(MUTED), rgb(CARD)),
                            )
                        },
                    ),
            );
        }
        tabs.into_any_element()
    }
    fn toolbar(&self, sidebar_width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let providers = self.providers.clone();
        let current = self.provider.clone();
        let reference = self.reference;
        let provider = action("provider", "")
            .h(px(28.))
            .rounded(px(6.))
            .accessibility_label("选择存储源")
            .px(px(12.))
            .border_color(rgb(BORDER))
            .bg(rgb(CARD))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(dot(ORANGE))
                    .child(label(
                        if self.provider.is_empty() {
                            "选择存储源".into()
                        } else {
                            self.provider.clone()
                        },
                        12.,
                        TEXT,
                    ))
                    .child(icon("caret-down", 10.).text_color(rgb(MUTED))),
            )
            .dropdown_menu(move |mut menu, _, _| {
                menu = menu.min_w(px(200.));
                if reference {
                    menu = menu.item(PopupMenuItem::label("参考图中的示例存储"));
                    for name in ["SM.MS", "Imgur"] {
                        let weak = weak.clone();
                        menu =
                            menu.item(PopupMenuItem::new(name).checked(name == current).on_click(
                                move |_, _, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.provider = name.into();
                                        cx.notify();
                                    });
                                },
                            ));
                    }
                }
                if !providers.is_empty() {
                    menu = menu.item(PopupMenuItem::label("已配置的存储源"));
                }
                for (name, _) in &providers {
                    let selected = name == &current;
                    let name = name.clone();
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(selected).on_click(
                        move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.provider = name.clone();
                                cx.notify();
                            });
                        },
                    ));
                }
                let weak = weak.clone();
                menu.separator()
                    .item(
                        PopupMenuItem::new("管理存储源…").on_click(move |_, window, cx| {
                            let _ = weak
                                .update(cx, |this, cx| this.navigate(Page::Settings, window, cx));
                        }),
                    )
            });
        let collapsed = self.preferences.sidebar_collapsed;
        let toggle = if collapsed {
            "展开侧边栏"
        } else {
            "收起侧边栏"
        };
        let (title, symbol) = match self.page {
            Page::Queue => ("上传队列", "upload-simple"),
            Page::Library => ("图库", "image"),
            Page::History => ("历史记录", "clock-counter-clockwise"),
            Page::Settings => ("设置", "gear"),
        };
        let chrome_button = |id, label: &str, symbol| {
            Button::new(id)
                .accessibility_label(label.to_owned())
                .tooltip(label.to_owned())
                .ghost()
                .small()
                .size(px(28.))
                .p_0()
                .rounded(px(5.))
                .child(icon(symbol, 17.).text_color(rgb(NAV_TEXT)))
        };
        TitleBar::new()
            .h(px(36.))
            .p_0()
            .bg(rgb(CANVAS))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h_full()
                    .w(px(190.) + sidebar_width * (50. / 240.))
                    .flex_shrink_0()
                    .pl(px(88.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .bg(rgb(if collapsed { CANVAS } else { SIDEBAR }))
                    .when(!collapsed, |this| {
                        this.border_r_1().border_color(rgb(BORDER))
                    })
                    .child(
                        chrome_button("toggle-sidebar", toggle, "panel-left").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.toggle_sidebar(&ToggleSidebar, window, cx)
                            }),
                        ),
                    )
                    .child(
                        chrome_button("navigate-back", "后退", "arrow-left")
                            .disabled(self.back_stack.is_empty())
                            .when(self.back_stack.is_empty(), |this| this.opacity(0.35))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(page) = this.back_stack.pop() {
                                    this.forward_stack.push(this.page);
                                    this.show_page(page, window, cx);
                                }
                            })),
                    )
                    .child(
                        chrome_button("navigate-forward", "前进", "arrow-right")
                            .disabled(self.forward_stack.is_empty())
                            .when(self.forward_stack.is_empty(), |this| this.opacity(0.35))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(page) = this.forward_stack.pop() {
                                    this.back_stack.push(this.page);
                                    this.show_page(page, window, cx);
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .px(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon(symbol, 18.).text_color(rgb(NAV_TEXT)))
                    .child(
                        label(title, 13., TEXT)
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_nowrap(),
                    )
                    .child(div().flex_1().min_w(px(12.)))
                    .when(self.page != Page::Settings, |this| {
                        this.child(
                            div().w(px(220.)).min_w(px(140.)).flex_shrink(1.).child(
                                Input::new(&self.search)
                                    .aria_label("搜索图片")
                                    .h(px(30.))
                                    .text_size(px(12.))
                                    .bg(rgb(CANVAS))
                                    .border_color(rgb(BORDER))
                                    .rounded(px(7.))
                                    .cleanable(true),
                            ),
                        )
                    })
                    .child(provider),
            )
            .into_any_element()
    }
    fn format_picker(&self, scope: &str, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.preferences.copy_format;
        let weak = cx.entity().downgrade();
        action(
            SharedString::from(format!("{scope}-copy-format")),
            selected.label(),
        )
        .accessibility_label("选择链接格式")
        .outline()
        .px(px(10.))
        .child(icon("caret-down", 10.).text_color(rgb(NAV_TEXT)))
        .dropdown_menu(move |mut menu, _, _| {
            menu = menu.min_w(px(190.)).item(PopupMenuItem::label("链接格式"));
            for format in CopyFormat::ALL {
                let weak = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(format.label())
                        .checked(format == selected)
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.preferences.copy_format = format;
                                this.save_preferences(cx);
                            });
                        }),
                );
            }
            menu
        })
        .into_any_element()
    }
    fn upload_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let ready = self
            .queue
            .items
            .iter()
            .filter(|i| !i.simulated && i.status == Status::Ready)
            .count();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mt(px(14.))
            .flex_wrap()
            .child(
                action("choose-files", "选择文件")
                    .outline()
                    .px(px(10.))
                    .disabled(self.preparing)
                    .icon(Icon::default().path("icons/folder-open.svg"))
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.pick_files(&ChooseFiles, window, cx)
                        }),
                    ),
            )
            .child(
                action("paste-image", "粘贴")
                    .outline()
                    .px(px(10.))
                    .disabled(self.preparing)
                    .icon(Icon::default().path("icons/clipboard.svg"))
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.paste_image(&PasteImage, window, cx)
                        }),
                    ),
            )
            .child(
                action("capture-image", "截图")
                    .outline()
                    .px(px(10.))
                    .disabled(self.preparing)
                    .icon(Icon::default().path("icons/scissors.svg"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.capture(&CaptureImage, window, cx)),
                    ),
            )
            .child(
                action("add-url", "链接")
                    .outline()
                    .px(px(10.))
                    .disabled(self.preparing)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_url_input = !this.show_url_input;
                        if this.show_url_input {
                            this.url_input
                                .update(cx, |input, cx| input.focus(window, cx));
                        } else {
                            this.focus.focus(window, cx);
                        }
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(self.format_picker("quick", cx))
            .child(
                action(
                    "upload-pending",
                    &if self.queue.batch.as_ref().is_some_and(|b| b.paused) {
                        "已暂停".into()
                    } else if self.queue.batch.is_some() {
                        "上传中…".into()
                    } else if ready == 0 {
                        "上传".into()
                    } else {
                        format!("上传 {ready} 张")
                    },
                )
                .primary()
                .icon(Icon::default().path("icons/upload-simple.svg"))
                .disabled(self.preparing || self.queue.batch.is_some() || ready == 0)
                .on_click(cx.listener(|this, _, _, cx| this.start_all_uploads(cx))),
            )
            .into_any_element()
    }
    fn drop_zone(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .w_full()
            .flex_shrink_0()
            .child(
                div()
                    .id("drop-zone")
                    .h(px(if window.viewport_size().height < px(760.) {
                        164.
                    } else {
                        248.
                    }))
                    .w_full()
                    .flex_shrink_0()
                    .rounded(px(16.))
                    .relative()
                    .bg(rgb(DROP))
                    .child(dashed_outline())
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .drag_over::<ExternalPaths>(|style, _, _, _| {
                        style.bg(rgb(ACTIVE_CARD)).border_color(rgb(ORANGE))
                    })
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        this.import_paths(paths.0.to_vec(), cx)
                    }))
                    .child(
                        div()
                            .size(px(56.))
                            .rounded(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(ACTIVE_CARD))
                            .child(icon("upload-simple", 24.).text_color(rgb(NAV_ACTIVE))),
                    )
                    .child(
                        label(
                            if self.preparing {
                                "正在读取图片…"
                            } else {
                                "拖拽图片到这里，或按 ⌘V 粘贴"
                            },
                            16.,
                            TEXT,
                        )
                        .font_weight(FontWeight::SEMIBOLD)
                        .mt(px(16.)),
                    )
                    .child(
                        label(
                            format!(
                                "PNG / JPEG / GIF / WebP / SVG / AVIF · 每张 ≤ {} MB",
                                self.upload_options.max_size_mb
                            ),
                            12.,
                            MUTED,
                        )
                        .mt(px(9.)),
                    )
                    .child(
                        label(
                            if self.preferences.auto_copy {
                                "上传完成后，自动复制所选格式的链接"
                            } else {
                                "上传完成后，可在队列中复制链接"
                            },
                            12.,
                            NAV_ACTIVE,
                        )
                        .mt(px(18.)),
                    ),
            )
            .child(self.upload_actions(cx))
            .when(self.show_url_input, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mt(px(12.))
                        .child(
                            div().flex_1().child(
                                Input::new(&self.url_input)
                                    .aria_label("图片链接")
                                    .h(px(36.)),
                            ),
                        )
                        .child(
                            action("import-url", "导入链接")
                                .outline()
                                .disabled(self.preparing)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let text = this.url_input.read(cx).value().to_string();
                                    this.import_urls(&text, cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }
    pub fn startup_update_check(&mut self, cx: &mut Context<Self>) {
        if let Some((message, error)) = crate::installer::installation_notice(&self.root) {
            self.message(message, error, cx);
        }
        if !self.reference && self.preferences.check_updates && crate::updates::due(&self.root) {
            self.check_updates(cx);
        }
    }
    fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.update_checking || self.update_downloading {
            return;
        }
        self.update_checking = true;
        self.update_notice = None;
        let task = cx
            .background_executor()
            .spawn(async { crate::updates::check() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.update_checking = false;
                match result {
                    Ok(update) => {
                        crate::updates::save_check_time(&this.root);
                        this.update_notice = Some((
                            update
                                .as_ref()
                                .map(|u| format!("发现新版本 {}", u.version))
                                .unwrap_or("未发现可安装的新版本".into()),
                            false,
                        ));
                        this.available_update = update;
                    }
                    Err(e) => this.update_notice = Some((e.to_string(), true)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn download_update(&mut self, cx: &mut Context<Self>) {
        if self.update_downloading {
            return;
        }
        let Some(update) = self.available_update.clone() else {
            return;
        };
        if update.size == 0 {
            cx.open_url(&update.page);
            return;
        }
        let root = self.root.clone();
        self.update_file = None;
        self.update_downloading = true;
        self.update_notice = Some(("正在下载安装包并校验完整性…".into(), false));
        let version = update.version.clone();
        let task = cx
            .background_executor()
            .spawn(async move { crate::updates::download(&update, &root) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.update_downloading = false;
                match result {
                    Ok(file) => {
                        this.update_file = Some((file, version));
                        this.update_notice = Some((
                            "安装包已校验，可退出并安装更新。原有配置、图库和终端命令会保留。"
                                .into(),
                            false,
                        ));
                    }
                    Err(e) => this.update_notice = Some((e.to_string(), true)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn install_application(&mut self, update: Option<(PathBuf, String)>, cx: &mut Context<Self>) {
        if self.install_preparing || self.shutting_down {
            return;
        }
        if self.queue.batch.is_some()
            || !self.queue.submitted.is_empty()
            || !self.queue.quick.is_empty()
            || self.preparing
        {
            self.update_notice = Some(("请先完成或取消上传和图片导入，再安装应用".into(), true));
            cx.notify();
            return;
        }
        self.install_preparing = true;
        self.update_notice = Some((
            "正在验证并准备安装，完成后会保存数据、退出并重新打开 img…".into(),
            false,
        ));
        let root = self.root.clone();
        let task = cx.background_executor().spawn(async move {
            match update {
                Some((file, version)) => crate::installer::prepare_update(&file, &version, &root),
                None => crate::installer::prepare_current(&root),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.install_preparing = false;
                match result {
                    Ok(_)
                        if this.queue.batch.is_some()
                            || !this.queue.submitted.is_empty()
                            || !this.queue.quick.is_empty()
                            || this.preparing =>
                    {
                        this.update_notice =
                            Some(("安装准备期间有新上传任务，请完成后重试".into(), true));
                    }
                    Ok(prepared) => {
                        this.pending_install = Some(prepared);
                        this.begin_shutdown(cx);
                    }
                    Err(error) => this.update_notice = Some((error.to_string(), true)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn install_cli(&mut self, cx: &mut Context<Self>) {
        let binary = self.engine.clone();
        let task = cx.background_executor().spawn(async move {
            std::process::Command::new(binary)
                .arg("install-cli")
                .output()
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.update_notice = Some(match result {
                    Ok(output) if output.status.success() => (
                        String::from_utf8_lossy(&output.stdout).trim().to_owned(), false),
                    Ok(_) => ("命令入口已存在或目录不可写。可直接使用应用内的 Contents/MacOS/img，或选择其他 CLI 安装目录。".into(), true),
                    Err(_) => ("无法启动内置 CLI，请重新安装应用。".into(), true),
                });
                cx.notify();
            });
        }).detach();
    }
    fn update_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(label("关于与更新", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(label(
                format!(
                    "img {} · {}",
                    crate::updates::CURRENT_VERSION,
                    crate::platform::os()
                ),
                13.,
                TEXT,
            ))
            .child(label(
                if option_env!("IMG_SIGNING_TEAM").is_some() {
                    "已签名发行版"
                } else if cfg!(target_os = "macos") {
                    "社区安装包 · 未经 Apple 公证，首次打开请按安装说明允许运行"
                } else {
                    "社区安装包 · 请从官方 Releases 下载，按系统安装器提示安装"
                },
                12.,
                MUTED,
            ))
            .when(!crate::installer::installed(), |body| {
                body.child(
                    Button::new("install-application")
                        .label("安装到应用程序并重新打开")
                        .primary()
                        .small()
                        .disabled(self.install_preparing || self.shutting_down)
                        .on_click(cx.listener(|this, _, _, cx| this.install_application(None, cx))),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("已内置 Rust CLI · 可独立在终端使用", 13., TEXT))
                    .child(
                        Button::new("install-cli")
                            .label("添加终端命令")
                            .outline()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| this.install_cli(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("每天自动检查新版本", 13., TEXT))
                    .child(
                        Switch::new("check-updates-auto")
                            .accessibility_label("每天自动检查新版本")
                            .checked(self.preferences.check_updates)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.preferences.check_updates = *checked;
                                this.save_preferences(cx);
                                if *checked {
                                    this.startup_update_check(cx);
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("check-updates")
                            .label(if self.update_checking {
                                "正在检查…"
                            } else {
                                "检查更新"
                            })
                            .outline()
                            .small()
                            .disabled(self.update_checking || self.update_downloading)
                            .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx))),
                    )
                    .child(
                        Button::new("all-releases")
                            .label("版本页面")
                            .ghost()
                            .small()
                            .on_click(|_, _, cx| cx.open_url(crate::updates::RELEASES_PAGE)),
                    )
                    .when_some(self.available_update.clone(), |this, update| {
                        this.child(
                            Button::new("release-notes")
                                .label("版本说明")
                                .ghost()
                                .small()
                                .on_click(move |_, _, cx| cx.open_url(&update.page)),
                        )
                        .child(
                            Button::new("download-update")
                                .label(if self.update_downloading {
                                    "下载中…"
                                } else {
                                    "获取更新"
                                })
                                .primary()
                                .small()
                                .disabled(self.update_downloading)
                                .on_click(cx.listener(|this, _, _, cx| this.download_update(cx))),
                        )
                    })
                    .when_some(self.update_file.clone(), |this, (file, version)| {
                        let manual = file.clone();
                        this.child(
                            Button::new("install-update")
                                .label(if self.install_preparing {
                                    "正在准备…"
                                } else {
                                    crate::installer::install_label()
                                })
                                .primary()
                                .small()
                                .disabled(
                                    self.install_preparing
                                        || self.shutting_down
                                        || self.queue.batch.is_some(),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.install_application(
                                        Some((file.clone(), version.clone())),
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::new("open-update")
                                .label("手动安装")
                                .ghost()
                                .small()
                                .disabled(self.queue.batch.is_some())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if crate::platform::open_path(&manual).is_err() {
                                        this.message("无法打开安装包", true, cx);
                                    }
                                })),
                        )
                    }),
            );
        if let Some((message, error)) = &self.update_notice {
            body = body.child(label(
                message.clone(),
                12.,
                if *error { RED } else { NAV_TEXT },
            ));
        }
        body.into_any_element()
    }
    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let card = || {
            div()
                .p(px(22.))
                .rounded(px(16.))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(CARD))
                .flex()
                .flex_col()
                .gap(px(18.))
        };
        let row = |title, description| {
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(label(title, 13., TEXT))
                .child(label(description, 12., MUTED))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(label("设置", 20., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(card().child(self.recovery_controls(cx)))
            .child(card().child(self.storage_settings.clone()))
            .child(card().child(self.upload_settings.clone()))
            .child(card().child(self.shortcut_settings.clone()))
            .child(
                card()
                    .child(label("链接与剪贴板", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(20.))
                            .child(row(
                                "上传后自动复制",
                                "批量上传时，一次复制本批所有成功图片的链接。",
                            ))
                            .child(
                                Switch::new("auto-copy")
                                    .accessibility_label("上传后自动复制")
                                    .checked(self.preferences.auto_copy)
                                    .color(rgb(NAV_ACTIVE))
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.preferences.auto_copy = *checked;
                                        this.save_preferences(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .pt(px(16.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(row("链接格式", "快捷上传、图库和历史记录共用此格式。"))
                            .child(self.format_picker("settings", cx)),
                    ),
            )
            .child(
                card()
                    .child(label("应用偏好", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(row("收起侧边栏", "隐藏侧栏以腾出空间，左上角可随时展开。"))
                            .child(
                                Switch::new("sidebar-preference")
                                    .accessibility_label("收起侧边栏")
                                    .checked(self.preferences.sidebar_collapsed)
                                    .color(rgb(NAV_ACTIVE))
                                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                        this.preferences.sidebar_collapsed = *checked;
                                        this.save_preferences(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .pt(px(16.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(row("图库展示", "选择网格或列表，自动记住你的选择。"))
                            .child(self.library_view_switch(cx)),
                    ),
            )
            .child(card().child(self.update_controls(cx)))
            .child(label(
                "⌘U 选择文件     ⌘V 粘贴     ⌘⇧S 截图     ⌘B 收起侧栏     ⌘F 搜索",
                11.,
                MUTED,
            ))
            .into_any_element()
    }
    fn queue_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let paused = self.queue.batch.as_ref().is_some_and(|b| b.paused);
        let has_paused = self
            .queue
            .items
            .iter()
            .any(|i| !i.simulated && i.status == Status::Paused);
        div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.))
            .mb(px(12.))
            .child(
                Button::new("pause-all")
                    .label(if paused || self.queue.batch.is_none() {
                        "继续队列"
                    } else {
                        "暂停全部"
                    })
                    .outline()
                    .small()
                    .disabled(self.queue.batch.is_none() && !has_paused)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if paused || this.queue.batch.is_none() {
                            this.resume_all(cx);
                        } else {
                            this.pause_all(cx);
                        }
                    })),
            )
            .child(
                Button::new("cancel-all")
                    .label("取消本批")
                    .ghost()
                    .small()
                    .disabled(self.queue.batch.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_all(cx))),
            )
            .child(
                Button::new("retry-failed")
                    .label("重试失败项")
                    .ghost()
                    .small()
                    .disabled(
                        self.queue.batch.is_some()
                            || !self
                                .queue
                                .items
                                .iter()
                                .any(|i| !i.simulated && i.status == Status::Failed),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.retry_failed(cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("clear-finished")
                    .label("清理已结束记录")
                    .ghost()
                    .small()
                    .disabled(!self.queue.items.iter().any(|i| {
                        matches!(i.status, Status::Done | Status::Failed | Status::Cancelled)
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let ids = this
                            .queue
                            .items
                            .iter()
                            .filter(|i| {
                                matches!(
                                    i.status,
                                    Status::Done | Status::Failed | Status::Cancelled
                                )
                            })
                            .map(|i| i.id.clone())
                            .collect();
                        this.remove_records(ids, window, cx);
                    })),
            )
            .into_any_element()
    }
    fn content(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.filtered(cx);
        let mut body = div()
            .flex()
            .flex_col()
            .w_full()
            .min_h(px(0.))
            .p(px(24.))
            .when(self.page != Page::Settings, |body| body.h_full());
        if self.page == Page::Settings {
            body = body.child(self.settings(cx));
        } else {
            if self.page == Page::Queue {
                body = body
                    .child(
                        div()
                            .h(px(24.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(16.))
                            .mb(px(16.))
                            .child(label("上传图片", 20., TEXT).font_weight(FontWeight::SEMIBOLD))
                            .child(label("支持拖拽 · 剪贴板 · 截图 · 文件选择", 12., MUTED)),
                    )
                    .child(self.drop_zone(window, cx));
            }
            let heading = match self.page {
                Page::Library => "图库",
                Page::History => "历史记录",
                _ => "上传队列",
            };
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.))
                    .flex_shrink_0()
                    .mb(px(11.))
                    .when(self.page == Page::Queue, |this| this.mt(px(24.)))
                    .child(label(heading, 16., TEXT).font_weight(FontWeight::SEMIBOLD))
                    .child(mono(format!("{} 项", rows.len()), 11., MUTED).ml(px(16.)))
                    .child(div().flex_1())
                    .when(self.page == Page::Queue, |this| {
                        this.child(self.queue_filters(cx))
                    })
                    .when(self.page == Page::Library, |this| {
                        this.child(self.library_view_switch(cx)).child(
                            Button::new("library-select")
                                .ghost()
                                .small()
                                .ml(px(8.))
                                .label(if self.selection.active {
                                    "完成"
                                } else {
                                    "选择"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.selection.active {
                                        this.selection.finish();
                                    } else {
                                        this.selection.active = true;
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            );
            if self.page == Page::Library && self.selection.active {
                body = body.child(self.library_selection_controls(rows.clone(), cx));
            }
            if self.page != Page::Library {
                body = body.child(self.queue_controls(cx));
            }
            if rows.is_empty() {
                body = body.child(
                    div()
                        .h(px(112.))
                        .rounded(px(16.))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .child(label(
                            if self.page == Page::Library && self.search.read(cx).value().is_empty()
                            {
                                "还没有已上传的图片"
                            } else if self.queue.items.is_empty() {
                                "还没有上传记录"
                            } else {
                                "没有符合条件的图片"
                            },
                            14.,
                            TEXT,
                        ))
                        .child(label(
                            if self.page == Page::Library && self.search.read(cx).value().is_empty()
                            {
                                "上传完成后，图片会自动出现在图库中"
                            } else if self.queue.items.is_empty() {
                                "选择图片或拖放到上方，开始第一次上传"
                            } else if self.page != Page::Queue {
                                "试试其他关键词，或清空搜索"
                            } else {
                                "试试其他关键词或切换筛选条件"
                            },
                            12.,
                            MUTED,
                        )),
                );
            } else if self.page == Page::Library {
                body = body.child(self.library(rows, window, cx));
            } else {
                body = body.child(self.virtual_records(rows, false, window, cx));
            }
        }
        div()
            .id("content-scroll")
            .flex_1()
            .min_h(px(0.))
            .when(self.page == Page::Settings, |body| body.overflow_y_scroll())
            .child(
                body.with_animation(
                    ("content-enter", self.content_revision),
                    Animation::new(Duration::from_millis(if self.content_revision == 0 {
                        0
                    } else {
                        160
                    }))
                    .with_easing(ease_out_quint()),
                    |this, phase| {
                        this.opacity(0.3 + 0.7 * phase)
                            .relative()
                            .top(px(5. * (1. - phase)))
                    },
                ),
            )
            .into_any_element()
    }
    fn footer(&self) -> AnyElement {
        let active = self
            .queue
            .items
            .iter()
            .find(|i| i.status == Status::Running);
        let mut bar = div()
            .h(px(42.))
            .flex_shrink_0()
            .px(px(24.))
            .border_t_1()
            .border_color(rgb(BORDER))
            .flex()
            .items_center()
            .gap(px(12.));
        if let Some(item) = active {
            let value = item.progress;
            let bytes = if let Some(active) = self.queue.active.get(&item.id) {
                let meter = active.control.progress();
                if meter.total > 0 {
                    format!(
                        "{} / {} · {}",
                        model::size_label(meter.sent),
                        model::size_label(meter.total as u64),
                        meter.label()
                    )
                } else {
                    meter.label()
                }
            } else {
                item.size_label()
            };
            bar = bar
                .child(dot(ORANGE))
                .child(label(item.name.clone(), 12., TEXT).text_ellipsis())
                .child(mono(bytes, 10., MUTED).whitespace_nowrap())
                .child(
                    div()
                        .flex_1()
                        .child(progress(self.display_progress(item), ORANGE_BRIGHT)),
                )
                .child(mono(
                    value.map(|p| format!("{p}%")).unwrap_or("上传中".into()),
                    10.,
                    ORANGE,
                ));
        } else {
            bar = bar
                .child(dot(if self.queue.items.is_empty() {
                    MUTED
                } else {
                    GREEN
                }))
                .child(label(
                    if self.preparing {
                        "正在读取图片…".into()
                    } else {
                        format!(
                            "{} 张图片 · {} 项已完成",
                            self.queue.items.len(),
                            self.queue
                                .items
                                .iter()
                                .filter(|i| i.status == Status::Done)
                                .count()
                        )
                    },
                    12.,
                    MUTED,
                ));
        }
        bar.into_any_element()
    }
    fn open_preview(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let width = (f32::from(window.viewport_size().width) - 96.).min(960.);
            let height = (f32::from(window.viewport_size().height) - 240.).clamp(240., 660.);
            let copy_actions = if item.url.is_some() {
                weak.update(cx, |this, cx| this.copy_actions(&item, "preview", cx))
                    .ok()
            } else {
                None
            };
            dialog
                .w(px(width))
                .p(px(20.))
                .rounded(px(18.))
                .bg(rgb(CANVAS))
                .close_button(false)
                .title(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(label(item.name.clone(), 14., TEXT))
                        .child(mono(item.size_label(), 11., MUTED)),
                )
                .child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(height))
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(
                            if let Some(path) = &item.thumbnail {
                                img(path.clone())
                            } else {
                                img(item.asset.clone())
                            }
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                        ),
                )
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .children(copy_actions)
                        .child(
                            action("preview-close", "关闭预览")
                                .primary()
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        ),
                )
        });
    }
}
impl Render for ImgDesktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        if self.page != Page::Library {
            self.selection.finish();
        }
        self.selection
            .reconcile(&self.queue.items, self.records_revision);
        // A shared spring keeps titlebar and content geometry in step. Retargeting
        // preserves velocity, so rapid clicks reverse naturally without snapping.
        let width_target = px(if self.preferences.sidebar_collapsed {
            0.
        } else {
            240.
        });
        let sidebar_width = gpui_kit::base::spring(
            "sidebar-width",
            width_target,
            gpui_kit::base::Spring::new(Duration::from_millis(300)).with_epsilon(0.2),
            window,
            cx,
        );
        let nav_target = px(12.
            + 40.
                * match self.page {
                    Page::Library => 0.,
                    Page::Queue => 1.,
                    Page::History => 2.,
                    Page::Settings => 3.,
                });
        let active_y = gpui_kit::base::spring(
            "sidebar-selection",
            nav_target,
            gpui_kit::base::Spring::new(Duration::from_millis(230)).with_epsilon(0.2),
            window,
            cx,
        );
        if self.trace_motion && sidebar_width != width_target {
            eprintln!(
                "sidebar-motion width={:.2} target={:.0}",
                f32::from(sidebar_width),
                f32::from(width_target)
            );
        }
        let mut main = div().flex_1().min_w(px(0.)).h_full().flex().flex_col();
        if let Some((text, error)) = &self.notice {
            main = main.child(
                div()
                    .px(px(24.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .bg(rgb(ORANGE_SOFT))
                    .child(label(text.clone(), 12., if *error { RED } else { MUTED }).flex_1())
                    .child(
                        Button::new("dismiss-notice")
                            .accessibility_label("关闭提示")
                            .ghost()
                            .xsmall()
                            .icon(Icon::default().path("icons/x.svg"))
                            .tooltip("关闭提示")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notice = None;
                                cx.notify();
                            })),
                    ),
            );
        }
        main = main.child(self.content(window, cx)).child(self.footer());
        div()
            .id("aperture")
            .key_context("ImgDesktop")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(rgb(CANVAS))
            .text_color(rgb(TEXT))
            .font_family("Inter Variable")
            .text_size(px(13.))
            .line_height(relative(1.3))
            .flex()
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.request_close(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseWindow, window, cx| {
                    this.hide_to_background(window, cx)
                }),
            )
            .on_action(cx.listener(Self::pick_files))
            .on_action(cx.listener(Self::paste_image))
            .on_action(cx.listener(Self::capture))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(|this, _: &Search, window, cx| {
                this.search
                    .update(cx, |search, cx| search.focus(window, cx))
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                window.close_dialog(cx);
                this.notice = None;
                cx.notify();
            }))
            .flex_col()
            .child(self.toolbar(sidebar_width, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .when(sidebar_width > px(0.), |this| {
                        this.child(
                            div()
                                .w(sidebar_width)
                                .h_full()
                                .flex_shrink_0()
                                .overflow_hidden()
                                .child(self.sidebar(active_y, cx)),
                        )
                    })
                    .child(main),
            )
            .into_any_element()
    }
}
