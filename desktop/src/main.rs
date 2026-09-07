#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod assets;
mod desktop_runtime;
mod diagnostics;
mod engine;
mod inbox;
mod installer;
mod model;
mod native;
mod platform;
mod preferences;
mod queue_store;
mod record_index;
mod selection;
mod settings;
mod shortcut_settings;
mod storage;
mod theme;
mod thumbnails;
mod ui;
mod updates;
mod upload_options;
mod upload_settings;

use gpui_kit::{
    component::{Root, TitleBar},
    *,
};
use ui::{
    CaptureImage, ChooseFiles, CloseWindow, DesktopShell, Dismiss, ImgDesktop, OpenWindow,
    PasteImage, QuickCapture, QuickPaste, Quit, Search, ToggleQueue, ToggleSidebar,
};

fn main() -> anyhow::Result<()> {
    let reference = std::env::args().any(|a| a == "--reference");
    let compact = std::env::args().any(|a| a == "--compact");
    let tall = std::env::args().any(|a| a == "--reference-size");
    let root = if let Some(path) = std::env::var_os("APERTURE_DATA_DIR") {
        std::path::PathBuf::from(path)
    } else if reference {
        model::data_dir()?.join("reference-preview")
    } else {
        model::data_dir()?
    };
    std::fs::create_dir_all(&root)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("session.lock"))?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("img 已在运行，请使用已打开的窗口"))?;
    let engine = std::env::var_os("APERTURE_ENGINE")
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_exe()?.with_file_name(if cfg!(windows) {
            "img.exe"
        } else {
            "img"
        }));
    let application = gpui_kit::application().with_assets(assets::Assets);
    application.on_reopen(|cx| desktop_runtime::DesktopRuntime::open(cx, None));
    application.run(move |cx| {
        gpui_kit::init(cx);
        cx.text_system()
            .add_fonts(vec![
                assets::bytes("fonts/InterVariable.ttf").expect("bundled Inter font"),
            ])
            .expect("load Inter");
        theme::init(cx);
        cx.bind_keys([
            KeyBinding::new("secondary-b", ToggleSidebar, Some("ImgDesktop")),
            KeyBinding::new("secondary-u", ChooseFiles, Some("ImgDesktop")),
            KeyBinding::new("secondary-v", PasteImage, Some("ImgDesktop")),
            KeyBinding::new("secondary-f", Search, Some("ImgDesktop")),
            KeyBinding::new("secondary-shift-s", CaptureImage, Some("ImgDesktop")),
            KeyBinding::new("escape", Dismiss, Some("ImgDesktop")),
            KeyBinding::new("secondary-q", Quit, None),
            KeyBinding::new("secondary-w", CloseWindow, None),
        ]);
        cx.set_app_identity(
            &std::env::var("APERTURE_APP_ID").unwrap_or_else(|_| "dev.img.desktop".into()),
            "img",
        );
        cx.on_action(|_: &OpenWindow, cx| {
            desktop_runtime::DesktopRuntime::dispatch(desktop_runtime::DesktopEvent::Open, cx)
        });
        cx.on_action(|_: &QuickPaste, cx| {
            desktop_runtime::DesktopRuntime::dispatch(desktop_runtime::DesktopEvent::Paste, cx)
        });
        cx.on_action(|_: &QuickCapture, cx| {
            desktop_runtime::DesktopRuntime::dispatch(desktop_runtime::DesktopEvent::Capture, cx)
        });
        cx.on_action(|_: &ToggleQueue, cx| {
            desktop_runtime::DesktopRuntime::dispatch(
                desktop_runtime::DesktopEvent::TogglePause,
                cx,
            )
        });
        cx.set_menus(vec![Menu {
            name: "img".into(),
            disabled: false,
            items: vec![
                MenuItem::action("打开窗口", OpenWindow),
                MenuItem::action("立即上传剪贴板", QuickPaste),
                MenuItem::action("截图并上传…", QuickCapture),
                MenuItem::action("暂停／继续上传", ToggleQueue),
                MenuItem::separator(),
                MenuItem::action("选择图片…", ChooseFiles),
                MenuItem::action("从剪贴板粘贴", PasteImage),
                MenuItem::action("截取屏幕区域…", CaptureImage),
                MenuItem::separator(),
                MenuItem::action("关闭窗口", CloseWindow),
                MenuItem::action("退出 img", Quit),
            ],
        }]);
        let size = size(
            px(if compact { 960. } else { 1280. }),
            px(if compact {
                700.
            } else if tall {
                1800.
            } else {
                900.
            }),
        );
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(11.))),
                }),
                window_min_size: Some(gpui_kit::size(px(960.), px(700.))),
                ..TitleBar::window_options()
            },
            move |window, cx| {
                window.set_window_title(if reference {
                    "img · 参考图预览"
                } else {
                    "img"
                });
                let view = cx
                    .new(|cx| ImgDesktop::new(root.clone(), engine.clone(), reference, window, cx));
                view.update(cx, |view, cx| view.startup_update_check(cx));
                if std::env::args().any(|arg| arg == "--smoke-test") {
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(3))
                            .await;
                        cx.update(|cx| cx.quit());
                    })
                    .detach();
                }
                #[cfg(feature = "perf")]
                view.update(cx, |view, cx| view.start_benchmark(window, cx));
                cx.on_action(|_: &Quit, cx| {
                    desktop_runtime::DesktopRuntime::dispatch(
                        desktop_runtime::DesktopEvent::Quit,
                        cx,
                    )
                });
                cx.on_action(|_: &CloseWindow, cx| {
                    desktop_runtime::DesktopRuntime::dispatch(
                        desktop_runtime::DesktopEvent::Hide,
                        cx,
                    )
                });
                let close_view = view.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    let _ = close_view.update(cx, |view, cx| view.hide_to_background(window, cx));
                    false
                });
                let session_view = view.clone();
                let session_window = window.window_handle();
                let session_root = root.clone();
                // AppKit can process nested events while creating a status item. Wait until
                // GPUI has registered the window and its root before installing native objects.
                cx.defer(move |cx| {
                    desktop_runtime::DesktopRuntime::install(
                        session_view,
                        session_window,
                        session_root,
                        cx,
                    );
                    desktop_runtime::DesktopRuntime::open(cx, None);
                });
                let shell = cx.new(|_| DesktopShell(view));
                cx.new(|cx| Root::new(shell, window, cx))
            },
        )
        .expect("open img window");
        cx.activate(true);
        // Keep the advisory lock for the entire native event loop.
        cx.on_app_quit(move |_| {
            let _ = &lock;
            async {}
        })
        .detach();
    });
    Ok(())
}
