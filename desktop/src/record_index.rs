use crate::model::{Item, Status};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct RecordIndex {
    revision: Option<u64>,
    records: Vec<(String, String, Status)>,
    positions: HashMap<String, usize>,
    query: String,
    scope: u8,
    filter: u8,
    result: Arc<Vec<String>>,
}
impl RecordIndex {
    pub fn rows(
        &mut self,
        items: &[Item],
        revision: u64,
        query: &str,
        scope: u8,
        filter: u8,
    ) -> Arc<Vec<String>> {
        let changed = self.revision != Some(revision);
        if changed {
            self.records = items
                .iter()
                .map(|i| {
                    (
                        i.id.clone(),
                        format!("{} {} {}", i.name, i.target, i.url.as_deref().unwrap_or(""))
                            .to_lowercase(),
                        i.status,
                    )
                })
                .collect();
            self.positions = items
                .iter()
                .enumerate()
                .map(|(n, i)| (i.id.clone(), n))
                .collect();
            self.revision = Some(revision);
        }
        let query = query.to_lowercase();
        if changed || self.query != query || self.scope != scope || self.filter != filter {
            self.result = Arc::new(
                self.records
                    .iter()
                    .filter(|(_, text, status)| {
                        (query.is_empty() || text.contains(&query))
                            && match scope {
                                1 => *status == Status::Done,
                                2 => matches!(
                                    status,
                                    Status::Done | Status::Failed | Status::Cancelled
                                ),
                                _ => match filter {
                                    1 => *status == Status::Running,
                                    2 => *status == Status::Done,
                                    3 => *status == Status::Failed,
                                    _ => true,
                                },
                            }
                    })
                    .map(|(id, _, _)| id.clone())
                    .collect(),
            );
            self.query = query;
            self.scope = scope;
            self.filter = filter;
        }
        self.result.clone()
    }
    pub fn position(&self, id: &str) -> Option<usize> {
        self.positions.get(id).copied()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_ids_survive_progress_and_invalidate_for_data_changes() {
        let mut items = Item::reference_items();
        let mut index = RecordIndex::default();
        let first = index.rows(&items, 1, "", 0, 0);
        items[0].progress = Some(90);
        assert!(Arc::ptr_eq(&first, &index.rows(&items, 1, "", 0, 0)));
        let library = index.rows(&items, 1, "avatar", 1, 0);
        assert_eq!(library.as_ref(), &[items[1].id.clone()]);
        items.remove(0);
        let next = index.rows(&items, 2, "AVATAR", 1, 0);
        assert_eq!(index.position(&next[0]), Some(0));
        assert_eq!(next.len(), 1);
    }
}
