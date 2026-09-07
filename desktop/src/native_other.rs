//! Global hotkeys on Windows/X11. Without a tray, closing the window exits safely.
use crate::desktop_runtime::{DesktopEvent, Shortcuts};
use anyhow::{Result, ensure};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{HotKey, Modifiers},
};
pub struct Native {
    #[cfg(windows)]
    tray: Option<tray_icon::TrayIcon>,
    #[cfg(target_os = "linux")]
    tray: Option<ksni::blocking::Handle<LinuxTray>>,
    manager: Option<GlobalHotKeyManager>,
    keys: Vec<HotKey>,
    #[cfg(target_os = "linux")]
    portal: Option<async_channel::Sender<Shortcuts>>,
}
impl Native {
    pub fn new(sender: async_channel::Sender<DesktopEvent>) -> Result<Self> {
        let tray = create_tray(sender.clone()).ok();
        #[cfg(target_os = "linux")]
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return Ok(Self {
                manager: None,
                keys: vec![],
                portal: Some(portal_shortcuts(sender)),
                tray,
            });
        }
        let manager = GlobalHotKeyManager::new().ok();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let _ = sender.try_send(DesktopEvent::Hotkey(
                event.id,
                event.state == HotKeyState::Pressed,
            ));
        }));
        Ok(Self {
            manager,
            tray,
            keys: vec![],
            #[cfg(target_os = "linux")]
            portal: None,
        })
    }
    pub fn configure(&mut self, shortcuts: &Shortcuts) -> Result<()> {
        let next = parse(shortcuts)?;
        #[cfg(target_os = "linux")]
        if let Some(portal) = &self.portal {
            portal.try_send(shortcuts.clone())?;
            return Ok(());
        }
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("快捷键管理器不可用"))?;
        let mut added = vec![];
        for key in &next {
            if !self.keys.contains(key) {
                if let Err(error) = manager.register(*key) {
                    let _ = manager.unregister_all(&added);
                    return Err(error.into());
                }
                added.push(*key);
            }
        }
        for key in &self.keys {
            if !next.contains(key) {
                manager.unregister(*key)?;
            }
        }
        self.keys = next;
        Ok(())
    }
    pub fn action(&self, id: u32) -> Option<DesktopEvent> {
        self.keys.iter().position(|key| key.id == id).map(|n| {
            if n == 0 {
                DesktopEvent::Paste
            } else {
                DesktopEvent::Capture
            }
        })
    }
    pub fn status(&self, _: &str, _: bool) {}
    pub fn has_tray(&self) -> bool {
        self.tray.is_some()
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        if let Some(manager) = &self.manager {
            let _ = manager.unregister_all(&self.keys);
        }
    }
}
pub fn restore_windows() {}
fn tray_pixels() -> anyhow::Result<Vec<u8>> {
    let bytes = crate::assets::bytes("icons/image.svg")
        .ok_or_else(|| anyhow::anyhow!("missing tray icon"))?;
    let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(32, 32).unwrap();
    pixmap.fill(resvg::tiny_skia::Color::from_rgba8(247, 243, 232, 255));
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            32. / tree.size().width(),
            32. / tree.size().height(),
        ),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.take())
}
#[cfg(windows)]
fn create_tray(sender: async_channel::Sender<DesktopEvent>) -> anyhow::Result<tray_icon::TrayIcon> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    let menu = Menu::new();
    let mut events = vec![];
    for (title, event) in [
        ("打开 img", DesktopEvent::Open),
        ("上传剪贴板", DesktopEvent::Paste),
        ("截图上传", DesktopEvent::Capture),
        ("暂停 / 继续", DesktopEvent::TogglePause),
        ("退出", DesktopEvent::Quit),
    ] {
        let item = MenuItem::new(title, true, None);
        events.push((item.id().clone(), event));
        menu.append(&item)?;
    }
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some((_, action)) = events.iter().find(|(id, _)| id == event.id()) {
            let _ = sender.try_send(action.clone());
        }
    }));
    Ok(tray_icon::TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("img")
        .with_icon(tray_icon::Icon::from_rgba(tray_pixels()?, 32, 32)?)
        .build()?)
}
#[cfg(target_os = "linux")]
struct LinuxTray {
    sender: async_channel::Sender<DesktopEvent>,
    pixels: Vec<u8>,
}
#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "dev.img.desktop".into()
    }
    fn title(&self) -> String {
        "img".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon {
            width: 32,
            height: 32,
            data: self.pixels.clone(),
        }]
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.try_send(DesktopEvent::Open);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        [
            ("打开 img", DesktopEvent::Open),
            ("上传剪贴板", DesktopEvent::Paste),
            ("截图上传", DesktopEvent::Capture),
            ("暂停 / 继续", DesktopEvent::TogglePause),
            ("退出", DesktopEvent::Quit),
        ]
        .into_iter()
        .map(|(label, event)| {
            ksni::menu::StandardItem {
                label: label.into(),
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray.sender.try_send(event.clone());
                }),
                ..Default::default()
            }
            .into()
        })
        .collect()
    }
}
#[cfg(target_os = "linux")]
fn create_tray(
    sender: async_channel::Sender<DesktopEvent>,
) -> anyhow::Result<ksni::blocking::Handle<LinuxTray>> {
    use ksni::blocking::TrayMethods;
    let rgba = tray_pixels()?;
    let pixels = rgba
        .chunks_exact(4)
        .flat_map(|p| [p[3], p[0], p[1], p[2]])
        .collect();
    Ok(LinuxTray { sender, pixels }.spawn()?)
}
#[cfg(target_os = "linux")]
fn portal_shortcuts(
    events: async_channel::Sender<DesktopEvent>,
) -> async_channel::Sender<Shortcuts> {
    let (tx, rx) = async_channel::unbounded::<Shortcuts>();
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            use ashpd::desktop::global_shortcuts::{GlobalShortcuts,NewShortcut};
            use futures_lite::StreamExt;
            let mut next=rx.recv().await.ok();
            while let Some(settings)=next.take() {
                if !settings.enabled {next=rx.recv().await.ok();continue;}
                let setup=async {
                    let portal=GlobalShortcuts::new().await?;
                    let session=portal.create_session(Default::default()).await?;
                    let activated=portal.receive_activated().await?;
                    let trigger=|s:&str|s.replace("Ctrl","CTRL").replace("Cmd","LOGO").replace("Super","LOGO").replace("Alt","ALT").replace("Shift","SHIFT");
                    let paste=trigger(&settings.clipboard);let capture=trigger(&settings.screenshot);
                    portal.bind_shortcuts(&session,&[
                        NewShortcut::new("paste","Upload clipboard image").preferred_trigger(paste.as_str()),
                        NewShortcut::new("capture","Capture and upload image").preferred_trigger(capture.as_str()),
                    ],None,Default::default()).await?.response()?;
                    Ok::<_,ashpd::Error>((session,activated))
                }.await;
                match setup {
                    Ok((session,mut activated)) => {
                        let _=events.send(DesktopEvent::Notice("系统已授权全局快捷键。实际按键可在系统快捷键设置中调整。".into())).await;
                        loop {
                            tokio::select! {
                                setting=rx.recv() => {next=setting.ok();break;}
                                activation=activated.next() => {
                                    let Some(activation)=activation else {break;};
                                    let event=if activation.shortcut_id()=="paste" {DesktopEvent::Paste} else {DesktopEvent::Capture};
                                    if events.send(event).await.is_err() {break;}
                                }
                            }
                        }
                        let _=session.close().await;
                    }
                    Err(_) => {
                        let _=events.send(DesktopEvent::Notice("系统未授权全局快捷键或门户不支持此功能。可在设置中重试授权，窗口上传仍可使用。".into())).await;
                        next=rx.recv().await.ok();
                    }
                }
            }
        });
    });
    tx
}
pub fn parse(shortcuts: &Shortcuts) -> Result<Vec<HotKey>> {
    if !shortcuts.enabled {
        return Ok(vec![]);
    }
    let keys: Vec<HotKey> = [&shortcuts.clipboard, &shortcuts.screenshot]
        .into_iter()
        .map(|s| {
            s.parse()
                .map_err(|_| anyhow::anyhow!("快捷键格式无效，例如 Cmd+Alt+U"))
        })
        .collect::<Result<_>>()?;
    ensure!(keys[0] != keys[1], "两个操作不能使用相同快捷键");
    ensure!(
        keys.iter().all(|key| key
            .mods
            .intersects(Modifiers::SUPER | Modifiers::CONTROL | Modifiers::ALT)),
        "全局快捷键需要 Cmd、Ctrl 或 Alt 修饰键"
    );
    Ok(keys)
}
