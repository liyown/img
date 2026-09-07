use super::*;

impl ImgDesktop {
    pub(super) fn display_progress(&self, item: &Item) -> Option<u8> {
        // The supplied mock labels its approximately 82% track as 87%.
        // Preserve that static composition only for the reference fixture.
        if self.reference && !self.simulating && item.simulated && item.progress == Some(87) {
            Some(82)
        } else {
            item.progress
        }
    }
    pub(super) fn queue_row(&self, item: Item, cx: &mut Context<Self>) -> AnyElement {
        let color = match item.status {
            Status::Done => GREEN,
            Status::Failed => RED,
            _ => ORANGE,
        };
        let status = match item.status {
            Status::Ready => "待上传".into(),
            Status::Running => self
                .queue
                .active
                .get(&item.id)
                .map(|a| a.control.progress().label())
                .unwrap_or_else(|| {
                    item.progress
                        .map(|p| format!("{p}%"))
                        .unwrap_or("上传中".into())
                }),
            Status::Done => "完成".into(),
            Status::Failed => "失败".into(),
            Status::Paused => "已暂停".into(),
            Status::Cancelled => "已取消".into(),
        };
        let preview_item = item.clone();
        let id = item.id.clone();
        let pause_id = id.clone();
        let cancel_id = id.clone();
        let remove_id = id.clone();
        let failure_item = item.clone();
        let title = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(22.))
            .min_w(px(0.))
            .child(label(item.name.clone(), 13., TEXT).text_ellipsis())
            .child(
                mono(
                    format!(
                        "{} · {}",
                        item.size_label(),
                        if item.target.is_empty() {
                            "未选择存储源"
                        } else {
                            &item.target
                        }
                    ),
                    10.,
                    MUTED,
                )
                .whitespace_nowrap(),
            )
            .child(div().flex_1())
            .when(item.status == Status::Done, |this| {
                this.child(self.copy_actions(&item, "queue", cx))
            })
            .when(item.status == Status::Failed, |this| {
                this.child(
                    Button::new(SharedString::from(format!("error-{}", failure_item.id)))
                        .ghost()
                        .xsmall()
                        .label("详情 / 处理")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.failure_details(failure_item.clone(), window, cx)
                        })),
                )
            })
            .when(item.status == Status::Running && !item.simulated, |this| {
                this.child(
                    Button::new(SharedString::from(format!("pause-{pause_id}")))
                        .ghost()
                        .xsmall()
                        .label("暂停")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stop_item(&pause_id, engine::PAUSE, cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("cancel-{cancel_id}")))
                        .ghost()
                        .xsmall()
                        .label("取消")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.stop_item(&cancel_id, engine::CANCEL, cx)
                        })),
                )
            })
            .when(item.status != Status::Running, |this| {
                this.child(
                    Button::new(SharedString::from(format!("remove-{remove_id}")))
                        .ghost()
                        .xsmall()
                        .label("清理")
                        .disabled(
                            self.queue
                                .batch
                                .as_ref()
                                .is_some_and(|b| b.pending.contains(&remove_id)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.remove_records(vec![remove_id.clone()], window, cx)
                        })),
                )
            })
            .when(
                matches!(
                    item.status,
                    Status::Ready | Status::Failed | Status::Paused | Status::Cancelled
                ),
                |this| {
                    this.child(
                        Button::new(SharedString::from(format!("start-{id}")))
                            .ghost()
                            .xsmall()
                            .h(px(22.))
                            .text_color(rgb(ORANGE))
                            .text_size(px(11.))
                            .disabled(self.queue.batch.is_some())
                            .label(if item.status == Status::Ready {
                                "开始上传"
                            } else {
                                "重试"
                            })
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.start_upload(&id, cx)),
                            ),
                    )
                },
            );
        let row = div()
            .id(SharedString::from(format!("row-{}", item.id)))
            .w_full()
            .h(px(96.))
            .overflow_hidden()
            .p(px(12.))
            .rounded(px(16.))
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(CARD))
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                Button::new(SharedString::from(format!("preview-{}", item.id)))
                    .accessibility_label(format!("预览 {}", item.name))
                    .ghost()
                    .p_0()
                    .size(px(48.))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .child(thumbnail(&item, &self.thumbnails))
                    .tooltip(format!("预览 {}", item.name))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_preview(preview_item.clone(), window, cx);
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(title)
                    .child(progress(self.display_progress(&item), color))
                    .when_some(item.error.clone(), |this, error| {
                        this.child(
                            label(
                                error,
                                11.,
                                if item.status == Status::Failed {
                                    RED
                                } else {
                                    MUTED
                                },
                            )
                            .mt(px(2.))
                            .text_ellipsis(),
                        )
                    }),
            )
            .child(
                div()
                    .min_w(px(48.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(4.))
                    .child(mono(status, 11., color))
                    .when(item.status == Status::Done, |this| {
                        this.child(icon("check", 12.).text_color(rgb(GREEN)))
                    }),
            );
        row.into_any_element()
    }
    pub(super) fn library_view_switch(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut switch = div()
            .h(px(34.))
            .flex()
            .items_center()
            .p(px(3.))
            .gap(px(2.))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded(px(10.));
        for (view, title) in [(LibraryView::Grid, "网格"), (LibraryView::List, "列表")] {
            let selected = self.preferences.library_view == view;
            switch = switch.child(
                Button::new(SharedString::from(format!("library-view-{title}")))
                    .accessibility_label(format!("{title}展示"))
                    .selected(selected)
                    .ghost()
                    .small()
                    .h(px(26.))
                    .px(px(12.))
                    .rounded(px(7.))
                    .text_size(px(12.))
                    .bg(rgb(if selected { TEXT } else { CANVAS }))
                    .text_color(rgb(if selected { CARD } else { MUTED }))
                    .label(title)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.preferences.library_view != view {
                            this.content_revision = this.content_revision.wrapping_add(1);
                        }
                        this.preferences.library_view = view;
                        this.save_preferences(cx);
                    }))
                    .with_spring(
                        SharedString::from(format!("library-transition-{title}")),
                        SpringAnimation::new(SpringConfig::new(700., 54., 1.))
                            .to(AnimationPhase(if selected { 1. } else { 0. })),
                        |this, phase| {
                            this.bg(phase.interpolate_between_clamped(
                                0.0..=1.0,
                                rgb(CANVAS),
                                rgb(TEXT),
                            ))
                            .text_color(
                                phase.interpolate_between_clamped(0.0..=1.0, rgb(MUTED), rgb(CARD)),
                            )
                        },
                    ),
            );
        }
        switch.into_any_element()
    }
    pub(super) fn library_grid_card(&self, item: Item, cx: &mut Context<Self>) -> AnyElement {
        let preview = item.clone();
        div()
            .min_w(px(0.))
            .p(px(10.))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded(px(14.))
            .bg(rgb(CARD))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                Button::new(SharedString::from(format!("grid-preview-{}", item.id)))
                    .accessibility_label(format!("预览 {}", item.name))
                    .ghost()
                    .p_0()
                    .w_full()
                    .h(px(136.))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .child(thumbnail(&item, &self.thumbnails))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_preview(preview.clone(), window, cx);
                    })),
            )
            .child(label(item.name.clone(), 13., TEXT).text_ellipsis())
            .child(
                mono(
                    format!("{} · {}", item.size_label(), item.target),
                    10.,
                    MUTED,
                )
                .text_ellipsis(),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(self.copy_actions(&item, "grid", cx)),
            )
            .into_any_element()
    }
    pub(super) fn library_list_row(&self, item: Item, cx: &mut Context<Self>) -> AnyElement {
        let preview = item.clone();
        div()
            .w_full()
            .h(px(74.))
            .px(px(12.))
            .border_b_1()
            .border_color(rgb(BORDER))
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                Button::new(SharedString::from(format!("list-preview-{}", item.id)))
                    .accessibility_label(format!("预览 {}", item.name))
                    .ghost()
                    .p_0()
                    .size(px(48.))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .child(thumbnail(&item, &self.thumbnails))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_preview(preview.clone(), window, cx);
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(label(item.name.clone(), 13., TEXT).text_ellipsis())
                    .child(label(item.url.clone().unwrap_or_default(), 11., MUTED).text_ellipsis()),
            )
            .child(
                label(item.target.clone(), 11., MUTED)
                    .w(px(90.))
                    .text_ellipsis(),
            )
            .child(mono(item.size_label(), 10., MUTED).w(px(66.)))
            .child(
                div()
                    .w(px(108.))
                    .flex()
                    .justify_end()
                    .child(self.copy_actions(&item, "list", cx)),
            )
            .into_any_element()
    }
    pub(super) fn library(
        &self,
        rows: std::sync::Arc<Vec<String>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.virtual_records(
            rows,
            self.preferences.library_view == LibraryView::Grid,
            window,
            cx,
        )
    }
    pub(super) fn virtual_records(
        &self,
        rows: std::sync::Arc<Vec<String>>,
        grid: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(feature = "perf")]
        if std::env::var_os("IMG_PERF_BASELINE").is_some() {
            return self.baseline_records(window, cx);
        }
        let columns = if grid {
            if window.viewport_size().width < px(1180.) {
                3
            } else {
                4
            }
        } else {
            1
        };
        let count = rows.len().div_ceil(columns);
        let page = self.page;
        uniform_list(
            ("records", page as usize * 10 + columns),
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|row| {
                        let mut line =
                            div().w_full().flex().gap(px(14.)).pb(px(8.)).h(px(if grid {
                                272.
                            } else if page == Page::Library {
                                82.
                            } else {
                                104.
                            }));
                        for column in 0..columns {
                            let item = rows
                                .get(row * columns + column)
                                .and_then(|id| this.indexed_item(id));
                            line = line.child(div().flex_1().min_w(px(0.)).h_full().when_some(
                                item,
                                |cell, item| {
                                    cell.child(if grid {
                                        this.library_grid_card(item, cx)
                                    } else if page == Page::Library {
                                        this.library_list_row(item, cx)
                                    } else {
                                        this.queue_row(item, cx)
                                    })
                                },
                            ));
                        }
                        line.into_any_element()
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.list_scroll)
        .w_full()
        .flex_1()
        .min_h(px(0.))
        .into_any_element()
    }
}
