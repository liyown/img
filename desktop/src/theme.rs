use gpui_kit::{
    component::{Theme, ThemeMode},
    *,
};

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
    Theme::change(ThemeMode::Light, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = "Inter Variable".into();
    theme.font_size = px(16.);
    theme.radius = px(10.);
    theme.shadow = false;
    let c = &mut theme.colors;
    c.background = rgb(CANVAS).into();
    c.foreground = rgb(TEXT).into();
    c.border = rgb(BORDER).into();
    c.input = rgb(STRONG_BORDER).into();
    c.primary = rgb(TEXT).into();
    c.primary_foreground = rgb(CARD).into();
    c.primary_hover = rgb(BUTTON).into();
    c.primary_active = rgb(TEXT).into();
    c.button_primary = c.primary;
    c.button_primary_foreground = c.primary_foreground;
    c.button_primary_hover = c.primary_hover;
    c.button_primary_active = c.primary_active;
    c.button = rgb(CARD).into();
    c.button_foreground = rgb(TEXT).into();
    c.button_hover = rgb(DROP).into();
    c.button_active = rgb(ORANGE_SOFT).into();
    c.accent = rgb(ORANGE_SOFT).into();
    c.accent_foreground = rgb(ORANGE).into();
    c.muted = rgb(DROP).into();
    c.muted_foreground = rgb(MUTED).into();
    c.secondary = rgb(DROP).into();
    c.secondary_foreground = rgb(TEXT).into();
    c.popover = rgb(CARD).into();
    c.popover_foreground = rgb(TEXT).into();
    c.ring = rgb(ORANGE_BORDER).into();
    c.selection = rgb(ORANGE_SOFT).into();
    c.link = rgb(ORANGE).into();
    theme.tokens = theme.colors.into();
    Theme::sync_base(cx);
}
