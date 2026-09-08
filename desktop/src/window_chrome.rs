use gpui_kit::prelude::*;
use gpui_kit::*;

/// AppKit owns resizing; these narrow regions only supply cursor feedback over
/// the full-size content view. Linux and Windows retain their platform borders.
pub fn resize_cursors(window: &Window) -> Vec<AnyElement> {
    if !cfg!(target_os = "macos") || window.is_fullscreen() {
        return Vec::new();
    }
    let region = |id, cursor| div().id(id).absolute().cursor(cursor);
    vec![
        region("resize-top", CursorStyle::ResizeUpDown)
            .top_0()
            .left(px(10.))
            .right(px(10.))
            .h(px(4.))
            .into_any_element(),
        region("resize-bottom", CursorStyle::ResizeUpDown)
            .bottom_0()
            .left(px(10.))
            .right(px(10.))
            .h(px(4.))
            .into_any_element(),
        region("resize-left", CursorStyle::ResizeLeftRight)
            .left_0()
            .top(px(10.))
            .bottom(px(10.))
            .w(px(4.))
            .into_any_element(),
        region("resize-right", CursorStyle::ResizeLeftRight)
            .right_0()
            .top(px(10.))
            .bottom(px(10.))
            .w(px(4.))
            .into_any_element(),
        region("resize-top-left", CursorStyle::ResizeUpLeftDownRight)
            .top_0()
            .left_0()
            .size(px(10.))
            .into_any_element(),
        region("resize-bottom-right", CursorStyle::ResizeUpLeftDownRight)
            .bottom_0()
            .right_0()
            .size(px(10.))
            .into_any_element(),
        region("resize-top-right", CursorStyle::ResizeUpRightDownLeft)
            .top_0()
            .right_0()
            .size(px(10.))
            .into_any_element(),
        region("resize-bottom-left", CursorStyle::ResizeUpRightDownLeft)
            .bottom_0()
            .left_0()
            .size(px(10.))
            .into_any_element(),
    ]
}
