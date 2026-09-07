//! Global hotkeys on Windows/X11. Without a tray, closing the window exits safely.
use crate::desktop_runtime::{DesktopEvent, Shortcuts};
use anyhow::{Result, ensure};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{HotKey, Modifiers},
};
pub struct Native {
    manager: GlobalHotKeyManager,
    keys: Vec<HotKey>,
}
impl Native {
    pub fn new(sender: async_channel::Sender<DesktopEvent>) -> Result<Self> {
        let manager = GlobalHotKeyManager::new()?;
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let _ = sender.try_send(DesktopEvent::Hotkey(
                event.id,
                event.state == HotKeyState::Pressed,
            ));
        }));
        Ok(Self {
            manager,
            keys: vec![],
        })
    }
    pub fn configure(&mut self, shortcuts: &Shortcuts) -> Result<()> {
        let next = parse(shortcuts)?;
        let mut added = vec![];
        for key in &next {
            if !self.keys.contains(key) {
                if let Err(error) = self.manager.register(*key) {
                    let _ = self.manager.unregister_all(&added);
                    return Err(error.into());
                }
                added.push(*key);
            }
        }
        for key in &self.keys {
            if !next.contains(key) {
                self.manager.unregister(*key)?;
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
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.manager.unregister_all(&self.keys);
    }
}
pub fn restore_windows() {}
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
