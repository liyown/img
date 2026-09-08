use super::*;
impl Tools {
    fn image_geometry(&self) -> Option<(f32, f32, f32, f32, f32)> {
        let metadata = &self.preview.as_ref()?.metadata["image"];
        let width = metadata["width"].as_u64()? as f32;
        let height = metadata["height"].as_u64()? as f32;
        if width == 0. || height == 0. {
            return None;
        }
        let bounds = self.bounds.get();
        let scale =
            (f32::from(bounds.size.width) / width).min(f32::from(bounds.size.height) / height);
        if scale <= 0. {
            return None;
        }
        Some((
            f32::from(bounds.origin.x) + (f32::from(bounds.size.width) - width * scale) / 2.,
            f32::from(bounds.origin.y) + (f32::from(bounds.size.height) - height * scale) / 2.,
            width,
            height,
            scale,
        ))
    }
    fn point_on_image(&self, p: Point<Pixels>, clamp: bool) -> Option<plan::Point> {
        let (x, y, w, h, scale) = self.image_geometry()?;
        let (px, py) = ((f32::from(p.x) - x) / scale, (f32::from(p.y) - y) / scale);
        if !clamp && (px < 0. || py < 0. || px > w || py > h) {
            return None;
        }
        Some(plan::Point {
            x: px.clamp(0., w),
            y: py.clamp(0., h),
        })
    }
    fn pointer_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview_busy
            || self.result_selected.is_some()
            || (!self.crop_editing && self.tool != Tool::Annotate)
        {
            return;
        }
        let Some(point) = self.point_on_image(event.position, false) else {
            return;
        };
        self.focus.focus(window, cx);
        if !self.crop_editing && self.mark_tool == MarkTool::Select {
            self.selected_annotation = self.edits().and_then(|edits| {
                edits
                    .annotations
                    .iter()
                    .rev()
                    .find(|annotation| {
                        let (x, y, right, bottom) = crate::tool_editor::bounds(annotation);
                        point.x >= x - 6.
                            && point.x <= right + 6.
                            && point.y >= y - 6.
                            && point.y <= bottom + 6.
                    })
                    .map(|annotation| annotation.id.clone())
            });
            if self.selected_annotation.is_none() {
                self.drag = None;
                cx.notify();
                return;
            }
        }
        self.drag = Some((point, point));
        cx.notify();
    }
    fn pointer_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start, _)) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            cx.notify();
            return;
        }
        let Some(mut point) = self.point_on_image(event.position, true) else {
            return;
        };
        if self.crop_editing
            && let Some(ratio) = self.crop_ratio
            && let Some((_, _, w, h, _)) = self.image_geometry()
        {
            let dx = point.x - start.x;
            let dy = point.y - start.y;
            let mut width = dx.abs();
            let mut height = width / ratio;
            let allowed = if dy < 0. { start.y } else { h - start.y };
            if height > allowed {
                height = allowed;
                width = height * ratio;
            }
            point.x = (start.x + width * if dx < 0. { -1. } else { 1. }).clamp(0., w);
            point.y = start.y + height * if dy < 0. { -1. } else { 1. };
        }
        self.drag = Some((start, point));
        cx.notify();
    }
    fn pointer_up(&mut self, cx: &mut Context<Self>) {
        let Some((from, to)) = self.drag.take() else {
            return;
        };
        if self.crop_editing {
            let x = from.x.min(to.x).floor().max(0.) as u32;
            let y = from.y.min(to.y).floor().max(0.) as u32;
            let width = (from.x.max(to.x).floor() as u32).saturating_sub(x);
            let height = (from.y.max(to.y).floor() as u32).saturating_sub(y);
            if width > 0 && height > 0 {
                self.plan.geometry.crop = Some(plan::Rect {
                    x,
                    y,
                    width,
                    height,
                });
                self.crop_editing = false;
                self.schedule_preview(cx);
            } else {
                cx.notify();
            }
            return;
        }
        if self.mark_tool == MarkTool::Select {
            if let Some(id) = self.selected_annotation.clone()
                && let Some(edits) = self.edits_mut()
            {
                edits.move_annotation(&id, to.x - from.x, to.y - from.y);
            }
            self.schedule_preview(cx);
            return;
        }
        let shape = match self.mark_tool {
            MarkTool::Arrow => Shape::Arrow { from, to },
            MarkTool::Rectangle => Shape::Rectangle { from, to },
            MarkTool::Redact => Shape::Redact { from, to },
            MarkTool::Text => Shape::Text {
                at: from,
                text: self.value("text", cx),
                size: self.number("font_size", "字号", cx).unwrap_or(32) as f32,
            },
            MarkTool::Step => Shape::Step {
                at: from,
                number: self
                    .edits()
                    .map(|edits| {
                        edits
                            .annotations
                            .iter()
                            .filter(|a| matches!(a.shape, Shape::Step { .. }))
                            .count()
                            + 1
                    })
                    .unwrap_or(1)
                    .min(9999) as u16,
                radius: 18.,
            },
            MarkTool::Select => return,
        };
        let color = match self.color("color", cx) {
            Ok(color) => color,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let annotation = Annotation {
            id: uuid::Uuid::new_v4().to_string(),
            shape,
            color,
            stroke_width: self.number("stroke", "线宽", cx).unwrap_or(4) as f32,
        };
        self.selected_annotation = Some(annotation.id.clone());
        if let Some(edits) = self.edits_mut() {
            let mut annotations = edits.annotations.clone();
            annotations.push(annotation);
            edits.replace(annotations);
        }
        self.schedule_preview(cx);
    }
    pub(super) fn canvas(&self, cx: &Context<Self>) -> AnyElement {
        let bounds = self.bounds.clone();
        let weak = cx.weak_entity();
        let overlay = canvas(
            move |area, _, _| {
                bounds.set(area);
                area
            },
            move |_, _, window, cx| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                let this = this.read(cx);
                let Some((x, y, width, height, scale)) = this.image_geometry() else {
                    return;
                };
                if this.tool == Tool::Split
                    && this.result_selected.is_none()
                    && !this.crop_editing
                    && let Ok(plan) = this.recipe(true, cx)
                {
                    let mut lines = Vec::new();
                    match plan.split {
                        Some(plan::Split::Height { height: tile }) => {
                            for y in (tile..height as u32).step_by(tile as usize) {
                                lines.push((0., y as f32, width, y as f32));
                            }
                        }
                        Some(plan::Split::Grid { rows, columns }) => {
                            for row in 1..rows {
                                let y = (height as u32 * row / rows) as f32;
                                lines.push((0., y, width, y));
                            }
                            for col in 1..columns {
                                let x = (width as u32 * col / columns) as f32;
                                lines.push((x, 0., x, height));
                            }
                        }
                        None => {}
                    }
                    for (left, top, right, bottom) in lines {
                        let mut path = PathBuilder::stroke(px(1.5));
                        path.move_to(point(px(x + left * scale), px(y + top * scale)));
                        path.line_to(point(px(x + right * scale), px(y + bottom * scale)));
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(0x3b82f6));
                        }
                    }
                }
                let rect = if let Some((from, to)) = this.drag {
                    Some((
                        from.x.min(to.x),
                        from.y.min(to.y),
                        from.x.max(to.x),
                        from.y.max(to.y),
                    ))
                } else {
                    this.selected_annotation.as_ref().and_then(|id| {
                        this.edits()?
                            .annotations
                            .iter()
                            .find(|a| &a.id == id)
                            .map(crate::tool_editor::bounds)
                    })
                };
                if let Some((left, top, right, bottom)) = rect {
                    let mut path = PathBuilder::stroke(px(1.5));
                    path.move_to(point(px(x + left * scale), px(y + top * scale)));
                    path.line_to(point(px(x + right * scale), px(y + top * scale)));
                    path.line_to(point(px(x + right * scale), px(y + bottom * scale)));
                    path.line_to(point(px(x + left * scale), px(y + bottom * scale)));
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgb(0x3b82f6));
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full();
        div()
            .id("tool-preview-canvas")
            .relative()
            .flex_1()
            .min_h(px(180.))
            .w_full()
            .overflow_hidden()
            .rounded(px(9.))
            .border_1()
            .border_color(crate::theme::color(BORDER))
            .bg(crate::theme::color(CANVAS))
            .track_focus(&self.focus)
            .when_some(self.preview.as_ref(), |view, preview| {
                view.child(
                    img(preview.lease.path.clone())
                        .image_cache(&self.thumbnails)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                )
            })
            .child(overlay)
            .when(self.preview_busy, |view| {
                view.child(
                    div()
                        .absolute()
                        .right(px(9.))
                        .top(px(9.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(5.))
                        .bg(crate::theme::color(CARD))
                        .child(label("正在预览…", 11., MUTED)),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event, window, cx| this.pointer_down(event, window, cx)),
            )
            .on_mouse_move(cx.listener(|this, event, _, cx| this.pointer_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.pointer_up(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.pointer_up(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                let modifiers = event.keystroke.modifiers;
                if (modifiers.platform || modifiers.control) && key == "z" {
                    if let Some(edits) = this.edits_mut() {
                        if modifiers.shift {
                            edits.redo();
                        } else {
                            edits.undo();
                        }
                    }
                    this.schedule_preview(cx);
                    cx.stop_propagation();
                } else if let Some(id) = this.selected_annotation.clone() {
                    let step = if modifiers.shift { 10. } else { 1. };
                    let offset = match key {
                        "left" => Some((-step, 0.)),
                        "right" => Some((step, 0.)),
                        "up" => Some((0., -step)),
                        "down" => Some((0., step)),
                        _ => None,
                    };
                    if let Some(edits) = this.edits_mut() {
                        if let Some((dx, dy)) = offset {
                            edits.move_annotation(&id, dx, dy);
                        } else if matches!(key, "backspace" | "delete") {
                            let annotations = edits
                                .annotations
                                .iter()
                                .filter(|a| a.id != id)
                                .cloned()
                                .collect();
                            edits.replace(annotations);
                        } else {
                            return;
                        }
                    }
                    this.schedule_preview(cx);
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }
}
