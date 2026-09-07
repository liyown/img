//! AppKit objects and global shortcut registration stay on the application main thread.
use crate::desktop_runtime::{DesktopEvent, Shortcuts};
use anyhow::{Context, Result, ensure};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{HotKey, Modifiers},
};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel,
};
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

pub struct MenuIvars {
    sender: async_channel::Sender<DesktopEvent>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = MenuIvars]
    struct ImgMenuTarget;
    unsafe impl NSObjectProtocol for ImgMenuTarget {}
    impl ImgMenuTarget {
        #[unsafe(method(performMenuAction:))]
        fn perform(&self, sender: &NSMenuItem) {
            let event = match sender.tag() {
                1 => DesktopEvent::Open, 2 => DesktopEvent::Paste,
                3 => DesktopEvent::Capture, 4 => DesktopEvent::TogglePause,
                5 => DesktopEvent::Quit, _ => return,
            };
            let _ = self.ivars().sender.try_send(event);
        }
    }
);

pub struct Native {
    bar: Retained<NSStatusBar>,
    item: Retained<NSStatusItem>,
    _target: Retained<ImgMenuTarget>,
    status: Retained<NSMenuItem>,
    pause: Retained<NSMenuItem>,
    manager: GlobalHotKeyManager,
    keys: Vec<HotKey>,
    leases: std::collections::HashMap<u32, std::fs::File>,
}
impl Native {
    pub fn has_tray(&self) -> bool {
        true
    }
    pub fn new(sender: async_channel::Sender<DesktopEvent>) -> Result<Self> {
        let mtm = MainThreadMarker::new().context("菜单栏必须在主线程初始化")?;
        let target = ImgMenuTarget::alloc(mtm).set_ivars(MenuIvars {
            sender: sender.clone(),
        });
        let target: Retained<ImgMenuTarget> = unsafe { msg_send![super(target), init] };
        let bar = NSStatusBar::systemStatusBar();
        let item = bar.statusItemWithLength(-1.);
        let button = item.button(mtm).context("无法创建菜单栏按钮")?;
        button.setTitle(&NSString::from_str("img"));
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("img"));
        menu.setAutoenablesItems(false);
        let make = |title: &str, tag: isize| {
            let menu_item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(&crate::i18n::text(title.to_owned())),
                    (tag > 0).then(|| sel!(performMenuAction:)),
                    &NSString::new(),
                )
            };
            menu_item.setTag(tag);
            menu_item.setEnabled(tag > 0);
            unsafe {
                menu_item.setTarget(Some(&target));
            }
            menu.addItem(&menu_item);
            menu_item
        };
        let status = make("就绪", 0);
        make("打开 img", 1);
        make("上传剪贴板", 2);
        make("截图上传…", 3);
        let pause = make("暂停上传", 4);
        make("退出 img", 5);
        item.setMenu(Some(&menu));
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(error) => {
                bar.removeStatusItem(&item);
                return Err(error.into());
            }
        };
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let _ = sender.try_send(DesktopEvent::Hotkey(
                event.id,
                event.state == HotKeyState::Pressed,
            ));
        }));
        Ok(Self {
            bar,
            item,
            _target: target,
            status,
            pause,
            manager,
            keys: vec![],
            leases: Default::default(),
        })
    }
    pub fn configure(&mut self, shortcuts: &Shortcuts) -> Result<()> {
        let next = parse(shortcuts)?;
        let mut added = vec![];
        let mut leases = std::collections::HashMap::new();
        for key in &next {
            if !self.keys.contains(key) {
                let lease = shortcut_lease(key.id);
                if lease.is_err() || self.manager.register(*key).is_err() {
                    for key in added {
                        let _ = self.manager.unregister(key);
                    }
                    anyhow::bail!(
                        "快捷键 {key} 注册失败，可能已被其他应用占用。原有快捷键保持不变。"
                    );
                }
                leases.insert(key.id, lease?);
                added.push(*key);
            }
        }
        for key in &self.keys {
            if !next.contains(key) {
                self.manager.unregister(*key)?;
            }
        }
        self.leases
            .retain(|id, _| next.iter().any(|key| key.id == *id));
        self.leases.extend(leases);
        self.keys = next;
        Ok(())
    }
    pub fn action(&self, id: u32) -> Option<DesktopEvent> {
        self.keys.iter().position(|key| key.id == id).map(|i| {
            if i == 0 {
                DesktopEvent::Paste
            } else {
                DesktopEvent::Capture
            }
        })
    }
    pub fn status(&self, text: &str, paused: bool) {
        self.status
            .setTitle(&NSString::from_str(&crate::i18n::text(text.to_owned())));
        self.pause
            .setTitle(&NSString::from_str(&crate::i18n::text(if paused {
                "继续上传"
            } else {
                "暂停上传"
            })));
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.manager.unregister_all(&self.keys);
        self.bar.removeStatusItem(&self.item);
    }
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

// Carbon permits different apps to subscribe to the same key. A user-local lease
// prevents duplicate img sessions; registration stays non-exclusive to preserve
// other apps' shortcuts. It cannot enumerate third-party non-exclusive bindings.
fn shortcut_lease(id: u32) -> Result<std::fs::File> {
    let root = if let Some(path) = std::env::var_os("APERTURE_HOTKEY_DIR") {
        std::path::PathBuf::from(path)
    } else {
        directories::BaseDirs::new()
            .context("无法确定快捷键锁目录")?
            .cache_dir()
            .join("img/hotkeys")
    };
    std::fs::create_dir_all(&root)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(format!("{id}.lock")))?;
    file.try_lock().context("该快捷键已被另一个 img 实例注册")?;
    Ok(file)
}

pub fn restore_windows() {
    if let Some(mtm) = MainThreadMarker::new() {
        for window in objc2_app_kit::NSApplication::sharedApplication(mtm)
            .windows()
            .iter()
        {
            if window.isMiniaturized() {
                window.deminiaturize(None);
            }
        }
    }
}
