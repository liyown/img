//! Portable image processing models shared by the renderer and desktop editor.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessingPlan {
    pub version: u32,
    pub encoding: Encoding,
    pub geometry: Geometry,
    pub annotations: Vec<Annotation>,
    pub watermark: Option<Watermark>,
    pub split: Option<Split>,
    pub stitch: Option<Stitch>,
}
impl Default for ProcessingPlan {
    fn default() -> Self {
        Self {
            version: 1,
            encoding: Encoding::default(),
            geometry: Geometry::default(),
            annotations: vec![],
            watermark: None,
            split: None,
            stitch: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Png,
    Jpeg,
    Webp,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Compression {
    Quality { quality: u8 },
    Target { bytes: u64 },
    Lossless,
}
impl Default for Compression {
    fn default() -> Self {
        Self::Quality { quality: 85 }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Encoding {
    pub format: Format,
    pub compression: Compression,
    pub jpeg_background: [u8; 3],
}
impl Default for Encoding {
    fn default() -> Self {
        Self {
            format: Format::Png,
            compression: Compression::default(),
            jpeg_background: [255; 3],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Geometry {
    pub resize: Option<Resize>,
    pub crop: Option<Rect>,
    pub quarter_turns: u8,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Resize {
    pub width: u32,
    pub height: u32,
    pub max_edge: u32,
    pub keep_aspect: bool,
    pub allow_upscale: bool,
}
impl Default for Resize {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            max_edge: 0,
            keep_aspect: true,
            allow_upscale: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub id: String,
    pub shape: Shape,
    pub color: [u8; 4],
    pub stroke_width: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Arrow { from: Point, to: Point },
    Rectangle { from: Point, to: Point },
    Redact { from: Point, to: Point },
    Text { at: Point, text: String, size: f32 },
    Step { at: Point, number: u16, radius: f32 },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    #[default]
    BottomRight,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Watermark {
    pub resource: String,
    pub position: Position,
    pub margin: u32,
    pub scale: f32,
    pub opacity: f32,
}
impl Default for Watermark {
    fn default() -> Self {
        Self {
            resource: String::new(),
            position: Position::BottomRight,
            margin: 16,
            scale: 0.2,
            opacity: 0.6,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Split {
    Height { height: u32 },
    Grid { rows: u32, columns: u32 },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Horizontal,
    #[default]
    Vertical,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Stitch {
    pub direction: Direction,
    pub spacing: u32,
    pub background: [u8; 4],
    pub cross_size: Option<u32>,
    pub allow_upscale: bool,
}
impl Default for Stitch {
    fn default() -> Self {
        Self {
            direction: Direction::Vertical,
            spacing: 0,
            background: [255; 4],
            cross_size: None,
            allow_upscale: false,
        }
    }
}
#[doc(hidden)]
pub fn dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0 && height > 0 && width <= 32768 && height <= 32768,
        "output edge must be between 1 and 32768 pixels"
    );
    ensure!(
        u64::from(width) * u64::from(height) <= 40_000_000,
        "output exceeds 40 megapixel processing limit"
    );
    Ok(())
}
impl ProcessingPlan {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported processing plan version");
        match self.encoding.compression {
            Compression::Quality { quality } => {
                ensure!((1..=100).contains(&quality), "quality must be 1–100")
            }
            Compression::Target { bytes } => ensure!(bytes > 0, "target size must be positive"),
            Compression::Lossless => ensure!(
                self.encoding.format != Format::Jpeg,
                "JPEG does not support lossless encoding; choose quality or target size"
            ),
        }
        ensure!(
            self.geometry.quarter_turns <= 3,
            "rotation must be 0, 1, 2 or 3 quarter turns"
        );
        if let Some(resize) = &self.geometry.resize {
            ensure!(
                resize.width.max(resize.height).max(resize.max_edge) <= 32768,
                "resize edge exceeds 32768"
            );
            ensure!(
                resize.width > 0 || resize.height > 0 || resize.max_edge > 0,
                "resize requires a dimension"
            );
            ensure!(
                resize.keep_aspect || (resize.width > 0 && resize.height > 0),
                "exact resize requires width and height"
            );
            ensure!(
                resize.max_edge == 0 || (resize.width == 0 && resize.height == 0),
                "choose longest edge or width/height"
            );
        }
        if let Some(crop) = &self.geometry.crop {
            dimensions(crop.width, crop.height)?;
        }
        if let Some(split) = &self.split {
            match split {
                Split::Height { height } => ensure!(
                    *height > 0 && *height <= 32768,
                    "split height must be 1–32768"
                ),
                Split::Grid { rows, columns } => ensure!(
                    *rows > 0 && *columns > 0 && u64::from(*rows) * u64::from(*columns) <= 1024,
                    "split grid must contain 1–1024 tiles"
                ),
            }
        }
        if let Some(stitch) = &self.stitch {
            ensure!(
                stitch.spacing <= 32768 && stitch.cross_size.is_none_or(|s| s > 0 && s <= 32768),
                "invalid stitch spacing or size"
            );
        }
        if let Some(mark) = &self.watermark {
            ensure!(
                mark.scale.is_finite()
                    && mark.scale > 0.
                    && mark.scale <= 1.
                    && mark.opacity.is_finite()
                    && (0.0..=1.0).contains(&mark.opacity)
                    && mark.margin <= 32768,
                "invalid watermark scale, opacity or margin"
            );
            ensure!(
                mark.resource.is_empty()
                    || (mark.resource.len() == 64
                        && mark.resource.bytes().all(|b| b.is_ascii_hexdigit())),
                "watermark resource must be a content hash"
            );
        }
        ensure!(self.annotations.len() <= 256, "too many annotations");
        let mut ids = std::collections::HashSet::new();
        for annotation in &self.annotations {
            ensure!(
                !annotation.id.is_empty() && ids.insert(&annotation.id),
                "annotation IDs must be unique"
            );
            ensure!(
                annotation.stroke_width.is_finite()
                    && (0.5..=256.).contains(&annotation.stroke_width),
                "invalid annotation stroke width"
            );
            let point = |p: Point| {
                ensure!(
                    p.x.is_finite()
                        && p.y.is_finite()
                        && p.x.abs() <= 32768.
                        && p.y.abs() <= 32768.,
                    "invalid annotation position"
                );
                Ok(())
            };
            match &annotation.shape {
                Shape::Arrow { from, to }
                | Shape::Rectangle { from, to }
                | Shape::Redact { from, to } => {
                    point(*from)?;
                    point(*to)?;
                }
                Shape::Text { at, text, size } => {
                    point(*at)?;
                    ensure!(
                        text.chars().count() <= 2048
                            && size.is_finite()
                            && (1.0..=1024.).contains(size),
                        "invalid annotation text size or length"
                    );
                }
                Shape::Step { at, number, radius } => {
                    point(*at)?;
                    ensure!(
                        *number > 0
                            && *number <= 9999
                            && radius.is_finite()
                            && (2.0..=512.).contains(radius),
                        "invalid step label"
                    );
                }
            }
        }
        Ok(())
    }
}
