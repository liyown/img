use crate::model::{self, Item, Status};
use anyhow::Result;
use std::{collections::HashSet, path::Path};

pub fn prepare(root: &Path, known: &HashSet<String>) -> Result<(Vec<Item>, Vec<String>)> {
    let records = img_records::pending(root)?;
    let mut items = vec![];
    let mut acknowledged = vec![];
    for record in records.into_iter().take(50) {
        if !known.contains(&record.id) {
            let bytes = std::fs::read(img_records::image_path(root, &record.id)?)?;
            let mut item = model::prepare_bytes_with_limit(
                bytes,
                &record.name,
                root,
                &record.provider,
                128 << 20,
            )?;
            item.status = Status::Done;
            item.url = Some(record.url);
            item.progress = Some(100);
            item.uploaded_size = Some(record.size);
            item.added_at = record.created_at;
            item.origin = record.origin;
            item.imported_record_id = Some(record.id.clone());
            if let Some(source) = &item.source {
                std::fs::File::open(source)?.sync_all()?;
            }
            items.push(item);
        }
        acknowledged.push(record.id);
    }
    Ok((items, acknowledged))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_records_are_acknowledged_without_reimporting() {
        let root = tempfile::tempdir().unwrap();
        let record = img_records::Record {
            id: String::new(),
            name: "test.png".into(),
            provider: "test".into(),
            url: "https://example.test/a.png".into(),
            remote_path: "a.png".into(),
            content_type: "image/png".into(),
            size: 0,
            origin: "editor".into(),
            created_at: 42,
        };
        let mut png = std::io::Cursor::new(vec![]);
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let id = img_records::publish(root.path(), record, &png.into_inner()).unwrap();
        let (items, ids) = prepare(root.path(), &HashSet::new()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].origin, "editor");
        assert_eq!(items[0].status, Status::Done);
        assert!(items[0].source.as_ref().unwrap().exists());
        assert_eq!(ids.as_slice(), std::slice::from_ref(&id));
        assert_eq!(img_records::pending(root.path()).unwrap().len(), 1);
        let (reimported, _) = prepare(root.path(), &HashSet::from([id.clone()])).unwrap();
        assert!(reimported.is_empty());
        model::save(root.path(), &items).unwrap();
        img_records::acknowledge(root.path(), &id).unwrap();
        assert!(img_records::pending(root.path()).unwrap().is_empty());
        assert!(items[0].source.as_ref().unwrap().exists());
    }
}
