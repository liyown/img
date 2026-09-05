mod assets;
mod engine;
mod model;
mod preferences;
mod settings;
mod storage;
mod theme;
mod ui;
mod updates;
mod upload_options;
mod upload_settings;

use gpui_kit::{
    component::{Root, TitleBar},
    *,
};
use ui::{
    CaptureImage, ChooseFiles, CloseWindow, DesktopShell, Dismiss, ImgDesktop, PasteImage, Quit,
    Search, ToggleSidebar,
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
        .unwrap_or(std::env::current_exe()?.with_file_name("img"));
    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
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
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.set_menus(vec![Menu {
                name: "img".into(),
                disabled: false,
                items: vec![
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
                    let view = cx.new(|cx| {
                        ImgDesktop::new(root.clone(), engine.clone(), reference, window, cx)
                    });
                    view.update(cx, |view, cx| view.startup_update_check(cx));
                    let close_view = view.downgrade();
                    let target_window = window.window_handle();
                    cx.on_action(move |_: &Quit, cx| {
                        let window = target_window;
                        let _ = window.update(cx, |_, window, cx| {
                            let _ =
                                close_view.update(cx, |view, cx| view.request_close(window, cx));
                        });
                    });
                    let close_view = view.downgrade();
                    let target_window = window.window_handle();
                    cx.on_action(move |_: &CloseWindow, cx| {
                        let window = target_window;
                        let _ = window.update(cx, |_, window, cx| {
                            let _ =
                                close_view.update(cx, |view, cx| view.request_close(window, cx));
                        });
                    });
                    let close_view = view.downgrade();
                    window.on_window_should_close(cx, move |window, cx| {
                        let _ = close_view.update(cx, |view, cx| view.request_close(window, cx));
                        false
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
