use super::*;
impl Tools {
    fn field(&self, key: &'static str, title: &str, width: f32) -> AnyElement {
        div()
            .w(px(width))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(label(title, 11., MUTED))
            .child(
                Input::new(&self.fields[key])
                    .aria_label(crate::i18n::text(title.to_owned()))
                    .h(px(29.))
                    .text_size(px(12.)),
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
        action(id, labels[selected])
            .dropdown_menu(move |mut menu, _, _| {
                for (index, label) in labels.iter().enumerate() {
                    let weak = weak.clone();
                    let change = change.clone();
                    menu = menu.item(
                        PopupMenuItem::new(crate::i18n::text(*label))
                            .checked(index == selected)
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    change(this, index);
                                    this.schedule_preview(cx);
                                });
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }
    fn controls(&self, cx: &Context<Self>) -> AnyElement {
        let mut controls = div().flex().flex_col().gap(px(10.));
        let mut row = div().flex().items_end().gap(px(9.)).flex_wrap();
        match self.tool {
            Tool::Convert => {
                row = row
                    .child(self.choose_option(
                        "tool-format",
                        &["PNG", "JPEG", "WebP"],
                        match self.plan.encoding.format {
                            plan::Format::Png => 0,
                            plan::Format::Jpeg => 1,
                            plan::Format::Webp => 2,
                        },
                        |this, index| {
                            this.plan.encoding.format = match index {
                                1 => plan::Format::Jpeg,
                                2 => plan::Format::Webp,
                                _ => plan::Format::Png,
                            };
                            if index == 1 && this.compression_mode == 2 {
                                this.compression_mode = 0;
                            }
                        },
                        cx,
                    ))
                    .child(self.choose_option(
                        "tool-compression",
                        if self.plan.encoding.format == plan::Format::Jpeg {
                            &["质量", "目标体积"]
                        } else {
                            &["质量", "目标体积", "无损"]
                        },
                        self.compression_mode as usize,
                        |this, index| this.compression_mode = index as u8,
                        cx,
                    ));
                if self.compression_mode == 1 {
                    row = row.child(self.field("target", "目标 KiB", 90.));
                } else if self.compression_mode == 0
                    && self.plan.encoding.format != plan::Format::Png
                {
                    row = row.child(self.field("quality", "质量 1–100", 84.));
                }
                if self.plan.encoding.format == plan::Format::Jpeg {
                    row = row.child(self.field("jpeg_bg", "透明区域背景", 110.));
                }
                controls = controls.child(row).child(label(
                    if self.plan.encoding.format == plan::Format::Png {
                        "PNG 保持无损；目标体积不会改变尺寸。"
                    } else {
                        "目标体积只调整编码质量，保留格式和尺寸。"
                    },
                    11.,
                    MUTED,
                ));
            }
            Tool::Geometry => {
                row = row.child(self.choose_option(
                    "tool-resize",
                    &["原始尺寸", "最长边", "宽高"],
                    self.resize_mode as usize,
                    |this, index| this.resize_mode = index as u8,
                    cx,
                ));
                if self.resize_mode == 1 {
                    row = row.child(self.field("edge", "最长边 px", 90.));
                } else if self.resize_mode == 2 {
                    row = row
                        .child(self.field("width", "宽 px", 70.))
                        .child(self.field("height", "高 px · 0 为自动", 96.));
                    let keep = self
                        .plan
                        .geometry
                        .resize
                        .as_ref()
                        .is_none_or(|r| r.keep_aspect);
                    row = row.child(self.choose_option(
                        "tool-aspect",
                        &["保持比例", "拉伸至尺寸"],
                        usize::from(!keep),
                        |this, index| {
                            this.plan
                                .geometry
                                .resize
                                .get_or_insert_with(Default::default)
                                .keep_aspect = index == 0
                        },
                        cx,
                    ));
                }
                let weak = cx.weak_entity();
                row = row.child(action("tool-rotate", "旋转 90°").ghost().on_click(
                    move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.plan.geometry.quarter_turns =
                                (this.plan.geometry.quarter_turns + 1) % 4;
                            this.schedule_preview(cx);
                        });
                    },
                ));
                let weak = cx.weak_entity();
                row = row.child(action("tool-flip-h", "水平翻转").ghost().on_click(
                    move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.plan.geometry.flip_horizontal =
                                !this.plan.geometry.flip_horizontal;
                            this.schedule_preview(cx);
                        });
                    },
                ));
                let weak = cx.weak_entity();
                row = row.child(action("tool-flip-v", "垂直翻转").ghost().on_click(
                    move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.plan.geometry.flip_vertical = !this.plan.geometry.flip_vertical;
                            this.schedule_preview(cx);
                        });
                    },
                ));
                controls = controls.child(row);
                if self.resize_mode > 0 {
                    controls = controls.child(
                        self.choose_option(
                            "tool-upscale",
                            &["不放大小图", "允许放大"],
                            usize::from(
                                self.plan
                                    .geometry
                                    .resize
                                    .as_ref()
                                    .is_some_and(|r| r.allow_upscale),
                            ),
                            |this, index| {
                                this.plan
                                    .geometry
                                    .resize
                                    .get_or_insert_with(Default::default)
                                    .allow_upscale = index == 1;
                            },
                            cx,
                        ),
                    );
                }
                let weak = cx.weak_entity();
                let mut crop = div().flex().gap(px(8.)).items_center().child(
                    action(
                        "tool-crop",
                        if self.crop_editing {
                            "取消选区"
                        } else {
                            "选择裁剪区域"
                        },
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.crop_editing = !this.crop_editing;
                            this.drag = None;
                            this.schedule_preview(cx);
                        });
                    }),
                );
                crop = crop.child(self.choose_option(
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
                ));
                if self.plan.geometry.crop.is_some() {
                    let weak = cx.weak_entity();
                    crop = crop.child(action("tool-reset-crop", "移除裁剪").ghost().on_click(
                        move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.plan.geometry.crop = None;
                                this.schedule_preview(cx);
                            });
                        },
                    ));
                }
                controls = controls.child(crop).when(self.crop_editing, |view| {
                    view.child(label("在预览上拖动选择区域，松开后应用。", 11., MUTED))
                });
            }
            Tool::Annotate => {
                row = row.child(self.choose_option(
                    "tool-mark",
                    &["选择与移动", "箭头", "矩形", "文字", "步骤编号", "实色遮盖"],
                    match self.mark_tool {
                        MarkTool::Select => 0,
                        MarkTool::Arrow => 1,
                        MarkTool::Rectangle => 2,
                        MarkTool::Text => 3,
                        MarkTool::Step => 4,
                        MarkTool::Redact => 5,
                    },
                    |this, index| {
                        this.mark_tool = match index {
                            0 => MarkTool::Select,
                            1 => MarkTool::Arrow,
                            2 => MarkTool::Rectangle,
                            3 => MarkTool::Text,
                            4 => MarkTool::Step,
                            _ => MarkTool::Redact,
                        };
                        this.selected_annotation = None;
                    },
                    cx,
                ));
                row = row
                    .child(self.field("color", "颜色", 96.))
                    .child(self.field("stroke", "线宽 px", 70.));
                if self.mark_tool == MarkTool::Text {
                    row = row
                        .child(self.field("text", "文字内容", 150.))
                        .child(self.field("font_size", "字号 px", 70.));
                }
                let undo = self.edits().is_some_and(Edits::can_undo);
                let redo = self.edits().is_some_and(Edits::can_redo);
                let weak = cx.weak_entity();
                row = row.child(
                    action("tool-undo", "撤销")
                        .ghost()
                        .disabled(!undo)
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(edits) = this.edits_mut() {
                                    edits.undo();
                                }
                                this.schedule_preview(cx);
                            });
                        }),
                );
                let weak = cx.weak_entity();
                row = row.child(
                    action("tool-redo", "重做")
                        .ghost()
                        .disabled(!redo)
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(edits) = this.edits_mut() {
                                    edits.redo();
                                }
                                this.schedule_preview(cx);
                            });
                        }),
                );
                let weak = cx.weak_entity();
                row = row.child(
                    action("tool-delete-mark", "删除标注")
                        .ghost()
                        .disabled(self.selected_annotation.is_none())
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                if let Some(id) = this.selected_annotation.take()
                                    && let Some(edits) = this.edits_mut()
                                {
                                    let annotations = edits
                                        .annotations
                                        .iter()
                                        .filter(|a| a.id != id)
                                        .cloned()
                                        .collect();
                                    edits.replace(annotations);
                                }
                                this.schedule_preview(cx);
                            });
                        }),
                );
                controls = controls.child(row).child(label(
                    "文字和编号点击添加；形状拖动绘制。标注按图片分别保存。",
                    11.,
                    MUTED,
                ));
            }
            Tool::Split => {
                row = row.child(self.choose_option(
                    "tool-split",
                    &["不切分", "按高度", "按行列"],
                    self.split_mode as usize,
                    |this, index| this.split_mode = index as u8,
                    cx,
                ));
                if self.split_mode == 1 {
                    row = row.child(self.field("split_height", "每段高度 px", 110.));
                } else if self.split_mode == 2 {
                    row = row
                        .child(self.field("rows", "行数", 74.))
                        .child(self.field("cols", "列数", 74.));
                }
                controls = controls.child(row).child(label(
                    "保留末尾不足一块的内容，按从左到右、从上到下的顺序导出。",
                    11.,
                    MUTED,
                ));
            }
            Tool::Stitch => {
                row = row
                    .child(self.choose_option(
                        "tool-stitch",
                        &["不拼接", "纵向", "横向"],
                        match &self.plan.stitch {
                            None => 0,
                            Some(s) if s.direction == plan::Direction::Vertical => 1,
                            _ => 2,
                        },
                        |this, index| {
                            this.plan.stitch = if index == 0 {
                                None
                            } else {
                                Some(plan::Stitch {
                                    direction: if index == 1 {
                                        plan::Direction::Vertical
                                    } else {
                                        plan::Direction::Horizontal
                                    },
                                    ..Default::default()
                                })
                            };
                        },
                        cx,
                    ))
                    .child(self.field("spacing", "间距 px", 76.))
                    .child(self.field("cross_size", "统一边长 · 0 为原始", 125.));
                let weak = cx.weak_entity();
                row = row.child(
                    action("tool-up", "上移")
                        .ghost()
                        .disabled(self.selected == 0)
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.inputs.swap(this.selected, this.selected - 1);
                                this.selected -= 1;
                                this.schedule_preview(cx);
                            });
                        }),
                );
                let weak = cx.weak_entity();
                row = row.child(
                    action("tool-down", "下移")
                        .ghost()
                        .disabled(self.selected + 1 >= self.inputs.len())
                        .on_click(move |_, _, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.inputs.swap(this.selected, this.selected + 1);
                                this.selected += 1;
                                this.schedule_preview(cx);
                            });
                        }),
                );
                controls = controls.child(row).child(label(
                    "按左侧文件顺序拼接，统一尺寸时默认不放大小图。",
                    11.,
                    MUTED,
                ));
                if let Some(stitch) = &self.plan.stitch {
                    let mut options = div()
                        .flex()
                        .items_end()
                        .gap(px(8.))
                        .flex_wrap()
                        .child(self.choose_option(
                            "stitch-upscale",
                            &["不放大小图", "允许放大"],
                            usize::from(stitch.allow_upscale),
                            |this, index| {
                                if let Some(stitch) = &mut this.plan.stitch {
                                    stitch.allow_upscale = index == 1;
                                }
                            },
                            cx,
                        ))
                        .child(self.choose_option(
                            "stitch-background",
                            &["透明背景", "纯色背景"],
                            usize::from(stitch.background[3] != 0),
                            |this, index| {
                                if let Some(stitch) = &mut this.plan.stitch {
                                    stitch.background[3] = if index == 0 { 0 } else { 255 };
                                }
                            },
                            cx,
                        ));
                    if stitch.background[3] != 0 {
                        options = options.child(self.field("stitch_bg", "背景颜色", 96.));
                    }
                    controls = controls.child(options);
                }
            }
            Tool::Watermark => {
                let weak = cx.weak_entity();
                row = row.child(
                    action(
                        "tool-watermark-file",
                        if self.watermark.is_some() {
                            "更换水印图"
                        } else {
                            "选择水印图"
                        },
                    )
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| this.choose_watermark(cx));
                    }),
                );
                if self.watermark.is_some() {
                    row = row
                        .child(self.field("watermark_scale", "宽度占比 %", 90.))
                        .child(self.field("opacity", "透明度 %", 84.))
                        .child(self.field("margin", "边距 px", 74.));
                }
                controls = controls.child(row);
                if let Some(mark) = &self.plan.watermark {
                    let positions = [
                        plan::Position::TopLeft,
                        plan::Position::Top,
                        plan::Position::TopRight,
                        plan::Position::Left,
                        plan::Position::Center,
                        plan::Position::Right,
                        plan::Position::BottomLeft,
                        plan::Position::Bottom,
                        plan::Position::BottomRight,
                    ];
                    let current = positions
                        .iter()
                        .position(|p| *p == mark.position)
                        .unwrap_or(8);
                    let weak = cx.weak_entity();
                    controls = controls.child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(self.choose_option(
                                "tool-watermark-position",
                                &[
                                    "左上", "上方", "右上", "左侧", "居中", "右侧", "左下", "下方",
                                    "右下",
                                ],
                                current,
                                move |this, index| {
                                    this.plan.watermark.as_mut().unwrap().position =
                                        positions[index]
                                },
                                cx,
                            ))
                            .child(
                                action("tool-watermark-remove", "移除水印")
                                    .ghost()
                                    .on_click(move |_, _, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.watermark = None;
                                            this.plan.watermark = None;
                                            this.schedule_preview(cx);
                                        });
                                    }),
                            ),
                    );
                }
            }
        }
        controls.into_any_element()
    }
    fn choose_watermark(&mut self, cx: &mut Context<Self>) {
        let task = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::i18n::text("选择水印图片")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = task.await
                && let Some(path) = paths.first()
            {
                let path = path.clone();
                let task = cx.background_executor().spawn(async move {
                    (|| -> anyhow::Result<(PathBuf, String)> {
                        anyhow::ensure!(path.metadata()?.len() <= 20 << 20, "水印图片超过 20 MiB");
                        let bytes = std::fs::read(&path)?;
                        Ok((path, img_records::catalog::digest(&bytes)))
                    })()
                });
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(mark) => {
                            if this.watermark.is_none()
                                && this.plan.watermark.as_ref().is_some_and(|preset| {
                                    !preset.resource.is_empty() && preset.resource != mark.1
                                })
                            {
                                this.error = Some(
                                    "水印图片与预设不一致，请选择原水印或移除水印后重新设置".into(),
                                );
                                cx.notify();
                                return;
                            }
                            this.plan
                                .watermark
                                .get_or_insert_with(Default::default)
                                .resource = mark.1.clone();
                            this.watermark = Some(mark);
                            this.schedule_preview(cx);
                        }
                        Err(error) => this.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
impl Render for Tools {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tool_names = ["转换与压缩", "尺寸与裁剪", "标注", "切分", "拼接", "水印"];
        let current = match self.tool {
            Tool::Convert => 0,
            Tool::Geometry => 1,
            Tool::Annotate => 2,
            Tool::Split => 3,
            Tool::Stitch => 4,
            Tool::Watermark => 5,
        };
        let mut view = div()
            .size_full()
            .p(px(22.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.add_paths(paths.0.to_vec(), HashMap::new(), cx)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(label("图片工具", 20., TEXT))
                    .child(
                        action("tools-presets", "预设")
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| this.presets(window, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        action("tools-paste", "粘贴")
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.paste(cx))),
                    )
                    .child(
                        action(
                            "tools-open",
                            if self.importing {
                                "正在读取…"
                            } else {
                                "打开图片"
                            },
                        )
                        .disabled(self.importing)
                        .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                    ),
            );
        if self.inputs.is_empty() {
            if let Some(error) = &self.error {
                view = view.child(label(error.clone(), 12., RED));
            }
            return view
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(14.))
                        .child(icon("image", 40.).text_color(crate::theme::color(MUTED)))
                        .child(label("将图片或文件夹拖到这里", 18., TEXT))
                        .child(label(
                            "转换、压缩、裁剪、标注、切分和拼接，无需配置图床。",
                            12.,
                            MUTED,
                        )),
                )
                .into_any_element();
        }
        let files = uniform_list(
            "tool-input-list",
            self.inputs.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let input = &this.inputs[index];
                        div()
                            .h(px(42.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .when(index == this.selected, |row| {
                                row.bg(crate::theme::color(BORDER))
                            })
                            .child(
                                label(
                                    input
                                        .path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned(),
                                    12.,
                                    TEXT,
                                )
                                .text_ellipsis(),
                            )
                            .id(SharedString::from(format!("tool-input-{index}")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = index;
                                this.selected_annotation = None;
                                this.schedule_preview(cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .flex_1()
        .min_h_0();
        let mut rail = div()
            .w(px(152.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(label(format!("文件 · {}", self.inputs.len()), 12., MUTED))
            .child(files);
        if !self.results.is_empty() {
            let results =
                uniform_list(
                    "tool-output-list",
                    self.results.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|index| {
                                let item = &this.results[index];
                                let size = item["image"]["size"].as_u64().unwrap_or(0);
                                div()
                                    .h(px(38.))
                                    .px(px(6.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(5.))
                                    .when(this.result_selected == Some(index), |row| {
                                        row.bg(crate::theme::color(BORDER))
                                    })
                                    .child(label(
                                        format!("{:02} · {}", index + 1, model::size_label(size)),
                                        11.,
                                        TEXT,
                                    ))
                                    .id(SharedString::from(format!("tool-result-{index}")))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_result(index, cx)
                                    }))
                                    .into_any_element()
                            })
                            .collect()
                    }),
                )
                .h(px(152.));
            rail = rail
                .child(label(format!("结果 · {}", self.results.len()), 12., MUTED))
                .child(results);
        }
        let mut workspace = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.choose_option(
                "tools-select-tool",
                &tool_names,
                current,
                |this, index| {
                    this.tool = match index {
                        1 => Tool::Geometry,
                        2 => Tool::Annotate,
                        3 => Tool::Split,
                        4 => Tool::Stitch,
                        5 => Tool::Watermark,
                        _ => Tool::Convert,
                    };
                    this.crop_editing = false;
                },
                cx,
            ));
        workspace = workspace.child(self.canvas(cx));
        if let Some(preview) = &self.preview {
            let metadata = &preview.metadata["image"];
            let size = metadata["size"].as_u64().unwrap_or(0);
            let target = metadata["target_met"] == false;
            workspace = workspace.child(label(
                format!(
                    "{} × {} · {} · {} · {}{}{}",
                    metadata["width"],
                    metadata["height"],
                    metadata["content_type"]
                        .as_str()
                        .unwrap_or("")
                        .trim_start_matches("image/"),
                    model::size_label(size),
                    crate::i18n::text(if metadata["has_alpha"] == true {
                        "含透明区域"
                    } else {
                        "不透明"
                    }),
                    metadata["saving_percent"]
                        .as_f64()
                        .map(|saving| if saving >= 0. {
                            format!(" · −{saving:.0}%")
                        } else {
                            format!(" · +{:.0}%", -saving)
                        })
                        .unwrap_or_default(),
                    if target {
                        " · 未达到目标体积"
                    } else {
                        ""
                    }
                ),
                11.,
                if target { RED } else { MUTED },
            ));
        }
        workspace = workspace.child(
            div()
                .border_t_1()
                .border_color(crate::theme::color(BORDER))
                .pt(px(12.))
                .child(self.controls(cx)),
        );
        if let Some(error) = &self.error {
            workspace = workspace.child(
                div()
                    .id("tool-error")
                    .max_h(px(66.))
                    .overflow_y_scroll()
                    .child(label(error.clone(), 12., RED)),
            );
        }
        workspace = workspace.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .flex_wrap()
                .child(
                    action("tools-copy-image", "复制图片")
                        .when(
                            self.copied_until
                                .is_some_and(|until| until > Instant::now()),
                            |button| button.icon(IconName::Check),
                        )
                        .ghost()
                        .disabled(self.preview.is_none() || self.preview_busy)
                        .on_click(cx.listener(|this, _, _, cx| this.copy_image(cx))),
                )
                .child(
                    action("tools-upload-results", "上传结果")
                        .disabled(self.task_id.is_none() || self.results.is_empty() || self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.publish(window, cx))),
                )
                .child(div().flex_1())
                .when(self.busy, |row| {
                    row.child(
                        action("tools-cancel-task", "取消")
                            .ghost()
                            .on_click(cx.listener(|this, _, _, _| {
                                if let Some(control) = &this.control {
                                    control.stop(engine::CANCEL);
                                }
                            })),
                    )
                })
                .child(
                    action(
                        "tools-export",
                        if self.busy {
                            "处理中…"
                        } else {
                            "保存结果"
                        },
                    )
                    .primary()
                    .disabled(self.busy || self.crop_editing)
                    .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                ),
        );
        view = view.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .gap(px(18.))
                .child(rail)
                .child(workspace),
        );
        view.into_any_element()
    }
}
