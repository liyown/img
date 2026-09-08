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
        if self.busy || self.preview_busy || !self.crop_editing {
            return;
        }
        let Some(point) = self.point_on_image(event.position, false) else {
            return;
        };
        self.focus.focus(window, cx);
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
        }
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
                let Some((x, y, _, _, scale)) = this.image_geometry() else {
                    return;
                };
                let rect = this.drag.map(|(from, to)| {
                    (
                        from.x.min(to.x),
                        from.y.min(to.y),
                        from.x.max(to.x),
                        from.y.max(to.y),
                    )
                });
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
            .min_h_0()
            .w_full()
            .overflow_hidden()
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
                if event.keystroke.key == "escape" && this.crop_editing {
                    this.crop_editing = false;
                    this.drag = None;
                    this.schedule_preview(cx);
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }
}
