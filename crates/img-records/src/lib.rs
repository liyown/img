//! Cross-process upload inbox. Each immutable directory is published atomically;
//! desktop queue ownership stays with the desktop process.
pub mod backup;
pub mod cache;
pub mod catalog;
pub mod credentials;
pub mod files;
pub mod migration;
pub mod sync;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub url: String,
    pub remote_path: String,
    pub content_type: String,
    pub size: u64,
    pub origin: String,
    pub created_at: u64,
}

pub fn data_dir() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("IMG_DATA_DIR") {
        ensure!(!root.is_empty(), "IMG_DATA_DIR must not be empty");
        return Ok(root.into());
    }
    Ok(directories::BaseDirs::new()
        .context("cannot locate user data directory")?
        .data_local_dir()
        .join("aperture"))
}

fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

pub fn publish(root: &Path, mut record: Record, image: &[u8]) -> Result<String> {
    let inbox = root.join("upload-inbox");
    std::fs::create_dir_all(&inbox)?;
    let temp = tempfile::tempdir_in(&inbox)?;
    record.id = uuid::Uuid::new_v4().to_string();
    record.size = image.len() as u64;
    for (name, data) in [
        ("image", image.to_vec()),
        ("record.json", serde_json::to_vec(&record)?),
    ] {
        let mut file = std::fs::File::create(temp.path().join(name))?;
        file.write_all(&data)?;
        file.sync_all()?;
    }
    std::fs::rename(temp.path(), inbox.join(&record.id))?;
    Ok(record.id)
}

pub fn pending(root: &Path) -> Result<Vec<Record>> {
    let inbox = root.join("upload-inbox");
    if !inbox.exists() {
        return Ok(vec![]);
    }
    let mut records = vec![];
    for entry in std::fs::read_dir(inbox)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if !valid_id(&id) || !entry.file_type()?.is_dir() {
            continue;
        }
        let record: Record =
            serde_json::from_slice(&std::fs::read(entry.path().join("record.json"))?)
                .context("upload inbox contains a damaged record; original retained")?;
        ensure!(record.id == id, "upload inbox record identity mismatch");
        records.push(record);
    }
    records.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    Ok(records)
}

pub fn image_path(root: &Path, id: &str) -> Result<PathBuf> {
    ensure!(valid_id(id), "invalid record ID");
    Ok(root.join("upload-inbox").join(id).join("image"))
}

/// Only acknowledge after the receiving queue and its image copy are durable.
pub fn acknowledge(root: &Path, id: &str) -> Result<()> {
    let path = image_path(root, id)?.parent().unwrap().to_owned();
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Record {
        Record {
            id: String::new(),
            name: "a.png".into(),
            provider: "test".into(),
            url: "https://example.test/a.png".into(),
            remote_path: "a.png".into(),
            content_type: "image/png".into(),
            size: 0,
            origin: "cli".into(),
            created_at: 1,
        }
    }
    #[test]
    fn concurrent_publications_survive_until_acknowledged() {
        let root = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let root = root.path();
                scope.spawn(move || publish(root, record(), b"image").unwrap());
            }
        });
        let records = pending(root.path()).unwrap();
        assert_eq!(records.len(), 16);
        for record in records {
            assert_eq!(
                std::fs::read(image_path(root.path(), &record.id).unwrap()).unwrap(),
                b"image"
            );
            acknowledge(root.path(), &record.id).unwrap();
        }
        assert!(pending(root.path()).unwrap().is_empty());
    }
    #[test]
    fn interrupted_staging_and_path_escape_are_not_consumed() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("upload-inbox/.tmp-partial")).unwrap();
        assert!(pending(root.path()).unwrap().is_empty());
        assert!(acknowledge(root.path(), "../outside").is_err());
    }
}

mod sync_catalog;

pub mod remote_lock;
