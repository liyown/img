use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader, metadata::Orientation};
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Read},
    path::Path,
};

pub fn detect(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok("image/png");
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Ok("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("image/webp");
    }
    if bytes.len() >= 16
        && &bytes[4..8] == b"ftyp"
        && bytes[8..bytes.len().min(32)]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|x| x == b"avif" || x == b"avis")
    {
        return Ok("image/avif");
    }
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let mut s = text.trim_start_matches('\u{feff}').trim_start();
    if s.starts_with("<?xml")
        && let Some((_, tail)) = s.split_once("?>")
    {
        s = tail.trim_start();
    }
    if s.starts_with("<svg")
        && s.as_bytes()
            .get(4)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>')
    {
        return Ok("image/svg+xml");
    }
    anyhow::bail!("not a supported image (PNG, JPEG, GIF, WebP, SVG or AVIF required)")
}
pub fn read_image(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)
        .with_context(|| format!("cannot open image {}", path.display()))?;
    let st = f.metadata()?;
    ensure!(st.is_file(), "image is not a regular file");
    ensure!(
        st.len() > 0 && st.len() <= limit,
        "image is empty or exceeds maximum size of {limit} bytes"
    );
    let mut bytes = Vec::with_capacity(st.len() as usize);
    (&mut f).take(limit + 1).read_to_end(&mut bytes)?;
    inspect(&bytes, limit)?;
    Ok(bytes)
}
pub fn inspect(bytes: &[u8], limit: u64) -> Result<&'static str> {
    ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= limit,
        "image is empty or exceeds maximum size of {limit} bytes"
    );
    detect(bytes)
}
fn decoder(data: &[u8]) -> Result<impl ImageDecoder + '_> {
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 << 20);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let decoder = reader.into_decoder()?;
    let (w, h) = decoder.dimensions();
    ensure!(
        u64::from(w) * u64::from(h) <= 40_000_000,
        "image exceeds 40 megapixel processing limit"
    );
    Ok(decoder)
}
fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&DynamicImage::ImageRgb8(img.to_rgb8()))?;
    Ok(out)
}
fn encode_webp(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let rgba = img.to_rgba8();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out).write_image(
        &rgba,
        rgba.width(),
        rgba.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}
fn transparent(img: &DynamicImage) -> bool {
    img.has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] != 255)
}

// Strip JPEG metadata without touching compressed scan data. Never treat bytes
// inside the entropy-coded image as a segment. Preserve ICC (APP2) color data.
pub fn strip_jpeg(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(data.starts_with(b"\xff\xd8"), "invalid JPEG");
    let mut out = vec![0xff, 0xd8];
    let mut pos = 2;
    while pos < data.len() {
        let start = pos;
        ensure!(data[pos] == 0xff, "invalid JPEG marker");
        while pos < data.len() && data[pos] == 0xff {
            pos += 1;
        }
        let marker = *data.get(pos).context("truncated JPEG marker")?;
        pos += 1;
        if marker == 0xda || marker == 0xd9 {
            out.extend_from_slice(&data[start..]);
            return Ok(out);
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            out.extend_from_slice(&data[start..pos]);
            continue;
        }
        let n = data.get(pos..pos + 2).context("truncated JPEG segment")?;
        let len = u16::from_be_bytes([n[0], n[1]]) as usize;
        ensure!(
            len >= 2 && pos + len <= data.len(),
            "invalid JPEG segment length"
        );
        if !matches!(marker, 0xe1 | 0xed | 0xfe) {
            out.extend_from_slice(&data[start..pos + len]);
        }
        pos += len;
    }
    anyhow::bail!("JPEG has no image scan")
}
pub struct Processed {
    pub data: Vec<u8>,
    pub content_type: String,
    pub original_size: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    pub format: String,
    pub quality: u8,
    pub max_edge: u32,
    pub watermark: String,
    pub opacity: u8,
}
impl Default for Recipe {
    fn default() -> Self {
        Self {
            format: "original".into(),
            quality: 85,
            max_edge: 0,
            watermark: String::new(),
            opacity: 60,
        }
    }
}
impl Recipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.format.as_str(), "original" | "png" | "jpeg" | "webp"),
            "unsupported output image format"
        );
        ensure!(
            (1..=100).contains(&self.quality) && self.opacity <= 100,
            "quality must be 1–100 and opacity 0–100"
        );
        ensure!(self.max_edge <= 32768, "longest edge exceeds 32768 pixels");
        Ok(())
    }
    pub fn preset(name: &str) -> Self {
        match name {
            "web" => Self {
                format: "webp".into(),
                max_edge: 1600,
                ..Self::default()
            },
            "photo" => Self {
                format: "jpeg".into(),
                max_edge: 2400,
                quality: 82,
                ..Self::default()
            },
            _ => Self::default(),
        }
    }
}
pub fn process_recipe(
    data: Vec<u8>,
    ct: &str,
    strip: bool,
    max_width: u32,
    optimize: bool,
    recipe: &Recipe,
) -> Result<Processed> {
    recipe.validate()?;
    if recipe == &Recipe::default() {
        return process(data, ct, strip, max_width, optimize);
    }
    ensure!(
        matches!(ct, "image/png" | "image/jpeg" | "image/webp"),
        "explicit image processing supports PNG, JPEG and static WebP; animation/vector input is preserved only with original settings"
    );
    let mut dec = decoder(&data)?;
    let orientation = dec.orientation()?;
    // Reject animated WebP instead of silently discarding frames.
    if ct == "image/webp" {
        ensure!(
            !image::codecs::webp::WebPDecoder::new(Cursor::new(&data))?.has_animation(),
            "animated WebP cannot be flattened by image processing"
        );
    }
    let mut image = DynamicImage::from_decoder(dec)?;
    image.apply_orientation(orientation);
    let mut scale = 1f64;
    if max_width > 0 {
        scale = scale.min(max_width as f64 / image.width() as f64);
    }
    if recipe.max_edge > 0 {
        scale = scale.min(recipe.max_edge as f64 / image.width().max(image.height()) as f64);
    }
    if scale < 1. {
        image = image.resize_exact(
            (image.width() as f64 * scale).round().max(1.) as u32,
            (image.height() as f64 * scale).round().max(1.) as u32,
            image::imageops::FilterType::CatmullRom,
        );
    }
    if !recipe.watermark.is_empty() {
        let bytes = read_image(Path::new(&recipe.watermark), 20 << 20)?;
        let mut mark = DynamicImage::from_decoder(decoder(&bytes)?)?
            .thumbnail((image.width() / 4).max(1), (image.height() / 4).max(1))
            .to_rgba8();
        for pixel in mark.pixels_mut() {
            pixel.0[3] = (u16::from(pixel.0[3]) * u16::from(recipe.opacity) / 100) as u8;
        }
        let margin = (image.width().min(image.height()) / 50).max(1);
        let x = image.width().saturating_sub(mark.width() + margin);
        let y = image.height().saturating_sub(mark.height() + margin);
        image::imageops::overlay(&mut image, &mark, i64::from(x), i64::from(y));
    }
    let format = if recipe.format == "original" {
        match ct {
            "image/jpeg" => "jpeg",
            "image/webp" => "webp",
            _ => "png",
        }
    } else {
        &recipe.format
    };
    let bytes = match format {
        "jpeg" => {
            // Composite transparency on white, rather than turning transparent pixels black.
            if transparent(&image) {
                let mut background = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                    image.width(),
                    image.height(),
                    image::Rgba([255, 255, 255, 255]),
                ));
                image::imageops::overlay(&mut background, &image, 0, 0);
                image = background;
            }
            encode_jpeg(&image, recipe.quality)?
        }
        "webp" => encode_webp(&image)?,
        _ => {
            let mut out = Cursor::new(vec![]);
            image.write_to(&mut out, image::ImageFormat::Png)?;
            out.into_inner()
        }
    };
    Ok(Processed {
        original_size: data.len() as u64,
        data: bytes,
        content_type: format!("image/{format}"),
    })
}
pub fn process(
    mut data: Vec<u8>,
    content_type: &str,
    strip: bool,
    max_width: u32,
    optimize: bool,
) -> Result<Processed> {
    let original_size = data.len() as u64;
    let mut ct = content_type.to_string();
    // Preserve original animation/vector/new-format bytes; do not flatten GIF/WebP.
    if !matches!(content_type, "image/jpeg" | "image/png")
        || (!strip && max_width == 0 && !optimize)
    {
        return Ok(Processed {
            data,
            content_type: ct,
            original_size: 0,
        });
    }
    if content_type == "image/jpeg" {
        let mut dec = decoder(&data)?;
        let orientation = dec.orientation()?;
        if orientation != Orientation::NoTransforms {
            let mut img = DynamicImage::from_decoder(dec)?;
            img.apply_orientation(orientation);
            data = encode_jpeg(&img, 95)?;
        } else {
            drop(dec);
        }
        if strip {
            data = strip_jpeg(&data)?;
        }
    }
    let mut resized = false;
    if max_width > 0 {
        let dec = decoder(&data)?;
        let (w, h) = dec.dimensions();
        if w > max_width {
            let img = DynamicImage::from_decoder(dec)?;
            let height = ((u64::from(h) * u64::from(max_width)) / u64::from(w)).max(1) as u32;
            let img = img.resize_exact(max_width, height, image::imageops::FilterType::CatmullRom);
            if transparent(&img) {
                data = encode_webp(&img)?;
                ct = "image/webp".into();
            } else {
                data = encode_jpeg(&img, 85)?;
                ct = "image/jpeg".into();
            }
            resized = true;
        }
    }
    if optimize && !resized {
        let img = DynamicImage::from_decoder(decoder(&data)?)?;
        let mut candidate = if content_type == "image/jpeg" {
            encode_jpeg(&img, 85)?
        } else {
            encode_webp(&img)?
        };
        let mut candidate_type = if content_type == "image/jpeg" {
            "image/jpeg"
        } else {
            "image/webp"
        };
        if content_type == "image/png" && !transparent(&img) {
            let jpeg = encode_jpeg(&img, 85)?;
            if jpeg.len() < candidate.len() {
                candidate = jpeg;
                candidate_type = "image/jpeg";
            }
        }
        if candidate.len() < data.len() {
            data = candidate;
            ct = candidate_type.into();
        }
    }
    Ok(Processed {
        original_size: if data.len() as u64 != original_size || ct != content_type || resized {
            original_size
        } else {
            0
        },
        data,
        content_type: ct,
    })
}
#[derive(Default, Serialize)]
pub struct ImageInfo {
    pub path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_type: String,
    #[serde(skip_serializing_if = "zero32")]
    pub width: u32,
    #[serde(skip_serializing_if = "zero32")]
    pub height: u32,
    pub size: u64,
    #[serde(skip_serializing_if = "is_false")]
    pub has_exif: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}
fn zero32(n: &u32) -> bool {
    *n == 0
}
fn is_false(b: &bool) -> bool {
    !*b
}
pub fn info(path: &Path) -> ImageInfo {
    let mut out = ImageInfo {
        path: path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let read = (|| -> Result<()> {
        let mut f = std::fs::File::open(path)?;
        let st = f.metadata()?;
        ensure!(st.is_file(), "not a regular file");
        out.size = st.len();
        let mut head = Vec::new();
        (&mut f).take(65536).read_to_end(&mut head)?;
        out.content_type = detect(&head)?.into();
        if let Ok((w, h)) = ImageReader::open(path)?
            .with_guessed_format()?
            .into_dimensions()
        {
            out.width = w;
            out.height = h;
        }
        if out.content_type == "image/jpeg" {
            out.has_exif = head.windows(2).any(|b| b == b"\xff\xe1");
        }
        Ok(())
    })();
    if let Err(e) = read {
        out.error = e.to_string();
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipe_limits_portrait_longest_edge_and_preserves_input() {
        let image = DynamicImage::new_rgba8(50, 200);
        let mut png = Cursor::new(vec![]);
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        let recipe = Recipe {
            format: "png".into(),
            max_edge: 80,
            ..Default::default()
        };
        let result = process_recipe(bytes.clone(), "image/png", false, 0, false, &recipe).unwrap();
        let dec = decoder(&result.data).unwrap();
        assert_eq!(dec.dimensions(), (20, 80));
        assert_eq!(decoder(&bytes).unwrap().dimensions(), (50, 200));
        let jpeg = process_recipe(
            bytes,
            "image/png",
            false,
            0,
            false,
            &Recipe {
                format: "jpeg".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = DynamicImage::from_decoder(decoder(&jpeg.data).unwrap())
            .unwrap()
            .to_rgb8();
        assert!(decoded.get_pixel(0, 0).0.iter().all(|v| *v > 245));
    }
    #[test]
    fn watermark_changes_output_without_modifying_logo() {
        let root = tempfile::tempdir().unwrap();
        let mark = root.path().join("mark.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
            .save(&mark)
            .unwrap();
        let before = std::fs::read(&mark).unwrap();
        let mut original = Cursor::new(vec![]);
        DynamicImage::new_rgba8(80, 80)
            .write_to(&mut original, image::ImageFormat::Png)
            .unwrap();
        let result = process_recipe(
            original.into_inner(),
            "image/png",
            false,
            0,
            false,
            &Recipe {
                format: "png".into(),
                watermark: mark.to_string_lossy().into_owned(),
                opacity: 100,
                ..Default::default()
            },
        )
        .unwrap();
        let pixels = DynamicImage::from_decoder(decoder(&result.data).unwrap())
            .unwrap()
            .to_rgba8();
        assert!(pixels.pixels().any(|p| p[0] == 255 && p[3] == 255));
        assert_eq!(std::fs::read(mark).unwrap(), before);
    }
    fn png(img: &DynamicImage) -> Vec<u8> {
        let mut b = Cursor::new(Vec::new());
        img.write_to(&mut b, image::ImageFormat::Png).unwrap();
        b.into_inner()
    }
    #[test]
    fn detects_bytes_and_rejects_disguised_files() {
        for (b, ct) in [
            (b"GIF89a".as_slice(), "image/gif"),
            (
                b"\xef\xbb\xbf <?xml version='1.0'?>\n<svg xmlns='x'>",
                "image/svg+xml",
            ),
            (b"\0\0\0\x18ftypavif\0\0\0\0", "image/avif"),
        ] {
            assert_eq!(detect(b).unwrap(), ct);
        }
        assert!(detect(b"<svgscript>").is_err());
        assert!(detect(b"not an image.png").is_err());
    }
    #[test]
    fn resize_and_optimization_preserve_transparency_and_animation() {
        let mut img = image::RgbaImage::from_pixel(31, 17, image::Rgba([180, 20, 200, 255]));
        img.put_pixel(3, 4, image::Rgba([1, 2, 3, 17]));
        let data = png(&DynamicImage::ImageRgba8(img));
        let result = process(data.clone(), "image/png", false, 0, true).unwrap();
        assert_ne!(result.content_type, "image/jpeg");
        assert!(
            image::load_from_memory(&result.data)
                .unwrap()
                .to_rgba8()
                .pixels()
                .any(|p| p[3] != 255)
        );
        let result = process(data, "image/png", false, 15, false).unwrap();
        assert_eq!(image::load_from_memory(&result.data).unwrap().width(), 15);
        for ct in ["image/gif", "image/webp", "image/svg+xml", "image/avif"] {
            let data = b"original animation".to_vec();
            assert_eq!(process(data.clone(), ct, true, 1, true).unwrap().data, data);
        }
    }
    #[test]
    fn jpeg_metadata_removed_and_orientation_applied() {
        let jpeg = encode_jpeg(&DynamicImage::new_rgb8(20, 10), 95).unwrap();
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut input = jpeg[..2].to_vec();
        input.extend([0xff, 0xe1]);
        input.extend(((exif.len() + 2) as u16).to_be_bytes());
        input.extend(exif);
        input.extend(&jpeg[2..]);
        let result = process(input, "image/jpeg", true, 0, false).unwrap();
        let decoded = image::load_from_memory(&result.data).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (10, 20));
        assert!(!result.data.windows(6).any(|b| b == b"Exif\0\0"));
        let mut raw = jpeg[..2].to_vec();
        raw.extend(b"\xff\xe1\0\x06abcd");
        raw.extend(&jpeg[2..]);
        assert_eq!(strip_jpeg(&raw).unwrap(), jpeg);
    }
}
