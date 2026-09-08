//! Session-only library selection, independent of search and virtualized cells.
use crate::{
    model::{Item, Status},
    preferences::CopyFormat,
};
use std::collections::HashSet;

#[derive(Default)]
pub struct Selection {
    pub active: bool,
    ids: HashSet<String>,
    revision: Option<u64>,
}
impl Selection {
    pub fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
    pub fn hidden(&self, visible: impl Fn(&str) -> bool) -> usize {
        self.ids.iter().filter(|id| !visible(id)).count()
    }
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn toggle(&mut self, id: String) {
        if !self.ids.remove(&id) {
            self.ids.insert(id);
        }
    }
    pub fn extend(&mut self, ids: &[String]) {
        self.ids.extend(ids.iter().cloned());
    }
    pub fn clear(&mut self) {
        self.ids.clear();
    }
    pub fn finish(&mut self) {
        self.clear();
        self.active = false;
    }
    pub fn reconcile(&mut self, items: &[Item], revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        let valid: HashSet<_> = items
            .iter()
            .filter(|i| i.status == Status::Done)
            .map(|i| i.id.as_str())
            .collect();
        self.ids.retain(|id| valid.contains(id.as_str()));
    }
    pub fn snapshot(&self, items: &[Item]) -> Vec<String> {
        items
            .iter()
            .filter(|i| self.contains(&i.id) && i.status == Status::Done)
            .map(|i| i.id.clone())
            .collect()
    }
}

pub fn copy_text(items: &[Item], ids: &[String], format: CopyFormat) -> (String, usize, usize) {
    let ids: HashSet<_> = ids.iter().collect();
    let lines: Vec<_> = items
        .iter()
        .filter(|i| ids.contains(&i.id))
        .filter_map(|i| {
            i.url
                .as_ref()
                .filter(|url| !url.trim().is_empty())
                .map(|url| format.render(&i.name, url))
        })
        .collect();
    let count = lines.len();
    (lines.join("\n"), count, ids.len().saturating_sub(count))
}

/// Stable IDs and a cached intersection make single-item selection independent of catalog size.
#[derive(Default)]
pub struct Ids {
    ids: HashSet<String>,
    visible: HashSet<String>,
    visible_count: usize,
}
impl Ids {
    pub fn reconcile(&mut self, visible: &HashSet<String>, valid: &HashSet<String>) {
        self.ids.retain(|id| valid.contains(id));
        self.visible = visible.clone();
        self.visible_count = self.ids.intersection(&self.visible).count();
    }
    pub fn toggle(&mut self, id: String) {
        if self.ids.remove(&id) {
            self.visible_count -= usize::from(self.visible.contains(&id));
        } else {
            self.visible_count += usize::from(self.visible.contains(&id));
            self.ids.insert(id);
        }
    }
    pub fn select_visible(&mut self) {
        self.ids.extend(self.visible.iter().cloned());
        self.visible_count = self.visible.len();
    }
    pub fn clear(&mut self) {
        self.ids.clear();
        self.visible_count = 0;
    }
    pub fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
    pub fn hidden(&self) -> usize {
        self.ids.len() - self.visible_count
    }
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.ids.iter()
    }
    pub fn snapshot(&self) -> HashSet<String> {
        self.ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn items() -> Vec<Item> {
        let mut items = Item::reference_items();
        for (n, i) in items.iter_mut().enumerate() {
            i.status = Status::Done;
            i.url = Some(format!("https://example.com/{n}"));
        }
        items
    }
    #[test]
    fn selection_survives_search_and_appends_all_results() {
        let items = items();
        let mut selection = Selection {
            active: true,
            ..Default::default()
        };
        selection.toggle(items[0].id.clone());
        selection.extend(&[items[1].id.clone()]);
        selection.extend(&[items[1].id.clone()]);
        assert_eq!(selection.len(), 2);
        assert_eq!(selection.hidden(|id| id == items[1].id), 1);
        assert_eq!(selection.hidden(|_| false), 2);
        assert_eq!(
            selection.snapshot(&items),
            items.iter().map(|i| i.id.clone()).collect::<Vec<_>>()
        );
        selection.reconcile(&items[1..], 1);
        assert!(!selection.contains(&items[0].id));
        selection.toggle(items[1].id.clone());
        assert_eq!(selection.len(), 0);
        selection.extend(&[items[1].id.clone()]);
        selection.clear();
        assert!(selection.active);
        selection.finish();
        assert!(!selection.active);
        assert_eq!(selection.len(), 0);
    }
    #[test]
    fn copies_in_record_order_in_every_format_and_counts_missing_links() {
        let mut items = items();
        let ids = vec![items[1].id.clone(), items[0].id.clone()];
        for format in CopyFormat::ALL {
            let (text, copied, skipped) = copy_text(&items, &ids, format);
            assert_eq!(copied, 2);
            assert_eq!(skipped, 0);
            assert_eq!(
                text,
                items
                    .iter()
                    .map(|i| format.render(&i.name, i.url.as_ref().unwrap()))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        items[0].url = None;
        assert_eq!(copy_text(&items, &ids, CopyFormat::Url).1, 1);
        assert_eq!(copy_text(&items, &ids, CopyFormat::Url).2, 1);
    }
}

#[cfg(test)]
mod id_tests {
    use super::*;
    fn set(values: &[&str]) -> HashSet<String> {
        values.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn selection_survives_search_appends_all_and_prunes_invalid_ids() {
        let mut s = Ids::default();
        let valid = set(&["a", "b", "c"]);
        s.reconcile(&set(&["a", "b"]), &valid);
        s.toggle("a".into());
        s.reconcile(&set(&["b", "c"]), &valid);
        assert_eq!(s.hidden(), 1);
        s.select_visible();
        assert_eq!(s.len(), 3);
        assert_eq!(s.hidden(), 1);
        s.toggle("b".into());
        assert_eq!(s.hidden(), 1);
        s.reconcile(&set(&[]), &set(&["c"]));
        assert_eq!(s.snapshot(), set(&["c"]));
        assert_eq!(s.hidden(), 1);
        s.clear();
        assert_eq!(s.hidden(), 0);
        assert!(s.is_empty());
    }
}
