use gpui_kit::{
    component::{Theme, ThemeMode},
    *,
};
use std::sync::atomic::{AtomicBool, Ordering};
static DARK: AtomicBool = AtomicBool::new(false);
pub fn color(value: u32) -> Rgba {
    rgb(if DARK.load(Ordering::Relaxed) {
        match value {
            CANVAS => 0x211e1b,
            SIDEBAR => 0x25211e,
            CARD => 0x2a2622,
            DROP => 0x302a23,
            TEXT => 0xf2ece3,
            MUTED => 0xb3a79a,
            BORDER => 0x3d362f,
            STRONG_BORDER => 0x5e5145,
            ORANGE => 0xffa83d,
            ORANGE_BRIGHT => 0xffb753,
            ORANGE_SOFT => 0x403020,
            ORANGE_BORDER => 0x806039,
            ACTIVE_CARD => 0x3a2c20,
            GREEN => 0x49cba0,
            RED => 0xff9180,
            TRACK => 0x433b32,
            BUTTON => 0xe5d7c6,
            NAV_ACTIVE => 0xffb860,
            NAV_SELECTED => 0x453626,
            NAV_TEXT => 0xd9ccbc,
            _ => value,
        }
    } else {
        value
    })
}
pub fn set_dark(dark: bool, cx: &mut App) {
    DARK.store(dark, Ordering::Relaxed);
    init(cx);
}

// Measured from the supplied 1280 × 1800 source. These semantic values are
// shared by product components and GPUI Kit, including its resolved tokens.
pub const CANVAS: u32 = 0xf7f3e8;
pub const SIDEBAR: u32 = 0xf9f5ec;
pub const CARD: u32 = 0xfaf7ee;
pub const DROP: u32 = 0xf6efe0;
pub const TEXT: u32 = 0x221811;
pub const MUTED: u32 = 0x85796d;
pub const BORDER: u32 = 0xeae6dc;
pub const STRONG_BORDER: u32 = 0xddd6c8;
pub const ORANGE: u32 = 0xe17900;
pub const ORANGE_BRIGHT: u32 = 0xf79500;
pub const ORANGE_SOFT: u32 = 0xf7e8d4;
pub const ORANGE_BORDER: u32 = 0xecc189;
pub const ACTIVE_CARD: u32 = 0xf5e6d0;
pub const GREEN: u32 = 0x00bc7d;
pub const RED: u32 = 0xc44737;
pub const TRACK: u32 = 0xe2ded5;
pub const BUTTON: u32 = 0x362d25;
pub const NAV_ACTIVE: u32 = 0x96500b;
pub const NAV_SELECTED: u32 = 0xe4dacb;
pub const NAV_TEXT: u32 = 0x55483c;

pub fn init(cx: &mut App) {
    Theme::change(
        if DARK.load(Ordering::Relaxed) {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let theme = Theme::global_mut(cx);
    theme.font_family = "Inter Variable".into();
    theme.font_size = px(16.);
    theme.radius = px(10.);
    theme.shadow = false;
    let c = &mut theme.colors;
    c.background = color(CANVAS).into();
    c.foreground = color(TEXT).into();
    c.border = color(BORDER).into();
    c.input = color(STRONG_BORDER).into();
    c.primary = color(TEXT).into();
    c.primary_foreground = color(CARD).into();
    c.primary_hover = color(BUTTON).into();
    c.primary_active = color(TEXT).into();
    c.button_primary = c.primary;
    c.button_primary_foreground = c.primary_foreground;
    c.button_primary_hover = c.primary_hover;
    c.button_primary_active = c.primary_active;
    c.button = color(CARD).into();
    c.button_foreground = color(TEXT).into();
    c.button_hover = color(DROP).into();
    c.button_active = color(ORANGE_SOFT).into();
    c.accent = color(ORANGE_SOFT).into();
    c.accent_foreground = color(ORANGE).into();
    c.muted = color(DROP).into();
    c.muted_foreground = color(MUTED).into();
    c.secondary = color(DROP).into();
    c.secondary_foreground = color(TEXT).into();
    c.popover = color(CARD).into();
    c.popover_foreground = color(TEXT).into();
    c.ring = color(ORANGE_BORDER).into();
    c.selection = color(ORANGE_SOFT).into();
    c.link = color(ORANGE).into();
    theme.tokens = theme.colors.into();
    Theme::sync_base(cx);
}
