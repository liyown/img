use super::*;
use ab_glyph::{Font, FontRef, ScaleFont, point};
use anyhow::{Result, ensure};
use image::RgbaImage;
use std::sync::LazyLock;
use tiny_skia::{Paint, PathBuilder, PixmapMut, Stroke, Transform};
static FONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::try_from_slice(include_bytes!("../../assets/NotoSansCJKsc-Regular.otf"))
        .expect("bundled Noto font")
});
pub fn draw(
    image: &mut RgbaImage,
    annotations: &[Annotation],
    control: &crate::control::Control,
) -> Result<()> {
    if annotations.is_empty() {
        return Ok(());
    }
    let (w, h) = image.dimensions();
    // Draw short transparent strips, leaving unannotated pixels byte-for-byte unchanged.
    // This bounds the extra raster allocation to 16 MiB even at the maximum image width.
    for top in (0..h).step_by(128) {
        let rows = 128.min(h - top);
        let mut buffer = vec![0u8; w as usize * rows as usize * 4];
        let mut canvas = PixmapMut::from_bytes(&mut buffer, w, rows).expect("RGBA strip size");
        let transform = Transform::from_translate(0., -(top as f32));
        for annotation in annotations {
            control.check()?;
            let mut paint = Paint {
                anti_alias: true,
                ..Default::default()
            };
            let [r, g, b, a] = annotation.color;
            paint.set_color_rgba8(r, g, b, a);
            let stroke = Stroke {
                width: annotation.stroke_width,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..Default::default()
            };
            match &annotation.shape {
                Shape::Arrow { from, to } => {
                    let dx = to.x - from.x;
                    let dy = to.y - from.y;
                    let length = dx.hypot(dy);
                    if length < 0.5 {
                        continue;
                    }
                    let head = (annotation.stroke_width * 4.).max(12.).min(length * 0.45);
                    let ux = dx / length;
                    let uy = dy / length;
                    let mut path = PathBuilder::new();
                    path.move_to(from.x, from.y);
                    path.line_to(to.x, to.y);
                    path.move_to(
                        to.x - ux * head - uy * head * 0.5,
                        to.y - uy * head + ux * head * 0.5,
                    );
                    path.line_to(to.x, to.y);
                    path.line_to(
                        to.x - ux * head + uy * head * 0.5,
                        to.y - uy * head - ux * head * 0.5,
                    );
                    if let Some(path) = path.finish() {
                        canvas.stroke_path(&path, &paint, &stroke, transform, None);
                    }
                }
                Shape::Rectangle { from, to } | Shape::Redact { from, to } => {
                    let solid = matches!(annotation.shape, Shape::Redact { .. });
                    let (mut x, mut y) = (from.x.min(to.x), from.y.min(to.y));
                    let (mut right, mut bottom) = (from.x.max(to.x), from.y.max(to.y));
                    if solid {
                        x = x.floor();
                        y = y.floor();
                        right = right.ceil();
                        bottom = bottom.ceil();
                        paint.set_color_rgba8(r, g, b, 255);
                        paint.anti_alias = false;
                    }
                    if let Some(rect) = tiny_skia::Rect::from_ltrb(x, y, right, bottom) {
                        if solid {
                            canvas.fill_rect(rect, &paint, transform, None);
                        } else {
                            canvas.stroke_path(
                                &PathBuilder::from_rect(rect),
                                &paint,
                                &stroke,
                                transform,
                                None,
                            );
                        }
                    }
                }
                Shape::Text { at, text, size } => text_on(
                    &mut canvas,
                    Point {
                        x: at.x,
                        y: at.y - top as f32,
                    },
                    text,
                    *size,
                    annotation.color,
                )?,
                Shape::Step { at, number, radius } => {
                    if let Some(circle) = PathBuilder::from_circle(at.x, at.y, *radius) {
                        canvas.fill_path(
                            &circle,
                            &paint,
                            tiny_skia::FillRule::Winding,
                            transform,
                            None,
                        );
                    }
                    let text = number.to_string();
                    let size = radius * 1.2;
                    let scaled = FONT.as_scaled(size);
                    let width = text
                        .chars()
                        .map(|ch| scaled.h_advance(FONT.glyph_id(ch)))
                        .sum::<f32>();
                    text_on(
                        &mut canvas,
                        Point {
                            x: at.x - width / 2.,
                            y: at.y - size / 2. - top as f32,
                        },
                        &text,
                        size,
                        [255; 4],
                    )?;
                }
            }
        }

        for (i, source) in canvas.data_mut().as_chunks::<4>().0.iter().enumerate() {
            let alpha = u32::from(source[3]);
            if alpha == 0 {
                continue;
            }
            let target = image.get_pixel_mut(i as u32 % w, top + i as u32 / w);
            let dst_alpha = u32::from(target[3]);
            let denominator = alpha * 255 + dst_alpha * (255 - alpha);
            for c in 0..3 {
                target[c] = ((u32::from(source[c]) * 65025
                    + u32::from(target[c]) * dst_alpha * (255 - alpha)
                    + denominator / 2)
                    / denominator)
                    .min(255) as u8;
            }
            target[3] = ((denominator + 127) / 255).min(255) as u8;
        }
    }
    Ok(())
}
fn text_on(
    canvas: &mut PixmapMut<'_>,
    origin: Point,
    text: &str,
    size: f32,
    color: [u8; 4],
) -> Result<()> {
    let scaled = FONT.as_scaled(size);
    let (width, height) = (canvas.width(), canvas.height());
    let mut cursor = point(origin.x, origin.y + scaled.ascent());
    let mut previous = None;
    for ch in text.chars() {
        if ch == '\n' {
            cursor.x = origin.x;
            cursor.y += scaled.height() + scaled.line_gap();
            previous = None;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        let id = FONT.glyph_id(ch);
        ensure!(
            id.0 != 0,
            "annotation contains a character unavailable in the bundled font"
        );
        if let Some(previous) = previous {
            cursor.x += scaled.kern(previous, id);
        }
        let glyph = id.with_scale_and_position(size, cursor);
        if let Some(outline) = FONT.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            if bounds.max.y <= 0.
                || bounds.min.y >= height as f32
                || bounds.max.x <= 0.
                || bounds.min.x >= width as f32
            {
                cursor.x += scaled.h_advance(id);
                previous = Some(id);
                continue;
            }
            outline.draw(|x, y, coverage| {
                let x = bounds.min.x as i64 + i64::from(x);
                let y = bounds.min.y as i64 + i64::from(y);
                if x < 0 || y < 0 || x >= i64::from(width) || y >= i64::from(height) {
                    return;
                }
                let index = ((y as u32 * width + x as u32) * 4) as usize;
                let target = &mut canvas.data_mut()[index..index + 4];
                let alpha = (f32::from(color[3]) * coverage).round() as u32;
                for c in 0..3 {
                    target[c] =
                        ((u32::from(color[c]) * alpha + u32::from(target[c]) * (255 - alpha) + 127)
                            / 255)
                            .min(255) as u8;
                }
                target[3] =
                    (alpha + (u32::from(target[3]) * (255 - alpha) + 127) / 255).min(255) as u8;
            });
        }
        cursor.x += scaled.h_advance(id);
        previous = Some(id);
    }
    Ok(())
}
