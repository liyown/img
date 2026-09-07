//! App-owned session: closing the window does not release its queue, writer or child processes.
use crate::{native::Native, ui::ImgDesktop};
use gpui_kit::{AnyWindowHandle, App, BorrowAppContext, Entity, Global};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub enum DesktopEvent {
    Open,
    Hide,
    Paste,
    Capture,
    TogglePause,
    Quit,
    Hotkey(u32, bool),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Shortcuts {
    pub enabled: bool,
    pub clipboard: String,
    pub screenshot: String,
}
impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            enabled: true,
            clipboard: "Cmd+Alt+U".into(),
            screenshot: "Cmd+Alt+S".into(),
        }
    }
}
impl Shortcuts {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("shortcuts.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
    fn save(&self, root: &Path) -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.as_file().sync_all()?;
        file.persist(root.join("shortcuts.json"))
            .map_err(|e| e.error)?;
        Ok(())
    }
}
#[derive(Default)]
struct PressGate(HashSet<u32>);
impl PressGate {
    fn accept(&mut self, id: u32, pressed: bool) -> bool {
        if pressed {
            self.0.insert(id)
        } else {
            self.0.remove(&id);
            false
        }
    }
}
pub struct DesktopRuntime {
    view: Entity<ImgDesktop>,
    window: AnyWindowHandle,
    native: Option<Native>,
    root: PathBuf,
    shortcuts: Shortcuts,
    depressed: PressGate,
}
impl Global for DesktopRuntime {}
impl DesktopRuntime {
    pub fn install(view: Entity<ImgDesktop>, window: AnyWindowHandle, root: PathBuf, cx: &mut App) {
        let (sender, receiver) = async_channel::unbounded();
        let settings = Shortcuts::load(&root);
        let mut errors = vec![];
        let shortcuts = settings.unwrap_or_else(|_| {
            errors.push("快捷键设置无法读取，请在设置中重新保存。".to_string());
            Shortcuts {
                enabled: false,
                ..Default::default()
            }
        });
        let native = match Native::new(sender) {
            Ok(mut native) => {
                if let Err(e) = native.configure(&shortcuts) {
                    errors.push(e.to_string());
                }
                Some(native)
            }
            Err(_) => {
                errors.push("菜单栏初始化失败，窗口将保持可用；请重新启动 img。".into());
                None
            }
        };
        cx.set_global(Self {
            view: view.clone(),
            window,
            native,
            root,
            shortcuts,
            depressed: PressGate::default(),
        });
        if !errors.is_empty() {
            view.update(cx, |view, cx| view.desktop_notice(errors.join("；"), cx));
        }
        cx.spawn(async move |cx| {
            while let Ok(event) = receiver.recv().await {
                cx.update(|cx| Self::dispatch(event, cx));
            }
        })
        .detach();
        cx.on_system_notification_response(|response, cx| {
            Self::open(cx, Some(response.tag.to_string()));
        });
    }
    pub fn dispatch(event: DesktopEvent, cx: &mut App) {
        // GPUI menu actions may be dispatched while the active window is borrowed.
        cx.defer(move |cx| Self::dispatch_now(event, cx));
    }
    fn dispatch_now(event: DesktopEvent, cx: &mut App) {
        let event = if let DesktopEvent::Hotkey(id, pressed) = event {
            cx.update_global::<Self, _>(|runtime, _| {
                if !runtime.depressed.accept(id, pressed) {
                    return None;
                }
                runtime.native.as_ref().and_then(|n| n.action(id))
            })
        } else {
            Some(event)
        };
        let Some(event) = event else { return };
        let runtime = cx.global::<Self>();
        let view = runtime.view.clone();
        let handle = runtime.window;
        let _ = handle.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.desktop_event(event, window, cx))
        });
    }
    pub fn open(cx: &mut App, tag: Option<String>) {
        let Some(runtime) = cx.try_global::<Self>() else {
            return;
        };
        let view = runtime.view.clone();
        let handle = runtime.window;
        let _ = handle.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.show_from_background(tag, window, cx))
        });
    }
    pub fn can_hide(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(|s| s.native.is_some())
    }
    pub fn status(cx: &App, text: &str, paused: bool) {
        if let Some(native) = cx.try_global::<Self>().and_then(|s| s.native.as_ref()) {
            native.status(text, paused);
        }
    }
    pub fn configure(shortcuts: Shortcuts, cx: &mut App) -> anyhow::Result<()> {
        cx.update_global::<Self, _>(|runtime, _| {
            let native = runtime
                .native
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("菜单栏不可用，请重启应用后再设置快捷键"))?;
            native.configure(&shortcuts)?;
            if let Err(error) = shortcuts.save(&runtime.root) {
                let _ = native.configure(&runtime.shortcuts);
                return Err(error);
            }
            runtime.shortcuts = shortcuts;
            runtime.depressed.0.clear();
            Ok(())
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_validate_and_roundtrip_without_touching_other_preferences() {
        let root = tempfile::tempdir().unwrap();
        let defaults = Shortcuts::load(root.path()).unwrap();
        assert_eq!(crate::native::parse(&defaults).unwrap().len(), 2);
        assert!(
            crate::native::parse(&Shortcuts {
                screenshot: defaults.clipboard.clone(),
                ..defaults.clone()
            })
            .is_err()
        );
        assert!(
            crate::native::parse(&Shortcuts {
                clipboard: "A".into(),
                ..defaults.clone()
            })
            .is_err()
        );
        let disabled = Shortcuts {
            enabled: false,
            ..defaults
        };
        disabled.save(root.path()).unwrap();
        assert_eq!(Shortcuts::load(root.path()).unwrap(), disabled);
        assert!(crate::native::parse(&disabled).unwrap().is_empty());
    }
    #[test]
    fn held_shortcuts_do_not_repeat_and_release_allows_next_upload() {
        let mut gate = PressGate::default();
        assert!(gate.accept(1, true));
        assert!(!gate.accept(1, true));
        assert!(gate.accept(2, true));
        assert!(!gate.accept(1, false));
        assert!(gate.accept(1, true));
    }
}
