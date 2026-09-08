use super::*;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::component::checkbox::Checkbox;

impl Tools {
    fn field(&self, key: &'static str, title: &str) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(label(title, 12., NAV_TEXT))
            .child(
                Input::new(&self.fields[key])
                    .aria_label(crate::i18n::text(title.to_owned()))
                    .disabled(self.busy)
                    .w_full()
                    .h(px(32.))
                    .text_size(px(14.)),
            )
            .into_any_element()
    }
    fn choose_option(
        &self,
        id: &'static str,
        labels: &[&'static str],
        selected: usize,
        change: impl Fn(&mut Self, usize) + Clone + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let labels = labels.to_vec();
        let weak = cx.weak_entity();
        action(id, "")
            .w_full()
            .h(px(32.))
            .px(px(10.))
            .disabled(self.busy)
            .accessibility_label(crate::i18n::text(labels[selected]))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label(labels[selected], 13., TEXT))
                    .child(div().flex_1())
                    .child(icon("caret-down", 12.).text_color(crate::theme::color(NAV_TEXT))),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for (index, label) in labels.iter().enumerate() {
                    let weak = weak.clone();
                    let change = change.clone();
                    menu = menu.item(
                        PopupMenuItem::new(crate::i18n::text(*label))
                            .checked(index == selected)
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    if !this.busy {
                                        change(this, index);
                                        this.schedule_preview(cx);
                                    }
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }
    fn controls(&self, cx: &Context<Self>) -> AnyElement {
        let section = || div().flex().flex_col().gap(px(10.));
        let mut controls = div().flex().flex_col().gap(px(24.));
        match self.tool {
            Tool::Convert => {
                let mut formats = div().flex().gap(px(6.));
                for (name, format) in [
                    ("PNG", plan::Format::Png),
                    ("JPEG", plan::Format::Jpeg),
                    ("WebP", plan::Format::Webp),
                ] {
                    formats = formats.child(
                        action(name, name)
                            .flex_1()
                            .px(px(6.))
                            .h(px(34.))
                            .selected(self.plan.encoding.format == format)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.plan.encoding.format = format;
                                if format == plan::Format::Jpeg && this.compression_mode == 2 {
                                    this.compression_mode = 0;
                                }
                                this.schedule_preview(cx);
                            })),
                    );
                }
                controls = controls.child(
                    section()
                        .child(label("输出格式", 12., NAV_TEXT))
                        .child(formats),
                );
                let mut compression =
                    section()
                        .child(label("压缩方式", 12., NAV_TEXT))
                        .child(self.choose_option(
                            "tool-compression",
                            match self.plan.encoding.format {
                                plan::Format::Png => &["无损压缩", "目标大小"][..],
                                plan::Format::Jpeg => &["按质量", "目标大小"][..],
                                plan::Format::Webp => &["按质量", "目标大小", "无损压缩"][..],
                            },
                            if self.plan.encoding.format == plan::Format::Png {
                                usize::from(self.compression_mode == 1)
                            } else {
                                self.compression_mode as usize
                            },
                            |this, index| {
                                this.compression_mode =
                                    if this.plan.encoding.format == plan::Format::Png && index == 0
                                    {
                                        2
                                    } else {
                                        index as u8
                                    };
                            },
                            cx,
                        ));
                if self.compression_mode == 1 {
                    compression = compression.child(self.field("target", "目标大小 (KiB)"));
                } else if self.compression_mode == 0
                    && self.plan.encoding.format != plan::Format::Png
                {
                    compression = compression.child(self.field("quality", "质量 (1–100)"));
                }
                compression = compression.child(label(
                    if self.plan.encoding.format == plan::Format::Png {
                        "无损压缩，保留图片细节。"
                    } else if self.compression_mode == 1 {
                        "只调整质量，不改变尺寸。"
                    } else {
                        "质量越高，文件通常越大。"
                    },
                    12.,
                    NAV_TEXT,
                ));
                controls = controls.child(compression);
                if self.plan.encoding.format == plan::Format::Jpeg {
                    controls =
                        controls.child(section().child(self.field("jpeg_bg", "透明区域填充色")));
                }
            }
            Tool::Geometry => {
                let mut resize =
                    section()
                        .child(label("调整尺寸", 12., NAV_TEXT))
                        .child(self.choose_option(
                            "tool-resize",
                            &["原始尺寸", "按最长边", "指定宽高"],
                            self.resize_mode as usize,
                            |this, index| this.resize_mode = index as u8,
                            cx,
                        ));
                if self.resize_mode == 1 {
                    resize = resize.child(self.field("edge", "最长边 (px)"));
                } else if self.resize_mode == 2 {
                    resize = resize
                        .child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(self.field("width", "宽 (px)"))
                                .child(self.field("height", "高 (px)")),
                        )
                        .child(
                            Checkbox::new("tool-aspect")
                                .label(crate::i18n::text("保持比例"))
                                .checked(
                                    self.plan
                                        .geometry
                                        .resize
                                        .as_ref()
                                        .is_none_or(|r| r.keep_aspect),
                                )
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    this.plan
                                        .geometry
                                        .resize
                                        .get_or_insert_with(Default::default)
                                        .keep_aspect = *checked;
                                    this.schedule_preview(cx);
                                })),
                        )
                        .child(label("宽或高填 0，自动计算另一边。", 12., NAV_TEXT));
                }
                if self.resize_mode > 0 {
                    resize = resize.child(
                        Checkbox::new("tool-upscale")
                            .label(crate::i18n::text("允许放大小图"))
                            .checked(
                                self.plan
                                    .geometry
                                    .resize
                                    .as_ref()
                                    .is_some_and(|r| r.allow_upscale),
                            )
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.plan
                                    .geometry
                                    .resize
                                    .get_or_insert_with(Default::default)
                                    .allow_upscale = *checked;
                                this.schedule_preview(cx);
                            })),
                    );
                }
                controls = controls.child(resize).child(
                    section()
                        .child(label("方向", 12., NAV_TEXT))
                        .child(
                            action("tool-rotate", "旋转 90°")
                                .h(px(32.))
                                .w_full()
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.plan.geometry.quarter_turns =
                                        (this.plan.geometry.quarter_turns + 1) % 4;
                                    this.schedule_preview(cx);
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(
                                    action("tool-flip-h", "水平翻转")
                                        .flex_1()
                                        .px(px(6.))
                                        .h(px(32.))
                                        .selected(self.plan.geometry.flip_horizontal)
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.plan.geometry.flip_horizontal =
                                                !this.plan.geometry.flip_horizontal;
                                            this.schedule_preview(cx);
                                        })),
                                )
                                .child(
                                    action("tool-flip-v", "垂直翻转")
                                        .flex_1()
                                        .px(px(6.))
                                        .h(px(32.))
                                        .selected(self.plan.geometry.flip_vertical)
                                        .disabled(self.busy)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.plan.geometry.flip_vertical =
                                                !this.plan.geometry.flip_vertical;
                                            this.schedule_preview(cx);
                                        })),
                                ),
                        ),
                );
                let mut crop = section()
                    .child(label("裁剪", 12., NAV_TEXT))
                    .child(self.choose_option(
                        "tool-crop-ratio",
                        &["自由比例", "1:1", "4:3", "16:9"],
                        match self.crop_ratio {
                            Some(1.) => 1,
                            Some(v) if v < 1.5 => 2,
                            Some(_) => 3,
                            None => 0,
                        },
                        |this, index| {
                            this.crop_ratio = match index {
                                1 => Some(1.),
                                2 => Some(4. / 3.),
                                3 => Some(16. / 9.),
                                _ => None,
                            }
                        },
                        cx,
                    ))
                    .child(
                        action(
                            "tool-crop",
                            if self.crop_editing {
                                "取消选区"
                            } else {
                                "选择裁剪区域"
                            },
                        )
                        .w_full()
                        .h(px(32.))
                        .disabled(self.busy || self.input.is_none())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.crop_editing = !this.crop_editing;
                            this.drag = None;
                            this.schedule_preview(cx);
                        })),
                    );
                if self.plan.geometry.crop.is_some() {
                    crop = crop.child(
                        action("tool-reset-crop", "移除裁剪")
                            .ghost()
                            .h(px(28.))
                            .w_full()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.plan.geometry.crop = None;
                                this.schedule_preview(cx);
                            })),
                    );
                }
                controls = controls.child(crop.when(self.crop_editing, |view| {
                    view.child(label("在图片上拖动选择区域。", 12., NAV_TEXT))
                }));
            }
        }
        controls.into_any_element()
    }
    fn file_name(&self) -> String {
        self.input.as_ref().map_or_else(
            || crate::i18n::text("图片预览").to_string(),
            |input| {
                if input.source_asset.is_none()
                    && input.path.starts_with(self.root.join("tool-inputs"))
                {
                    crate::i18n::text("剪贴板图片").to_string()
                } else {
                    input
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                }
            },
        )
    }
    fn preview_frame(&self, cx: &Context<Self>) -> AnyElement {
        let mut frame = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(10.))
            .border_1()
            .border_color(crate::theme::color(BORDER))
            .bg(crate::theme::color(CARD))
            .child(
                div()
                    .h(px(42.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(crate::theme::color(BORDER))
                    .child(icon("image", 15.).text_color(crate::theme::color(NAV_TEXT)))
                    .child(
                        label(self.file_name(), 12., NAV_TEXT)
                            .min_w_0()
                            .flex_1()
                            .text_ellipsis(),
                    )
                    .when(self.input.is_some(), |row| {
                        row.child(
                            Button::new("tools-remove-image")
                                .ghost()
                                .size(px(28.))
                                .icon(IconName::Close)
                                .tooltip(crate::i18n::text("移除图片"))
                                .accessibility_label(crate::i18n::text("移除图片"))
                                .disabled(self.busy || self.importing)
                                .on_click(cx.listener(|this, _, _, cx| this.clear_input(cx))),
                        )
                    }),
            );
        frame = if self.input.is_some() {
            frame.child(self.canvas(cx))
        } else {
            frame.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(14.))
                    .child(icon("image", 36.).text_color(crate::theme::color(MUTED)))
                    .child(label("将一张图片拖到这里", 17., TEXT))
                    .child(label("PNG、JPEG、静态 WebP", 12., NAV_TEXT))
                    .child(
                        action("tool-empty-open", "打开图片")
                            .disabled(self.importing || self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                    ),
            )
        };
        let metadata = self
            .preview
            .as_ref()
            .map(|preview| &preview.metadata["image"]);
        let caption = metadata.filter(|_| !self.preview_busy).map_or_else(
            || crate::i18n::text("原始文件始终保留").to_string(),
            |image| {
                format!(
                    "{} × {} px · {}",
                    image["width"],
                    image["height"],
                    crate::i18n::text(if image["has_alpha"] == true {
                        "含透明区域"
                    } else {
                        "不透明"
                    })
                )
            },
        );
        frame
            .child(
                div()
                    .h(px(38.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(crate::theme::color(BORDER))
                    .child(label(caption, 12., NAV_TEXT)),
            )
            .into_any_element()
    }
    fn output_controls(&self, cx: &Context<Self>) -> AnyElement {
        let ready = self.input.is_some()
            && !self.busy
            && !self.importing
            && !self.crop_editing
            && self.error.is_none();
        let image = self
            .preview
            .as_ref()
            .filter(|_| !self.preview_busy && !self.crop_editing && self.error.is_none())
            .map(|p| &p.metadata["image"]);
        let size = image
            .and_then(|i| i["size"].as_u64())
            .map(model::size_label)
            .unwrap_or_else(|| "—".into());
        let detail = image
            .map(|i| {
                let format = i["content_type"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches("image/")
                    .to_uppercase();
                if i["target_met"] == false {
                    format!("{format} · {}", crate::i18n::text("未达到目标大小"))
                } else {
                    let saving = i["saving_percent"].as_f64().unwrap_or(0.);
                    format!(
                        "{format} · {} {:.0}%",
                        crate::i18n::text(if saving >= 0. { "减少" } else { "增加" }),
                        saving.abs()
                    )
                }
            })
            .unwrap_or_else(|| {
                crate::i18n::text(if self.preview_busy {
                    "正在计算…"
                } else if self.crop_editing {
                    "完成选区后可保存"
                } else {
                    "预览完成后显示实际大小"
                })
                .to_string()
            });
        let mut output = div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(14.))
            .border_t_1()
            .border_color(crate::theme::color(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(label("处理后", 12., NAV_TEXT))
                    .child(div().flex_1())
                    .child(label(size, 20., TEXT).font_weight(FontWeight::SEMIBOLD)),
            )
            .child(label(
                detail,
                12.,
                if image.is_some_and(|i| i["target_met"] == false) {
                    RED
                } else {
                    NAV_TEXT
                },
            ))
            .child(
                action(
                    "tools-export",
                    if self.busy {
                        "处理中…"
                    } else {
                        "保存图片"
                    },
                )
                .primary()
                .w_full()
                .h(px(36.))
                .disabled(!ready)
                .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        action("tools-copy-image", "复制图片")
                            .flex_1()
                            .px(px(4.))
                            .h(px(32.))
                            .when(
                                self.copied_until
                                    .is_some_and(|until| until > Instant::now()),
                                |button| button.icon(IconName::Check),
                            )
                            .disabled(!ready || self.preview.is_none() || self.preview_busy)
                            .on_click(cx.listener(|this, _, _, cx| this.copy_image(cx))),
                    )
                    .child(
                        action("tools-upload-results", "上传图片")
                            .flex_1()
                            .px(px(4.))
                            .h(px(32.))
                            .disabled(!ready)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.publish_current(window, cx)),
                            ),
                    ),
            )
            .when(self.busy, |view| {
                view.child(
                    action("tools-cancel-task", "取消")
                        .ghost()
                        .h(px(28.))
                        .w_full()
                        .on_click(cx.listener(|this, _, _, _| {
                            if let Some(control) = &this.control {
                                control.stop(engine::CANCEL);
                            }
                        })),
                )
            });
        if self.saved_to_user
            && let Some(path) = self
                .result
                .as_ref()
                .and_then(|r| r["path"].as_str())
                .map(PathBuf::from)
        {
            output = output.child(
                action("tools-show-saved", "显示已保存的文件")
                    .ghost()
                    .w_full()
                    .h(px(28.))
                    .icon(IconName::Check)
                    .on_click(move |_, _, cx| cx.reveal_path(&path)),
            );
        }
        output.into_any_element()
    }
}
impl Render for Tools {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut inspector = div()
            .w(px(252.))
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .pl(px(20.))
            .border_l_1()
            .border_color(crate::theme::color(BORDER))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(label("处理设置", 13., TEXT).font_weight(FontWeight::SEMIBOLD))
                    .child(div().flex_1())
                    .child(
                        action("tools-presets", "预设")
                            .ghost()
                            .h(px(28.))
                            .px(px(6.))
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.presets(window, cx))),
                    )
                    .child(
                        Button::new("tools-reset")
                            .ghost()
                            .size(px(28.))
                            .disabled(self.busy)
                            .child(
                                icon("arrow-clockwise", 15.)
                                    .text_color(crate::theme::color(NAV_TEXT)),
                            )
                            .tooltip(crate::i18n::text("重置参数"))
                            .accessibility_label(crate::i18n::text("重置参数"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.reset_settings(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div().size_full().pr(px(20.)).child(
                            div()
                                .id("tool-inspector-scroll")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.inspector_scroll)
                                .child(self.controls(cx)),
                        ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .w(px(12.))
                            .child(
                                Scrollbar::vertical(&self.inspector_scroll)
                                    .viewport_from_layout()
                                    .mode(ScrollbarMode::Always)
                                    .styles(|styles| {
                                        styles
                                            .track(|track| track.width(px(12.)))
                                            .track_hover(|track| track.width(px(12.)))
                                            .track_active(|track| track.width(px(12.)))
                                    }),
                            ),
                    ),
            );
        if let Some(error) = &self.error {
            inspector = inspector.child(
                div()
                    .id("tool-error")
                    .max_h(px(76.))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .child(label(error.clone(), 12., RED)),
            );
        }
        inspector = inspector.child(self.output_controls(cx));
        div()
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .p(px(22.))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.add_paths(paths.0.to_vec(), HashMap::new(), cx)
            }))
            .child(
                div()
                    .size_full()
                    .max_w(px(1160.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                label(
                                    if self.tool == Tool::Geometry {
                                        "尺寸与裁剪"
                                    } else {
                                        "转换与压缩"
                                    },
                                    20.,
                                    TEXT,
                                )
                                .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(div().flex_1())
                            .child(
                                action("tools-paste", "粘贴图片")
                                    .ghost()
                                    .disabled(self.busy || self.importing)
                                    .on_click(cx.listener(|this, _, _, cx| this.paste(cx))),
                            )
                            .child(
                                action(
                                    "tools-open",
                                    if self.importing {
                                        "正在读取…"
                                    } else if self.input.is_some() {
                                        "更换图片"
                                    } else {
                                        "打开图片"
                                    },
                                )
                                .disabled(self.busy || self.importing)
                                .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .max_h(px(760.))
                            .flex()
                            .gap(px(20.))
                            .child(self.preview_frame(cx))
                            .child(inspector),
                    ),
            )
    }
}
