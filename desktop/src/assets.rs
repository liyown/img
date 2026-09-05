use gpui_kit::*;
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Embedded;

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(asset) = Embedded::get(path) {
            return Ok(Some(asset.data));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Embedded::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.into_owned()))
            .collect())
    }
}

pub fn icon(name: &str, size: f32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_shrink_0()
}

pub fn bytes(path: &str) -> anyhow::Result<Cow<'static, [u8]>> {
    Embedded::get(path)
        .map(|a| a.data)
        .ok_or_else(|| anyhow::anyhow!("资源不可用"))
}
