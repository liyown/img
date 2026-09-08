//! Shared image renderer. Previews and exported files use this exact pipeline.
mod annotations;
pub mod batch;
pub mod publish;
mod render;
pub use img_records::processing::*;
pub use render::{EncodedImage, process, stitch};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Control;
    use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
    use std::{
        io::Cursor,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    fn png(image: RgbaImage) -> Vec<u8> {
        let mut bytes = Cursor::new(vec![]);
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    fn decode(output: &EncodedImage) -> RgbaImage {
        image::load_from_memory(&output.data).unwrap().into_rgba8()
    }
    #[test]
    fn transparent_jpeg_background_webp_modes_and_target_dimensions() {
        let bytes = png(RgbaImage::from_pixel(31, 19, Rgba([27, 120, 201, 0])));
        let mut plan = ProcessingPlan::default();
        plan.encoding.format = Format::Jpeg;
        plan.encoding.jpeg_background = [20, 80, 200];
        plan.encoding.compression = Compression::Quality { quality: 100 };
        let jpeg = process(&bytes, &plan, None, &Control::default())
            .unwrap()
            .remove(0);
        let pixel = decode(&jpeg)[(4, 4)];
        for (actual, expected) in pixel.0[..3].iter().zip([20i16, 80, 200]) {
            assert!((i16::from(*actual) - expected).abs() <= 2);
        }
        assert!(!jpeg.has_alpha);
        plan.encoding.format = Format::Webp;
        plan.encoding.compression = Compression::Lossless;
        let webp = process(&bytes, &plan, None, &Control::default())
            .unwrap()
            .remove(0);
        assert!(webp.has_alpha);
        assert_eq!((webp.width, webp.height), (31, 19));
        plan.encoding.compression = Compression::Target { bytes: 1 };
        let target = process(&bytes, &plan, None, &Control::default())
            .unwrap()
            .remove(0);
        assert_eq!(target.target_met, Some(false));
        assert_eq!(target.content_type, "image/webp");
        assert_eq!((target.width, target.height), (31, 19));
        plan.encoding.format = Format::Png;
        let png = process(&bytes, &plan, None, &Control::default())
            .unwrap()
            .remove(0);
        assert_eq!(png.target_met, Some(false));
        assert_eq!(decode(&png)[(0, 0)], Rgba([27, 120, 201, 0]));
    }
    #[test]
    fn splits_keep_remainders_and_row_major_order_without_touching_pixels() {
        let image = RgbaImage::from_fn(5, 7, |x, y| Rgba([x as u8, y as u8, 10, 255]));
        let bytes = png(image.clone());
        let mut plan = ProcessingPlan {
            split: Some(Split::Height { height: 3 }),
            ..Default::default()
        };
        let slices = process(&bytes, &plan, None, &Control::default()).unwrap();
        assert_eq!(
            slices.iter().map(|s| s.height).collect::<Vec<_>>(),
            [3, 3, 1]
        );
        assert_eq!(decode(&slices[2])[(4, 0)], image[(4, 6)]);
        plan.split = Some(Split::Grid {
            rows: 2,
            columns: 2,
        });
        let tiles = process(&bytes, &plan, None, &Control::default()).unwrap();
        assert_eq!(
            tiles
                .iter()
                .map(|tile| (tile.width, tile.height))
                .collect::<Vec<_>>(),
            [(2, 3), (3, 3), (2, 4), (3, 4)]
        );
        let mut restored = RgbaImage::new(5, 7);
        for (tile, (x, y)) in tiles.iter().zip([(0, 0), (2, 0), (0, 3), (2, 3)]) {
            image::imageops::overlay(&mut restored, &decode(tile), x, y);
        }
        assert_eq!(restored, image);
    }
    #[test]
    fn geometry_exif_crop_and_watermark_share_export_coordinates() {
        let mut jpeg = vec![];
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&DynamicImage::new_rgb8(20, 10))
            .unwrap();
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut input = jpeg[..2].to_vec();
        input.extend([0xff, 0xe1]);
        input.extend(((exif.len() + 2) as u16).to_be_bytes());
        input.extend(exif);
        input.extend(&jpeg[2..]);
        let output = process(
            &input,
            &ProcessingPlan::default(),
            None,
            &Control::default(),
        )
        .unwrap()
        .remove(0);
        assert_eq!((output.width, output.height), (10, 20));
        let original = png(RgbaImage::from_pixel(20, 10, Rgba([255, 255, 255, 255])));
        let mark = png(RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])));
        let mut plan = ProcessingPlan::default();
        plan.geometry.crop = Some(Rect {
            x: 2,
            y: 1,
            width: 10,
            height: 6,
        });
        plan.geometry.quarter_turns = 1;
        plan.geometry.flip_horizontal = true;
        plan.watermark = Some(Watermark {
            position: Position::TopLeft,
            margin: 0,
            scale: 0.5,
            opacity: 1.,
            ..Default::default()
        });
        let output = process(&original, &plan, Some(&mark), &Control::default())
            .unwrap()
            .remove(0);
        assert_eq!((output.width, output.height), (6, 10));
        assert_eq!(decode(&output)[(0, 0)], Rgba([255, 0, 0, 255]));
    }
    #[test]
    fn annotations_render_chinese_and_redaction_is_opaque_without_damaging_other_alpha() {
        let original = RgbaImage::from_pixel(300, 270, Rgba([27, 120, 201, 1]));
        let bytes = png(original.clone());
        let plan = ProcessingPlan {
            annotations: vec![
                Annotation {
                    id: "text".into(),
                    shape: Shape::Text {
                        at: Point { x: 4., y: 10. },
                        text: "中文 img".into(),
                        size: 42.,
                    },
                    color: [0, 0, 0, 255],
                    stroke_width: 3.,
                },
                Annotation {
                    id: "cover".into(),
                    shape: Shape::Redact {
                        from: Point { x: 20., y: 120. },
                        to: Point { x: 60., y: 150. },
                    },
                    color: [50, 60, 70, 10],
                    stroke_width: 3.,
                },
                Annotation {
                    id: "step".into(),
                    shape: Shape::Step {
                        at: Point { x: 110., y: 130. },
                        number: 2,
                        radius: 15.,
                    },
                    color: [255, 0, 0, 255],
                    stroke_width: 3.,
                },
                Annotation {
                    id: "arrow".into(),
                    shape: Shape::Arrow {
                        from: Point { x: 200., y: 30. },
                        to: Point { x: 240., y: 60. },
                    },
                    color: [255, 0, 0, 255],
                    stroke_width: 3.,
                },
            ],
            ..Default::default()
        };
        let output = process(&bytes, &plan, None, &Control::default())
            .unwrap()
            .remove(0);
        let image = decode(&output);
        assert_eq!(image[(299, 269)], original[(299, 269)]);
        assert_eq!(image[(30, 125)], Rgba([50, 60, 70, 255]));
        assert_eq!(image[(30, 140)], Rgba([50, 60, 70, 255]));
        assert!(
            image::imageops::crop_imm(&image, 4, 10, 100, 50)
                .pixels()
                .any(|(_, _, p)| p[3] > 100)
        );
    }
    #[test]
    fn stitch_preserves_order_does_not_upscale_and_refuses_oversize_before_canvas() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.png");
        let b = root.path().join("b.png");
        std::fs::write(&a, png(RgbaImage::from_pixel(4, 2, Rgba([255, 0, 0, 255])))).unwrap();
        std::fs::write(&b, png(RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255])))).unwrap();
        let plan = ProcessingPlan {
            stitch: Some(Stitch {
                cross_size: Some(4),
                spacing: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let output = stitch(
            &[a.clone(), b.clone()],
            &plan,
            None,
            1 << 20,
            &Control::default(),
        )
        .unwrap()
        .remove(0);
        assert_eq!((output.width, output.height), (4, 5));
        let image = decode(&output);
        assert_eq!(image[(0, 0)], Rgba([255, 0, 0, 255]));
        assert_eq!(image[(0, 3)], Rgba([255; 4]));
        assert_eq!(image[(1, 3)], Rgba([0, 0, 255, 255]));
        std::fs::write(&a, png(RgbaImage::new(1, 20000))).unwrap();
        std::fs::write(&b, png(RgbaImage::new(1, 20000))).unwrap();
        assert!(
            stitch(&[a, b], &plan, None, 1 << 20, &Control::default())
                .unwrap_err()
                .to_string()
                .contains("edge")
        );
    }
    #[test]
    fn animation_is_rejected_and_cancellation_produces_no_exports() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("animated.gif");
        let bytes = b"GIF89a\x01\0\x01\0\0\0\0";
        std::fs::write(&original, bytes).unwrap();
        assert!(process(bytes, &ProcessingPlan::default(), None, &Control::default()).is_err());
        assert_eq!(std::fs::read(original).unwrap(), bytes);
        let input = png(RgbaImage::new(10, 10));
        let control = Control::default();
        control.cancel();
        assert!(process(&input, &ProcessingPlan::default(), None, &control).is_err());
    }
    #[test]
    fn hundred_image_batch_cancel_retry_and_changed_original_are_preserved() {
        use batch::{Input, ProcessingTask};
        let root = tempfile::tempdir().unwrap();
        let c = img_records::catalog::Catalog::open(&root.path().join("data")).unwrap();
        let bytes = png(RgbaImage::new(20, 20));
        let mut inputs = vec![];
        for i in 0..100 {
            let path = root.path().join(format!("{i}.png"));
            std::fs::write(&path, &bytes).unwrap();
            inputs.push(Input::snapshot(&path, 1 << 20).unwrap());
        }
        let first = inputs[0].path.clone();
        let mut task = ProcessingTask::create(
            inputs,
            ProcessingPlan::default(),
            None,
            &root.path().join("out"),
            1 << 20,
        )
        .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let control = Control::default();
        let stop = control.clone();
        let control = control.with_reporter(move |_| {
            if count.fetch_add(1, Ordering::SeqCst) >= 4 {
                stop.cancel();
            }
        });
        batch::run(&c, &mut task, &control).unwrap();
        assert!(task.files.iter().any(|r| r.status == "cancelled"));
        assert!(std::fs::read_dir(&task.output_dir).unwrap().count() < 100);
        batch::run(&c, &mut task, &Control::default()).unwrap();
        assert_eq!(task.files.len(), 100);
        assert!(task.files.iter().all(|r| r.status == "complete"));
        assert_eq!(std::fs::read_dir(&task.output_dir).unwrap().count(), 100);
        assert_eq!(std::fs::read(&first).unwrap(), bytes);
        let changed_output = task.files[0].outputs[0].path.clone();
        std::fs::write(&changed_output, b"edited by user").unwrap();
        batch::run(&c, &mut task, &Control::default()).unwrap();
        assert_eq!(task.files[0].status, "failed");
        assert_eq!(std::fs::read(changed_output).unwrap(), b"edited by user");
        let mut second = ProcessingTask::create(
            vec![Input::snapshot(&first, 1 << 20).unwrap()],
            ProcessingPlan::default(),
            None,
            &root.path().join("other"),
            1 << 20,
        )
        .unwrap();
        std::fs::write(&first, png(RgbaImage::new(21, 21))).unwrap();
        batch::run(&c, &mut second, &Control::default()).unwrap();
        assert_eq!(second.files[0].status, "failed");
        assert!(second.files[0].outputs.is_empty());
        assert!(c.sync_events(None).unwrap().is_empty());
    }
}
