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
    /// Clear uses the same LRU path with a zero limit; leased entries are skipped.
    pub fn trim(&self, limit: u64) -> Result<u64> {
        let _lock = self.lock()?;
        let mut stmt = self
            .catalog
            .db
            .prepare("SELECT key,size FROM cache ORDER BY accessed,key")?;
        let entries = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, crate::catalog::unsigned(r, 1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut total: u64 = entries.iter().map(|(_, s)| s).sum();
        let mut removed = 0;
        for (key, size) in entries {
            if total <= limit {
                break;
            }
            let path = self.file(&key)?;
            match File::options().read(true).write(true).open(&path) {
                Ok(file) => {
                    if file.try_lock().is_err() {
                        continue;
                    }
                    ensure!(
                        std::fs::symlink_metadata(&path)?.is_file(),
                        "cache entry is not a regular file"
                    );
                    std::fs::remove_file(&path)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            self.catalog
                .db
                .execute("DELETE FROM cache WHERE key=?", [key])?;
            total = total.saturating_sub(size);
            removed += size;
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
