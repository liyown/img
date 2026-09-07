use crate::{theme::*, upload_options::UploadOptions};
use gpui_kit::{
    component::{
        button::*,
        input::{Input, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        switch::Switch,
        *,
    },
    prelude::*,
    *,
};
use std::{collections::BTreeMap, path::PathBuf};

pub struct UploadOptionsChanged(pub UploadOptions);
pub struct UploadSettings {
    engine: PathBuf,
    preview: Option<tempfile::TempDir>,
    root: PathBuf,
    options: UploadOptions,
    fields: BTreeMap<&'static str, Entity<InputState>>,
    notice: Option<(String, bool)>,
}
impl EventEmitter<UploadOptionsChanged> for UploadSettings {}
const FIELDS: [(&str, &str, &str); 10] = [
    ("path", "上传目录", "留空上传到根目录"),
    ("path_template", "路径模板", "{year}/{month}/{filename}"),
    ("max_width", "最大宽度（像素）", "0 表示保持原尺寸"),
    ("max_size_mb", "单张大小上限（MB）", "1–128"),
    ("concurrency", "同时上传数量", "1–6"),
    ("retry_count", "自动重试次数", "0–5"),
    ("max_edge", "最长边（像素）", "0 表示保持原尺寸"),
    ("quality", "JPEG 质量", "1–100；WebP 使用无损编码"),
    ("watermark", "图片水印", "水印图片的绝对路径，留空关闭"),
    ("watermark_opacity", "水印透明度（%）", "0–100，默认 60"),
];
fn text(s: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .text_color(crate::theme::color(color))
        .child(crate::i18n::text(s))
}
impl UploadSettings {
    pub fn new(
        root: PathBuf,
        engine: PathBuf,
        options: UploadOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let values = [
            &options.path,
            &options.path_template,
            &options.max_width.to_string(),
            &options.max_size_mb.to_string(),
            &options.concurrency.to_string(),
            &options.retry_count.to_string(),
            &options.max_edge.to_string(),
            &options.quality.to_string(),
            &options.watermark,
            &options.watermark_opacity.to_string(),
        ]
        .map(|v| v.to_owned());
        let fields = FIELDS
            .iter()
            .zip(values)
            .map(|((key, _, placeholder), value)| {
                (
                    *key,
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(crate::i18n::text(*placeholder))
                            .default_value(value)
                    }),
                )
            })
            .collect();
        Self {
            engine,
            preview: None,
            root,
            options,
            fields,
            notice: None,
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let mut options = self.options.clone();
        let result = (|| -> anyhow::Result<()> {
            let value = |key| self.fields[key].read(cx).value().trim().to_string();
            options.path = value("path");
            options.path_template = value("path_template");
            options.max_width = value("max_width")
                .parse()
                .map_err(|_| anyhow::anyhow!("最大宽度需要填写整数"))?;
            options.max_size_mb = value("max_size_mb")
                .parse()
                .map_err(|_| anyhow::anyhow!("大小上限需要填写整数"))?;
            options.concurrency = value("concurrency")
                .parse()
                .map_err(|_| anyhow::anyhow!("同时上传数量需要填写整数"))?;
            options.retry_count = value("retry_count")
                .parse()
                .map_err(|_| anyhow::anyhow!("重试次数需要填写整数"))?;
            options.max_edge = value("max_edge")
                .parse()
                .map_err(|_| anyhow::anyhow!("最长边需要填写整数"))?;
            options.quality = value("quality")
                .parse()
                .map_err(|_| anyhow::anyhow!("JPEG 质量需要填写整数"))?;
            options.watermark = value("watermark");
            anyhow::ensure!(
                options.watermark.is_empty()
                    || std::path::Path::new(&options.watermark).is_absolute(),
                "水印图片需要填写绝对路径"
            );
            options.watermark_opacity = value("watermark_opacity")
                .parse()
                .map_err(|_| anyhow::anyhow!("水印透明度需要填写整数"))?;
            options.save(&self.root)
        })();
        match result {
            Ok(()) => {
                self.options = options.clone();
                cx.emit(UploadOptionsChanged(options));
                self.notice = Some(("上传设置已保存，下一个批次生效".into(), false));
            }
            Err(e) => self.notice = Some((e.to_string(), true)),
        }
        cx.notify();
    }
    fn preview_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
        if self.notice.as_ref().is_some_and(|(_, error)| *error) {
            return;
        }
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::i18n::text("选择要预览的图片")),
        });
        let options = self.options.clone();
        let binary = self.engine.clone();
        let root = self.root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(source) = paths.into_iter().next() else {
                return;
            };
            let task = cx
                .background_executor()
                .spawn(async move {
                    let temp = tempfile::tempdir_in(root)?;
                    let config = temp.path().join("config.toml");
                    std::fs::write(&config, options.engine_config("version=1")?)?;
                    let output = temp.path().join("preview.image");
                    let mut command = std::process::Command::new(binary);
                    command
                        .arg("--config")
                        .arg(config)
                        .arg("process")
                        .arg(&source)
                        .arg("--output")
                        .arg(&output);
                    if options.optimize {
                        command.arg("--optimize");
                    }
                    let result = crate::engine::run(command, &crate::engine::Control::default())?;
                    anyhow::ensure!(
                        result.success,
                        "图片处理预览失败，请检查图片格式、水印路径与尺寸"
                    );
                    let data: serde_json::Value = serde_json::from_slice(&result.stdout)?;
                    Ok::<_, anyhow::Error>((temp, source, output, data))
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match task {
                    Ok((temp, source, output, data)) => {
                        this.preview = Some(temp);
                        let summary = format!(
                            "{} → {} 字节 · {} × {} · {}",
                            data["original_size"],
                            data["size"],
                            data["info"]["width"],
                            data["info"]["height"],
                            data["content_type"].as_str().unwrap_or("")
                        );
                        window.open_dialog(cx, move |dialog, window, _| {
                            let width =
                                (f32::from(window.viewport_size().width) - 80.).clamp(300., 900.);
                            dialog
                                .title(crate::i18n::text("图片处理预览"))
                                .w(px(width))
                                .child(text(summary.clone(), 12., MUTED))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(12.))
                                        .h(px(320.))
                                        .child(
                                            div().flex_1().min_w(px(0.)).h_full().child(
                                                img(source.clone())
                                                    .size_full()
                                                    .object_fit(ObjectFit::Contain),
                                            ),
                                        )
                                        .child(
                                            div().flex_1().min_w(px(0.)).h_full().child(
                                                img(output.clone())
                                                    .size_full()
                                                    .object_fit(ObjectFit::Contain),
                                            ),
                                        ),
                                )
                                .child(text(
                                    "左侧原图，右侧处理结果。原文件保持不变；此操作不上传。",
                                    12.,
                                    MUTED,
                                ))
                        });
                    }
                    Err(error) => this.notice = Some((error.to_string(), true)),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for UploadSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut grid = div().grid().grid_cols(2).gap(px(14.));
        for (key, title, _) in FIELDS {
            grid = grid.child(
                div()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(text(title, 12., NAV_TEXT))
                    .child(
                        Input::new(&self.fields[key])
                            .aria_label(crate::i18n::text(title))
                            .h(px(36.))
                            .text_size(px(12.))
                            .bg(crate::theme::color(CANVAS)),
                    ),
            );
        }
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(text("上传与图片处理", 16., TEXT).font_weight(FontWeight::SEMIBOLD))
            .child(text(
                "处理上传副本，保留原图。压缩和缩放用于 PNG / JPEG；EXIF 清理用于 JPEG。",
                12.,
                MUTED,
            ))
            .child(grid);
        let selected_format = self.options.image_format.clone();
        let weak = cx.entity().downgrade();
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(text("图片输出格式", 13., TEXT))
                .child(
                    Button::new("image-format")
                        .label(crate::i18n::text(selected_format.clone()))
                        .small()
                        .dropdown_menu(move |mut menu, _, _| {
                            for format in ["original", "png", "jpeg", "webp"] {
                                let weak = weak.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(crate::i18n::text(format))
                                        .checked(selected_format == format)
                                        .on_click(move |_, _, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.options.image_format = format.into();
                                                cx.notify();
                                            });
                                        }),
                                );
                            }
                            menu
                        }),
                ),
        );
        let mut presets = div().flex().flex_wrap().gap(px(8.));
        for (key, title, format, edge, quality) in [
            ("original", "原始尺寸", "original", 0, 85),
            ("web", "网页配图", "webp", 1600, 85),
            ("photo", "照片", "jpeg", 2400, 82),
        ] {
            presets = presets.child(
                Button::new(key)
                    .label(crate::i18n::text(title))
                    .small()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.options.image_format = format.into();
                        this.fields["max_edge"].update(cx, |field, cx| {
                            field.set_value(edge.to_string(), window, cx)
                        });
                        this.fields["quality"].update(cx, |field, cx| {
                            field.set_value(quality.to_string(), window, cx)
                        });
                        this.fields["max_width"]
                            .update(cx, |field, cx| field.set_value("0", window, cx));
                        cx.notify();
                    })),
            );
        }
        body = body.child(presets).child(
            Button::new("preview-processing")
                .label(crate::i18n::text("保存并预览图片"))
                .small()
                .on_click(cx.listener(|this, _, window, cx| this.preview_image(window, cx))),
        );
        let selected = self.options.rename.clone();
        let weak = cx.entity().downgrade();
        let name = match selected.as_str() {
            "timestamp" => "时间戳",
            "hash" => "内容哈希",
            "uuid" => "随机名称",
            _ => "保留文件名",
        };
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(text("文件命名", 13., TEXT))
                .child(
                    Button::new("upload-rename")
                        .label(crate::i18n::text(name))
                        .small()
                        .dropdown_menu(move |mut menu, _, _| {
                            for (key, label) in [
                                ("original", "保留文件名"),
                                ("timestamp", "时间戳"),
                                ("hash", "内容哈希"),
                                ("uuid", "随机名称"),
                            ] {
                                let weak = weak.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(crate::i18n::text(label))
                                        .checked(selected == key)
                                        .on_click(move |_, _, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.options.rename = key.into();
                                                cx.notify();
                                            });
                                        }),
                                );
                            }
                            menu
                        }),
                ),
        );
        for (key, title, checked) in [
            (
                "reuse",
                "重复图片复用已有链接（远端删除后请关闭）",
                self.options.reuse,
            ),
            (
                "optimize",
                "压缩图片（可能转换为 JPEG / WebP）",
                self.options.optimize,
            ),
            (
                "strip_exif",
                "移除 JPEG 的 EXIF / GPS 信息",
                self.options.strip_exif,
            ),
            ("overwrite", "同名文件覆盖远端图片", self.options.overwrite),
            (
                "http_sources",
                "允许从 HTTP 链接导入图片",
                self.options.allow_http_sources,
            ),
        ] {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(text(title, 13., TEXT))
                    .child(
                        Switch::new(key)
                            .accessibility_label(crate::i18n::text(title))
                            .checked(checked)
                            .color(crate::theme::color(NAV_ACTIVE))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                match key {
                                    "optimize" => this.options.optimize = *checked,
                                    "reuse" => this.options.reuse = *checked,
                                    "strip_exif" => this.options.strip_exif = *checked,
                                    "overwrite" => this.options.overwrite = *checked,
                                    _ => this.options.allow_http_sources = *checked,
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        body = body.child(text("模板变量：{year} {month} {day} {filename} {stem} {ext} {hash} {uuid} {timestamp} {unix}", 11., MUTED))
            .child(div().flex().justify_end().child(Button::new("save-upload-options").label(crate::i18n::text("保存上传设置")).primary().small().on_click(cx.listener(|this, _, _, cx| this.save(cx)))));
        if let Some((notice, error)) = &self.notice {
            body = body.child(text(notice.clone(), 12., if *error { RED } else { GREEN }));
        }
        body
    }
}
