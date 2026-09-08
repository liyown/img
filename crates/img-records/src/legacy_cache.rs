//! Enumerate only files the old desktop importer owned. Never follow symlinks or record paths.
use crate::catalog::{Catalog, digest};
use anyhow::{Result, ensure};
use rusqlite::OptionalExtension;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
pub(crate) struct Entry {
    pub path: PathBuf,
    pub size: u64,
    pub accessed: i64,
    pub protected: bool,
}
pub(crate) fn entries(catalog: &Catalog) -> Result<Vec<Entry>> {
    let images = catalog.root.join("images");
    if !images.exists() {
        return Ok(vec![]);
    }
    ensure!(
        images.symlink_metadata()?.is_dir(),
        "legacy cache directory is not a regular directory"
    );
    let queue = catalog.root.join("queue.json");
    let rows: Vec<serde_json::Value> = if queue.exists() {
        serde_json::from_slice(&std::fs::read(queue)?)?
    } else {
        vec![]
    };
    let mut completed = BTreeMap::new();
    for row in rows {
        let Some(id) = row["id"]
            .as_str()
            .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        else {
            continue;
        };
        let imported = catalog
            .db
            .query_row(
                "SELECT 1 FROM imported WHERE source=?",
                [format!("legacy:{id}")],
                |r| r.get::<_, u8>(0),
            )
            .optional()?
            .is_some();
        let safe = row["status"] == "Done" && row["simulated"] != true && imported;
        completed
            .entry(id.to_string())
            .and_modify(|all| *all &= safe)
            .or_insert(safe);
    }
    let mut entries = vec![];
    for dir in std::fs::read_dir(&images)? {
        let dir = dir?;
        let id = dir.file_name().to_string_lossy().into_owned();
        if uuid::Uuid::parse_str(&id).is_err() || !dir.file_type()?.is_dir() {
            continue;
        }
        let protected = !completed.get(&id).copied().unwrap_or(false);
        for name in ["preview.png", "thumbnail-512.png"] {
            add(catalog, &dir.path().join(name), protected, &mut entries)?;
        }
        let originals = dir.path().join("original");
        if originals.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            for file in std::fs::read_dir(originals)? {
                add(catalog, &file?.path(), protected, &mut entries)?;
            }
        }
    }
    Ok(entries)
}
fn add(catalog: &Catalog, path: &Path, protected: bool, entries: &mut Vec<Entry>) -> Result<()> {
    let Ok(meta) = path.symlink_metadata() else {
        return Ok(());
    };
    if !meta.is_file() {
        return Ok(());
    }
    let accessed = catalog
        .setting(&access_key(catalog, path))?
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });
    entries.push(Entry {
        path: path.into(),
        size: meta.len(),
        accessed,
        protected,
    });
    Ok(())
}
fn access_key(catalog: &Catalog, path: &Path) -> String {
    format!(
        "legacy-cache-access:{}",
        digest(
            path.strip_prefix(&catalog.root)
                .unwrap_or(path)
                .to_string_lossy()
                .as_bytes()
        )
    )
}
pub(crate) fn touch(catalog: &Catalog, path: &Path) -> Result<()> {
    ensure!(
        path.starts_with(catalog.root.join("images")),
        "not an application cache path"
    );
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();
    catalog.set_setting(&access_key(catalog, path), &now.to_string())
}
