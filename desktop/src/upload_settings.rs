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
    root: PathBuf,
    options: UploadOptions,
    fields: BTreeMap<&'static str, Entity<InputState>>,
    notice: Option<(String, bool)>,
}
impl EventEmitter<UploadOptionsChanged> for UploadSettings {}
const FIELDS: [(&str, &str, &str); 6] = [
    ("path", "上传目录", "留空上传到根目录"),
    ("path_template", "路径模板", "{year}/{month}/{filename}"),
    ("max_width", "最大宽度（像素）", "0 表示保持原尺寸"),
    ("max_size_mb", "单张大小上限（MB）", "1–128"),
    ("concurrency", "同时上传数量", "1–6"),
    ("retry_count", "自动重试次数", "0–5"),
];
fn text(s: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .text_size(px(size))
        .text_color(rgb(color))
        .child(s.into())
}
impl UploadSettings {
    pub fn new(
        root: PathBuf,
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
                            .placeholder(*placeholder)
                            .default_value(value)
                    }),
                )
            })
            .collect();
        Self {
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
                            .aria_label(title)
                            .h(px(36.))
                            .text_size(px(12.))
                            .bg(rgb(CANVAS)),
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
                        .label(name)
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
                                    PopupMenuItem::new(label).checked(selected == key).on_click(
                                        move |_, _, cx| {
                                            let _ = weak.update(cx, |this, cx| {
                                                this.options.rename = key.into();
                                                cx.notify();
                                            });
                                        },
                                    ),
                                );
                            }
                            menu
                        }),
                ),
        );
        for (key, title, checked) in [
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
                            .accessibility_label(title)
                            .checked(checked)
                            .color(rgb(NAV_ACTIVE))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                match key {
                                    "optimize" => this.options.optimize = *checked,
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
            .child(div().flex().justify_end().child(Button::new("save-upload-options").label("保存上传设置").primary().small().on_click(cx.listener(|this, _, _, cx| this.save(cx)))));
        if let Some((notice, error)) = &self.notice {
            body = body.child(text(notice.clone(), 12., if *error { RED } else { GREEN }));
        }
        body
    }
}
