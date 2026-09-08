//! Reconstructible device-local files. Leases protect active readers across processes.
use crate::catalog::{Catalog, digest};
use anyhow::{Result, ensure};
use rusqlite::params;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub const DEFAULT_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
pub struct CacheLease {
    pub path: PathBuf,
    _file: File,
}
pub struct Cache {
    catalog: Catalog,
}
impl Cache {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root.join("cache"))?;
        Ok(Self {
            catalog: Catalog::open(root)?,
        })
    }
    pub fn limit(&self) -> Result<u64> {
        Ok(self
            .catalog
            .setting("cache-limit")?
            .map(|s| s.parse())
            .transpose()?
            .unwrap_or(DEFAULT_LIMIT))
    }
    pub fn set_limit(&self, limit: u64) -> Result<()> {
        ensure!(
            (16 * 1024 * 1024..=1024 * 1024 * 1024 * 1024).contains(&limit),
            "cache limit must be between 16 MiB and 1 TiB"
        );
        self.catalog.set_setting("cache-limit", &limit.to_string())
    }
    fn file(&self, key: &str) -> Result<PathBuf> {
        ensure!(
            key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid cache key"
        );
        Ok(self.catalog.root.join("cache").join(key))
    }
    fn lock(&self) -> Result<File> {
        let f = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.catalog.root.join("cache.lock"))?;
        f.lock()?;
        Ok(f)
    }
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let _lock = self.lock()?;
        let key = digest(bytes);
        let path = self.file(&key)?;
        if path.exists() {
            ensure!(
                std::fs::symlink_metadata(&path)?.is_file(),
                "cache entry is not a regular file"
            );
            ensure!(
                digest(&std::fs::read(&path)?) == key,
                "cache content is damaged"
            );
        } else {
            let mut tmp = tempfile::NamedTempFile::new_in(self.catalog.root.join("cache"))?;
            tmp.write_all(bytes)?;
            tmp.as_file().sync_all()?;
            tmp.persist_noclobber(&path).map_err(|e| e.error)?;
        }
        self.catalog.db.execute("INSERT INTO cache(key,path,size,accessed) VALUES(?,?,?,?) ON CONFLICT(key) DO UPDATE SET accessed=excluded.accessed",params![key,key,bytes.len() as i64,now()])?;
        Ok(key)
    }
    pub fn lease(&self, key: &str) -> Result<CacheLease> {
        let _lock = self.lock()?;
        let path = self.file(key)?;
        ensure!(
            std::fs::symlink_metadata(&path)?.is_file(),
            "cache entry is not a regular file"
        );
        let file = File::open(&path)?;
        file.lock_shared()?;
        self.catalog.db.execute(
            "UPDATE cache SET accessed=? WHERE key=?",
            params![now(), key],
        )?;
        Ok(CacheLease { path, _file: file })
    }
    pub fn touch_legacy(&self, path: &Path) -> Result<()> {
        crate::legacy_cache::touch(&self.catalog, path)
    }
    pub fn usage(&self) -> Result<(u64, u64)> {
        let _lock = self.lock()?;
        let current: u64 =
            self.catalog
                .db
                .query_row("SELECT coalesce(sum(size),0) FROM cache", [], |r| {
                    crate::catalog::unsigned(r, 0)
                })?;
        let legacy = crate::legacy_cache::entries(&self.catalog)?;
        Ok((
            current + legacy.iter().map(|e| e.size).sum::<u64>(),
            legacy.iter().filter(|e| e.protected).map(|e| e.size).sum(),
        ))
    }
    /// Clear uses the same LRU path with a zero limit; leased entries are skipped.
    pub fn trim(&self, limit: u64) -> Result<u64> {
        let _lock = self.lock()?;
        let mut stmt = self
            .catalog
            .db
            .prepare("SELECT key,size,accessed FROM cache ORDER BY accessed,key")?;
        let mut entries = stmt
            .query_map([], |r| {
                Ok((
                    Some(r.get::<_, String>(0)?),
                    crate::catalog::unsigned(r, 1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(key, size, accessed)| {
                Ok((
                    self.file(key.as_ref().unwrap())?,
                    key,
                    size,
                    accessed,
                    false,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        entries.extend(
            crate::legacy_cache::entries(&self.catalog)?
                .into_iter()
                .map(|e| (e.path, None, e.size, e.accessed, e.protected)),
        );
        entries.sort_by(|a, b| a.3.cmp(&b.3).then(a.0.cmp(&b.0)));
        let mut total: u64 = entries.iter().map(|e| e.2).sum();
        let mut removed = 0;
        for (path, key, size, _, protected) in entries {
            if total <= limit {
                break;
            }
            if protected {
                continue;
            }
            let freed = match File::options().read(true).write(true).open(&path) {
                Ok(file) => {
                    if file.try_lock().is_err() {
                        continue;
                    }
                    ensure!(
                        std::fs::symlink_metadata(&path)?.is_file(),
                        "cache entry is not a regular file"
                    );
                    std::fs::remove_file(&path)?;
                    size
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
                Err(e) => return Err(e.into()),
            };
            if let Some(key) = key {
                self.catalog
                    .db
                    .execute("DELETE FROM cache WHERE key=?", [key])?;
            }
            total = total.saturating_sub(size);
            removed += freed;
        }
        Ok(removed)
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clearing_cache_preserves_remote_records_and_active_tasks() {
        let root = tempfile::tempdir().unwrap();
        let cache = Cache::open(root.path()).unwrap();
        let a = cache.put(b"first").unwrap();
        cache.put(b"second").unwrap();
        let lease = cache.lease(&a).unwrap();
        assert_eq!(cache.trim(0).unwrap(), 6);
        assert!(lease.path.exists());
        drop(lease);
        assert_eq!(cache.trim(0).unwrap(), 5);
        assert!(root.path().join("catalog.sqlite3").exists());
        assert!(cache.lease("../../secret").is_err());
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::*;
    #[test]
    fn cache_clear_includes_imported_app_copies_and_preserves_active_and_user_files() {
        let root = tempfile::tempdir().unwrap();
        let done = uuid::Uuid::new_v4().to_string();
        let active = uuid::Uuid::new_v4().to_string();
        let original = root.path().join("user-original.png");
        std::fs::write(&original, b"user bytes").unwrap();
        let mut rows = vec![];
        for (id, status) in [(&done, "Done"), (&active, "Running")] {
            let dir = root.path().join("images").join(id).join("original");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("image.png"), b"app bytes").unwrap();
            rows.push(serde_json::json!({"id":id,"status":status,"source":original,"url":"https://example.test/image.png","target":"test","name":"image.png"}));
        }
        std::fs::write(
            root.path().join("queue.json"),
            serde_json::to_vec(&rows).unwrap(),
        )
        .unwrap();
        let mut catalog = Catalog::open(root.path()).unwrap();
        catalog.import_legacy().unwrap();
        let count = catalog.query(&Default::default()).unwrap().total;
        let cache = Cache::open(root.path()).unwrap();
        assert_eq!(cache.usage().unwrap(), (18, 9));
        let done_file = root
            .path()
            .join("images")
            .join(&done)
            .join("original/image.png");
        let lease = File::open(&done_file).unwrap();
        lease.lock_shared().unwrap();
        assert_eq!(cache.trim(0).unwrap(), 0);
        drop(lease);
        assert_eq!(cache.trim(0).unwrap(), 9);
        assert!(!done_file.exists());
        assert!(
            root.path()
                .join("images")
                .join(active)
                .join("original/image.png")
                .exists()
        );
        assert_eq!(std::fs::read(original).unwrap(), b"user bytes");
        assert_eq!(catalog.query(&Default::default()).unwrap().total, count);
        assert_eq!(
            std::fs::read(root.path().join("queue.json")).unwrap(),
            serde_json::to_vec(&rows).unwrap()
        );
    }
    #[cfg(unix)]
    #[test]
    fn cache_inventory_never_follows_legacy_directory_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let file = other.path().join("preview.png");
        std::fs::write(&file, b"outside").unwrap();
        std::fs::create_dir(root.path().join("images")).unwrap();
        std::os::unix::fs::symlink(
            other.path(),
            root.path()
                .join("images")
                .join(uuid::Uuid::new_v4().to_string()),
        )
        .unwrap();
        let cache = Cache::open(root.path()).unwrap();
        assert_eq!(cache.trim(0).unwrap(), 0);
        assert_eq!(std::fs::read(file).unwrap(), b"outside");
    }
}
