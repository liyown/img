use img_records::processing::{Annotation, Point, Shape};
#[derive(Clone, Default)]
pub struct Edits {
    pub annotations: Vec<Annotation>,
    undo: Vec<Vec<Annotation>>,
    redo: Vec<Vec<Annotation>>,
}
impl Edits {
    pub fn replace(&mut self, annotations: Vec<Annotation>) {
        if self.annotations == annotations {
            return;
        }
        self.undo
            .push(std::mem::replace(&mut self.annotations, annotations));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.annotations, previous));
        }
    }
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.annotations, next));
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn move_annotation(&mut self, id: &str, dx: f32, dy: f32) {
        let mut annotations = self.annotations.clone();
        if let Some(annotation) = annotations.iter_mut().find(|a| a.id == id) {
            translate(annotation, dx, dy);
            self.replace(annotations);
        }
    }
}
pub fn translate(annotation: &mut Annotation, dx: f32, dy: f32) {
    let shift = |p: &mut Point| {
        p.x += dx;
        p.y += dy;
    };
    match &mut annotation.shape {
        Shape::Arrow { from, to } | Shape::Rectangle { from, to } | Shape::Redact { from, to } => {
            shift(from);
            shift(to);
        }
        Shape::Text { at, .. } | Shape::Step { at, .. } => shift(at),
    }
}
pub fn bounds(annotation: &Annotation) -> (f32, f32, f32, f32) {
    match &annotation.shape {
        Shape::Arrow { from, to } | Shape::Rectangle { from, to } | Shape::Redact { from, to } => (
            from.x.min(to.x),
            from.y.min(to.y),
            from.x.max(to.x),
            from.y.max(to.y),
        ),
        Shape::Text { at, text, size } => (
            at.x,
            at.y,
            at.x + text
                .chars()
                .map(|c| if c.is_ascii() { 0.6 } else { 1. })
                .sum::<f32>()
                * size,
            at.y + size * 1.3,
        ),
        Shape::Step { at, radius, .. } => {
            (at.x - radius, at.y - radius, at.x + radius, at.y + radius)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_history_moves_undoes_redoes_and_discards_old_redo() {
        let mut a = Edits::default();
        let b = Edits::default();
        a.replace(vec![Annotation {
            id: "a".into(),
            shape: Shape::Arrow {
                from: Point { x: 1., y: 2. },
                to: Point { x: 3., y: 4. },
            },
            color: [255; 4],
            stroke_width: 3.,
        }]);
        a.move_annotation("a", 10., 20.);
        assert_eq!(bounds(&a.annotations[0]), (11., 22., 13., 24.));
        a.undo();
        assert_eq!(bounds(&a.annotations[0]), (1., 2., 3., 4.));
        a.redo();
        assert_eq!(bounds(&a.annotations[0]), (11., 22., 13., 24.));
        a.undo();
        a.replace(vec![]);
        assert!(!a.can_redo());
        assert!(b.annotations.is_empty());
    }
}
