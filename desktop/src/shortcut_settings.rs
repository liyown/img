use crate::{
    desktop_runtime::{DesktopRuntime, Shortcuts},
    theme::*,
};
use gpui_kit::{
    component::{
        button::*,
        input::{Input, InputState},
        switch::Switch,
        *,
    },
    prelude::*,
    *,
};
use std::path::Path;
pub struct ShortcutSettings {
    enabled: bool,
    clipboard: Entity<InputState>,
    screenshot: Entity<InputState>,
    notice: Option<(String, bool)>,
}
impl ShortcutSettings {
    pub fn new(root: &Path, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Shortcuts::load(root).unwrap_or_default();
        Self {
            enabled: settings.enabled,
            clipboard: cx.new(|cx| InputState::new(window, cx).default_value(settings.clipboard)),
            screenshot: cx.new(|cx| InputState::new(window, cx).default_value(settings.screenshot)),
            notice: None,
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let settings = Shortcuts {
            enabled: self.enabled,
            clipboard: self.clipboard.read(cx).value().trim().into(),
            screenshot: self.screenshot.read(cx).value().trim().into(),
        };
        self.notice = Some(match DesktopRuntime::configure(settings, cx) {
            Ok(()) => ("快捷键已保存并生效".into(), false),
            Err(e) => (e.to_string(), true),
        });
        cx.notify();
    }
}
impl Render for ShortcutSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(20.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(crate::i18n::text("后台与全局快捷键")),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(crate::theme::color(MUTED))
                    .child(crate::i18n::text(
                        "关闭窗口或 ⌘W 后继续上传，⌘Q 退出。快捷操作自动上传并复制成功链接。",
                    )),
            )
            .child(
                Switch::new("global-shortcuts")
                    .label(crate::i18n::text("启用全局快捷键"))
                    .checked(self.enabled)
                    .on_click(cx.listener(|this, value, _, cx| {
                        this.enabled = *value;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(div().w(px(120.)).child(crate::i18n::text("上传剪贴板")))
                    .child(Input::new(&self.clipboard).disabled(!self.enabled)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(div().w(px(120.)).child(crate::i18n::text("截图上传")))
                    .child(Input::new(&self.screenshot).disabled(!self.enabled)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(crate::theme::color(MUTED))
                    .child(crate::i18n::text(
                        "例如 Cmd+Alt+U。需要修饰键；冲突时保留原快捷键，并显示注册失败。",
                    )),
            )
            .when_some(self.notice.clone(), |d, (text, error)| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(crate::theme::color(if error { RED } else { GREEN }))
                        .child(text),
                )
            })
            .child(
                Button::new("save-shortcuts")
                    .label(crate::i18n::text("保存快捷键"))
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            )
    }
}
