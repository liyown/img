use super::*;
use crate::{control::Control, media};
use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageDecoder, ImageEncoder, RgbaImage, metadata::Orientation};
use serde::{Deserialize, Serialize};
use std::{io::Cursor, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncodedImage {
    pub output_order: usize,
    pub width: u32,
    pub height: u32,
    pub content_type: String,
    pub size: u64,
    pub original_size: u64,
    pub saving_percent: f64,
    pub has_alpha: bool,
    pub target_met: Option<bool>,
    pub quality: Option<u8>,
    pub content_hash: String,
    #[serde(skip)]
    pub data: Vec<u8>,
}
fn decode(data: &[u8]) -> Result<DynamicImage> {
    let ct = media::detect(data)?;
    ensure!(
        matches!(ct, "image/png" | "image/jpeg" | "image/webp"),
        "editing supports PNG, JPEG and static WebP; upload animation and vector formats unchanged"
    );
    if ct == "image/webp" {
        ensure!(
            !image::codecs::webp::WebPDecoder::new(Cursor::new(data))?.has_animation(),
            "animated WebP cannot be flattened by image processing"
        );
    }
    if ct == "image/png" {
        ensure!(
            !image::codecs::png::PngDecoder::new(Cursor::new(data))?.is_apng()?,
            "animated PNG cannot be flattened by image processing"
        );
    }
    let mut decoder = media::decoder(data)?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}
fn resized(width: u32, height: u32, resize: &Resize) -> Result<(u32, u32)> {
    let (w, h) = if !resize.keep_aspect {
        if resize.allow_upscale {
            (resize.width, resize.height)
        } else {
            (resize.width.min(width), resize.height.min(height))
        }
    } else {
        let mut scale = if resize.allow_upscale {
            f64::INFINITY
        } else {
            1.
        };
        if resize.max_edge > 0 {
            scale = scale.min(f64::from(resize.max_edge) / f64::from(width.max(height)));
        }
        if resize.width > 0 {
            scale = scale.min(f64::from(resize.width) / f64::from(width));
        }
        if resize.height > 0 {
            scale = scale.min(f64::from(resize.height) / f64::from(height));
        }
        (
            (f64::from(width) * scale).round().max(1.) as u32,
            (f64::from(height) * scale).round().max(1.) as u32,
        )
    };
    dimensions(w, h)?;
    Ok((w, h))
}
fn geometry(mut image: DynamicImage, geometry: &Geometry) -> Result<DynamicImage> {
    if let Some(resize) = &geometry.resize {
        let (w, h) = resized(image.width(), image.height(), resize)?;
        if (w, h) != (image.width(), image.height()) {
            image = image.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
        }
    }
    if let Some(crop) = &geometry.crop {
        ensure!(
            u64::from(crop.x) + u64::from(crop.width) <= u64::from(image.width())
                && u64::from(crop.y) + u64::from(crop.height) <= u64::from(image.height()),
            "crop extends beyond image bounds"
        );
        image = image.crop_imm(crop.x, crop.y, crop.width, crop.height);
    }
    image = match geometry.quarter_turns {
        1 => image.rotate90(),
        2 => image.rotate180(),
        3 => image.rotate270(),
        _ => image,
    };
    if geometry.flip_horizontal {
        image = image.fliph();
    }
    if geometry.flip_vertical {
        image = image.flipv();
    }
    Ok(image)
}
fn watermark(image: &mut RgbaImage, mark: &Watermark, bytes: Option<&[u8]>) -> Result<()> {
    let bytes = bytes.context("choose the watermark image on this device")?;
    ensure!(
        mark.resource.is_empty() || img_records::catalog::digest(bytes) == mark.resource,
        "watermark image changed; choose it again"
    );
    let source = decode(bytes)?;
    let width = ((image.width() as f32 * mark.scale).round() as u32)
        .max(1)
        .min(image.width());
    let height = ((source.height() as f64 * f64::from(width) / f64::from(source.width())).round()
        as u32)
        .max(1);
    // A tall watermark is fitted inside the image without allocating an oversized intermediate.
    let mut stamp = source
        .resize(
            width,
            height.min(image.height()),
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgba8();
    for pixel in stamp.pixels_mut() {
        pixel[3] = (f32::from(pixel[3]) * mark.opacity).round() as u8;
    }
    let dx = image.width() - stamp.width();
    let dy = image.height() - stamp.height();
    let x = match mark.position {
        Position::TopLeft | Position::Left | Position::BottomLeft => mark.margin.min(dx),
        Position::TopRight | Position::Right | Position::BottomRight => {
            dx.saturating_sub(mark.margin)
        }
        _ => dx / 2,
    };
    let y = match mark.position {
        Position::TopLeft | Position::Top | Position::TopRight => mark.margin.min(dy),
        Position::BottomLeft | Position::Bottom | Position::BottomRight => {
            dy.saturating_sub(mark.margin)
        }
        _ => dy / 2,
    };
    image::imageops::overlay(image, &stamp, i64::from(x), i64::from(y));
    Ok(())
}
fn encode(image: &RgbaImage, encoding: &Encoding, quality: u8) -> Result<Vec<u8>> {
    match encoding.format {
        Format::Png => {
            let mut output = vec![];
            image::codecs::png::PngEncoder::new_with_quality(
                &mut output,
                image::codecs::png::CompressionType::Best,
                image::codecs::png::FilterType::Adaptive,
            )
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgba8,
            )?;
            Ok(output)
        }
        Format::Jpeg => {
            let mut flat = image::RgbImage::new(image.width(), image.height());
            for (source, target) in image.pixels().zip(flat.pixels_mut()) {
                let alpha = u32::from(source[3]);
                for c in 0..3 {
                    target[c] = ((u32::from(source[c]) * alpha
                        + u32::from(encoding.jpeg_background[c]) * (255 - alpha)
                        + 127)
                        / 255) as u8;
                }
            }
            let mut output = vec![];
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality).write_image(
                flat.as_raw(),
                flat.width(),
                flat.height(),
                image::ExtendedColorType::Rgb8,
            )?;
            Ok(output)
        }
        Format::Webp => {
            let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
            let output = encoder
                .encode_simple(
                    matches!(encoding.compression, Compression::Lossless),
                    f32::from(quality),
                )
                .map_err(|error| anyhow::anyhow!("WebP encoding failed: {error:?}"))?;
            Ok(output.to_vec())
        }
    }
}
fn encoded(
    image: RgbaImage,
    encoding: &Encoding,
    order: usize,
    original_size: u64,
    control: &Control,
) -> Result<EncodedImage> {
    control.check()?;
    let mut quality = match encoding.compression {
        Compression::Quality { quality } => quality,
        _ => 100,
    };
    let mut data = encode(&image, encoding, quality)?;
    let target = if let Compression::Target { bytes } = encoding.compression {
        Some(bytes)
    } else {
        None
    };
    if let Some(target) = target
        && encoding.format != Format::Png
        && data.len() as u64 > target
    {
        let smallest = encode(&image, encoding, 1)?;
        quality = 1;
        data = smallest;
        if data.len() as u64 <= target {
            let (mut low, mut high) = (2u8, 99u8);
            while low <= high {
                control.check()?;
                let mid = low + (high - low) / 2;
                let candidate = encode(&image, encoding, mid)?;
                if candidate.len() as u64 <= target {
                    quality = mid;
                    data = candidate;
                    low = mid + 1;
                } else {
                    high = mid - 1;
                }
            }
        }
    }
    let size = data.len() as u64;
    Ok(EncodedImage {
        output_order: order,
        width: image.width(),
        height: image.height(),
        content_type: encoding.format.content_type().into(),
        size,
        original_size,
        saving_percent: if original_size > 0 {
            (1. - size as f64 / original_size as f64) * 100.
        } else {
            0.
        },
        has_alpha: encoding.format != Format::Jpeg && image.pixels().any(|p| p[3] != 255),
        target_met: target.map(|target| size <= target),
        quality: if encoding.format == Format::Png
            || matches!(encoding.compression, Compression::Lossless)
        {
            None
        } else {
            Some(quality)
        },
        content_hash: img_records::catalog::digest(&data),
        data,
    })
}
fn finish(
    image: DynamicImage,
    plan: &ProcessingPlan,
    mark: Option<&[u8]>,
    original: u64,
    control: &Control,
) -> Result<Vec<EncodedImage>> {
    control.check()?;
    let mut image = geometry(image, &plan.geometry)?.into_rgba8();
    annotations::draw(&mut image, &plan.annotations, control)?;
    if let Some(watermark_plan) = &plan.watermark {
        watermark(&mut image, watermark_plan, mark)?;
    }
    control.check()?;
    let (w, h) = image.dimensions();
    let mut tiles = vec![];
    match plan.split {
        None => tiles.push(Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        }),
        Some(Split::Height { height }) => {
            ensure!(
                h.div_ceil(height) <= 1024,
                "split exceeds 1024 output files"
            );
            for y in (0..h).step_by(height as usize) {
                tiles.push(Rect {
                    x: 0,
                    y,
                    width: w,
                    height: height.min(h - y),
                });
            }
        }
        Some(Split::Grid { rows, columns }) => {
            ensure!(
                rows <= h && columns <= w,
                "grid would produce an empty tile"
            );
            for row in 0..rows {
                for col in 0..columns {
                    let x = w * col / columns;
                    let y = h * row / rows;
                    tiles.push(Rect {
                        x,
                        y,
                        width: w * (col + 1) / columns - x,
                        height: h * (row + 1) / rows - y,
                    });
                }
            }
        }
    }
    if tiles.len() == 1 {
        return Ok(vec![encoded(image, &plan.encoding, 0, original, control)?]);
    }
    let mut outputs = Vec::with_capacity(tiles.len());
    for (order, tile) in tiles.iter().enumerate() {
        control.check()?;
        let part =
            image::imageops::crop_imm(&image, tile.x, tile.y, tile.width, tile.height).to_image();
        outputs.push(encoded(part, &plan.encoding, order, original, control)?);
    }
    Ok(outputs)
}
pub fn process(
    bytes: &[u8],
    plan: &ProcessingPlan,
    watermark: Option<&[u8]>,
    control: &Control,
) -> Result<Vec<EncodedImage>> {
    plan.validate()?;
    ensure!(
        plan.stitch.is_none(),
        "stitching requires the ordered multi-image entry point"
    );
    control.check()?;
    finish(decode(bytes)?, plan, watermark, bytes.len() as u64, control)
}
/// Inspect dimensions of every input and the combined output before allocating the canvas.
/// Decode one input at a time; the ordered inputs are never all decoded together.
pub fn stitch(
    files: &[PathBuf],
    plan: &ProcessingPlan,
    watermark: Option<&[u8]>,
    limit: u64,
    control: &Control,
) -> Result<Vec<EncodedImage>> {
    stitch_expected(files, plan, watermark, limit, control, None)
}
pub(super) fn stitch_expected(
    files: &[PathBuf],
    plan: &ProcessingPlan,
    watermark: Option<&[u8]>,
    limit: u64,
    control: &Control,
    expected: Option<&[String]>,
) -> Result<Vec<EncodedImage>> {
    plan.validate()?;
    let options = plan.stitch.as_ref().context("stitch parameters missing")?;
    ensure!(
        !files.is_empty() && files.len() <= 1000,
        "stitching requires 1–1000 images"
    );
    let mut sizes = vec![];
    let mut hashes = vec![];
    let mut original = 0u64;
    let (mut across, mut along) = (0u64, 0u64);
    for (index, file) in files.iter().enumerate() {
        control.check()?;
        let bytes = media::read_image(file, limit)?;
        if let Some(expected) = expected {
            ensure!(
                expected.get(index) == Some(&img_records::catalog::digest(&bytes)),
                "stitch input changed; create a new task"
            );
        }
        original = original.saturating_add(bytes.len() as u64);
        let mut decoder = media::decoder(&bytes)?;
        let (mut width, mut height) = decoder.dimensions();
        if matches!(
            decoder.orientation()?,
            Orientation::Rotate90
                | Orientation::Rotate270
                | Orientation::Rotate90FlipH
                | Orientation::Rotate270FlipH
        ) {
            std::mem::swap(&mut width, &mut height);
        }
        let (width, height) = if let Some(size) = options.cross_size {
            resized(
                width,
                height,
                &Resize {
                    width: if options.direction == Direction::Vertical {
                        size
                    } else {
                        0
                    },
                    height: if options.direction == Direction::Horizontal {
                        size
                    } else {
                        0
                    },
                    allow_upscale: options.allow_upscale,
                    ..Default::default()
                },
            )?
        } else {
            (width, height)
        };
        let (a, b) = if options.direction == Direction::Vertical {
            (width, height)
        } else {
            (height, width)
        };
        across = across.max(u64::from(a));
        along = along
            .checked_add(u64::from(b))
            .context("stitch size overflow")?;
        sizes.push((width, height));
        hashes.push(img_records::catalog::digest(&bytes));
    }
    along = along
        .checked_add(u64::from(options.spacing) * (files.len() - 1) as u64)
        .context("stitch spacing overflow")?;
    let (w, h) = if options.direction == Direction::Vertical {
        (across, along)
    } else {
        (along, across)
    };
    dimensions(
        u32::try_from(w).context("stitch width overflow")?,
        u32::try_from(h).context("stitch height overflow")?,
    )?;
    let mut canvas = RgbaImage::from_pixel(w as u32, h as u32, image::Rgba(options.background));
    let mut offset = 0u32;
    for ((file, (width, height)), hash) in files.iter().zip(sizes).zip(hashes) {
        control.check()?;
        let bytes = media::read_image(file, limit)?;
        ensure!(
            img_records::catalog::digest(&bytes) == hash,
            "stitch input changed after size calculation"
        );
        let image =
            decode(&bytes)?.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
        let (x, y) = if options.direction == Direction::Vertical {
            ((w as u32 - width) / 2, offset)
        } else {
            (offset, (h as u32 - height) / 2)
        };
        image::imageops::overlay(&mut canvas, &image, i64::from(x), i64::from(y));
        offset = offset
            .saturating_add(if options.direction == Direction::Vertical {
                height
            } else {
                width
            })
            .saturating_add(options.spacing);
    }
    finish(
        DynamicImage::ImageRgba8(canvas),
        plan,
        watermark,
        original,
        control,
    )
}
