//! The shared, metadata-only library. Cache contents never establish remote availability.
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const FILTER: &str = r#"FROM assets a WHERE (?1 OR a.hidden=0)
            AND (?2='' OR instr(lower(a.name),lower(?2))>0 OR EXISTS(SELECT 1 FROM locations s WHERE s.asset_id=a.id AND instr(lower(s.url),lower(?2))>0))
            AND (?3='' OR a.content_type=?3) AND (?4='' OR a.origin=?4)
            AND (?5 IS NULL OR a.added_at>=?5) AND (?6 IS NULL OR a.added_at<=?6)
            AND EXISTS(SELECT 1 FROM locations l WHERE l.asset_id=a.id AND (?7='' OR l.provider=?7)
                AND (?8='' OR substr(l.path,1,length(?8))=?8) AND (?9='' OR l.availability=?9))"#;
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Location {
    pub id: String,
    pub asset_id: String,
    pub namespace: String,
    pub provider: String,
    pub path: Option<String>,
    pub url: String,
    pub version: String,
    pub availability: String,
    pub last_checked: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub added_at: u64,
    pub origin: String,
    pub content_hash: Option<String>,
    pub hidden: bool,
    pub preferred_location: Option<String>,
    pub locations: Vec<Location>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogQuery {
    pub text: String,
    pub provider: String,
    pub prefix: String,
    pub content_type: String,
    pub origin: String,
    pub availability: String,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub include_hidden: bool,
    pub limit: usize,
    pub offset: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub total: usize,
    pub assets: Vec<Asset>,
}

/// Only verified object keys belong in `path`. Legacy URLs must use None.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteRecord {
    pub namespace: String,
    pub provider: String,
    pub path: Option<String>,
    pub url: String,
    pub version: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub added_at: u64,
    pub origin: String,
    pub content_hash: Option<String>,
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn identity(parts: &[&str]) -> String {
    // Length-prefixing prevents namespace/key boundary collisions.
    let mut h = Sha256::new();
    for part in parts {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    format!("{:x}", h.finalize())
}
pub struct Catalog {
    pub(crate) db: Connection,
    pub root: PathBuf,
    _restore_guard: std::fs::File,
}
impl Catalog {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let restore_guard = std::fs::File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("catalog.lock"))?;
        restore_guard
            .try_lock_shared()
            .map_err(|_| anyhow::anyhow!("library restore is in progress"))?;
        let mut db = Connection::open(root.join("catalog.sqlite3"))?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", true)?;
        let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= SCHEMA_VERSION,
            "library was created by a newer img version"
        );
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        if version == 0 {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(r#"
                CREATE TABLE IF NOT EXISTS assets(
                    id TEXT PRIMARY KEY,name TEXT NOT NULL,content_type TEXT NOT NULL,size INTEGER NOT NULL,
                    added_at INTEGER NOT NULL,origin TEXT NOT NULL,content_hash TEXT,
                    hidden INTEGER NOT NULL DEFAULT 0,preferred_location TEXT);
                CREATE INDEX IF NOT EXISTS assets_date ON assets(added_at DESC,id);
                CREATE TABLE IF NOT EXISTS locations(
                    id TEXT PRIMARY KEY,asset_id TEXT NOT NULL REFERENCES assets(id),namespace TEXT NOT NULL,
                    provider TEXT NOT NULL,path TEXT,url TEXT NOT NULL,version TEXT NOT NULL,
                    availability TEXT NOT NULL DEFAULT 'unknown',last_checked INTEGER);
                CREATE INDEX IF NOT EXISTS locations_asset ON locations(asset_id);
                CREATE INDEX IF NOT EXISTS locations_provider ON locations(provider,path);
                CREATE TABLE IF NOT EXISTS versions(parent TEXT NOT NULL,child TEXT NOT NULL,recipe TEXT NOT NULL,
                    PRIMARY KEY(parent,child));
                CREATE TABLE IF NOT EXISTS imported(source TEXT PRIMARY KEY);
                CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY,kind TEXT NOT NULL,body TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS cache(key TEXT PRIMARY KEY,path TEXT NOT NULL,size INTEGER NOT NULL,
                    accessed INTEGER NOT NULL,pins INTEGER NOT NULL DEFAULT 0);
                PRAGMA user_version=1;
            "#)?;
            tx.commit()?;
        }
        let catalog = Self {
            db,
            root: root.to_owned(),
            _restore_guard: restore_guard,
        };
        catalog.sync_install_capture()?;
        Ok(catalog)
    }
    pub fn upsert(&mut self, record: &RemoteRecord) -> Result<String> {
        Self::validate_record(record)?;
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let id = Self::upsert_transaction(&tx, record)?;
        tx.commit()?;
        Ok(id)
    }
    fn validate_record(record: &RemoteRecord) -> Result<()> {
        ensure!(
            !record.namespace.is_empty() && !record.url.is_empty(),
            "remote namespace and URL are required"
        );
        ensure!(
            record.size <= i64::MAX as u64 && record.added_at <= i64::MAX as u64,
            "remote metadata exceeds supported range"
        );
        if let Some(hash) = &record.content_hash {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|v| v.is_ascii_hexdigit()),
                "invalid SHA-256"
            );
        }
        if let Some(path) = &record.path {
            ensure!(
                !path.is_empty()
                    && !path.starts_with('/')
                    && !path.contains('\\')
                    && !path.split('/').any(|v| v == ".." || v == "."),
                "invalid remote object path"
            );
        }
        Ok(())
    }
    fn upsert_transaction(tx: &rusqlite::Transaction<'_>, record: &RemoteRecord) -> Result<String> {
        let id = identity(&[
            &record.namespace,
            if record.path.is_some() { "key" } else { "url" },
            record.path.as_deref().unwrap_or(&record.url),
        ]);
        let mut asset_id = record
            .content_hash
            .as_ref()
            .map(|h| format!("sha256:{}", h.to_ascii_lowercase()))
            .unwrap_or_else(|| format!("remote:{}", identity(&[&id, &record.version])));
        let previous: Option<(String, String)> = tx
            .query_row(
                "SELECT asset_id,version FROM locations WHERE id=?",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        // An unchanged observed version does not discard a previously verified digest.
        if record.content_hash.is_none()
            && let Some((old, version)) = &previous
            && version == &record.version
        {
            asset_id = old.clone();
        }
        let previous = previous.map(|(id, _)| id);
        let inserted=tx.execute("INSERT INTO assets(id,name,content_type,size,added_at,origin,content_hash) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
            params![asset_id,record.name,record.content_type,record.size as i64,record.added_at as i64,record.origin,record.content_hash])?;
        if inserted > 0
            && let Some(old) = &previous
        {
            tx.execute("UPDATE assets SET hidden=(SELECT hidden FROM assets WHERE id=?1),preferred_location=(SELECT preferred_location FROM assets WHERE id=?1) WHERE id=?2",params![old,asset_id])?;
        }
        if let Some(previous) = previous.filter(|old| old != &asset_id) {
            // Discovering a digest promotes unknown metadata; changed versions remain related.
            tx.execute(
                "INSERT OR IGNORE INTO versions(parent,child,recipe) VALUES(?,?,?)",
                params![previous, asset_id, "remote-version"],
            )?;
        }
        tx.execute("INSERT INTO locations(id,asset_id,namespace,provider,path,url,version) VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET asset_id=excluded.asset_id,provider=excluded.provider,path=excluded.path,url=excluded.url,version=excluded.version,availability=CASE WHEN locations.version=excluded.version THEN locations.availability ELSE 'unknown' END,last_checked=CASE WHEN locations.version=excluded.version THEN locations.last_checked ELSE NULL END",
            params![id,asset_id,record.namespace,record.provider,record.path,record.url,record.version])?;
        Ok(asset_id)
    }
    /// Record a verified copy without rolling back a newer observation of its source.
    pub fn record_copy(
        &mut self,
        source: &crate::targets::Target,
        destination: &RemoteRecord,
    ) -> Result<String> {
        Self::validate_record(destination)?;
        let hash = destination
            .content_hash
            .as_ref()
            .context("verified copy requires a content hash")?;
        ensure!(
            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid copy hash"
        );
        if let Some(expected) = &source.content_hash {
            ensure!(expected.eq_ignore_ascii_case(hash), "copy content changed");
        }
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: Option<(String, String)> = tx
            .query_row(
                "SELECT asset_id,version FROM locations WHERE id=?",
                [&source.location.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if current.as_ref().is_some_and(|(id, version)| {
            id == &source.input_id && version == &source.location.version
        }) {
            let origin = RemoteRecord {
                namespace: source.location.namespace.clone(),
                provider: source.location.provider.clone(),
                path: source.location.path.clone(),
                url: source.location.url.clone(),
                version: source.location.version.clone(),
                name: source.name.clone(),
                content_type: destination.content_type.clone(),
                size: destination.size,
                added_at: tx.query_row(
                    "SELECT added_at FROM assets WHERE id=?",
                    [&source.input_id],
                    |row| row.get::<_, i64>(0),
                )? as u64,
                origin: "verified-copy".into(),
                content_hash: Some(hash.clone()),
            };
            Self::upsert_transaction(&tx, &origin)?;
        }
        let id = Self::upsert_transaction(&tx, destination)?;
        if id != source.input_id {
            tx.execute("INSERT OR IGNORE INTO versions(parent,child,recipe) SELECT ?,?, 'verified-copy' WHERE EXISTS(SELECT 1 FROM assets WHERE id=?)", rusqlite::params![source.input_id,id,source.input_id])?;
        }
        tx.commit()?;
        Ok(id)
    }
    pub fn finish_scan(
        &mut self,
        namespace: &str,
        prefix: &str,
        seen: &std::collections::BTreeSet<String>,
        checked: u64,
        excluded: &[String],
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        let entries = {
            let mut stmt=tx.prepare("SELECT id,path FROM locations WHERE namespace=?1 AND substr(path,1,length(?2))=?2 AND availability!='deleted'")?;
            stmt.query_map(params![namespace, prefix], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, path) in entries {
            if !seen.contains(&path)
                && !path.split('/').any(|part| part == ".img-sync")
                && !excluded
                    .iter()
                    .any(|p| !p.is_empty() && path.starts_with(p))
            {
                tx.execute(
                    "UPDATE locations SET availability='pending-missing',last_checked=? WHERE id=?",
                    params![checked as i64, id],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn locations(&self, id: &str) -> Result<Vec<Location>> {
        let mut stmt=self.db.prepare("SELECT id,asset_id,namespace,provider,path,url,version,availability,last_checked FROM locations WHERE asset_id=? ORDER BY id")?;
        Ok(stmt
            .query_map([id], |r| {
                Ok(Location {
                    id: r.get(0)?,
                    asset_id: r.get(1)?,
                    namespace: r.get(2)?,
                    provider: r.get(3)?,
                    path: r.get(4)?,
                    url: r.get(5)?,
                    version: r.get(6)?,
                    availability: r.get(7)?,
                    last_checked: r.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn get(&self, id: &str) -> Result<Asset> {
        let mut asset=self.db.query_row("SELECT id,name,content_type,size,added_at,origin,content_hash,hidden,preferred_location FROM assets WHERE id=?",[id],|r|Ok(Asset{id:r.get(0)?,name:r.get(1)?,content_type:r.get(2)?,size:unsigned(r,3)?,added_at:unsigned(r,4)?,origin:r.get(5)?,content_hash:r.get(6)?,hidden:r.get(7)?,preferred_location:r.get(8)?,locations:vec![]})).context("image not found in library")?;
        asset.locations = self.locations(id)?;
        Ok(asset)
    }
    pub fn query(&self, q: &CatalogQuery) -> Result<Page> {
        let condition = FILTER;
        let values = params![
            q.include_hidden,
            q.text,
            q.content_type,
            q.origin,
            q.since.map(i64::try_from).transpose()?,
            q.until.map(i64::try_from).transpose()?,
            q.provider,
            q.prefix,
            q.availability
        ];
        let total = self
            .db
            .query_row(&format!("SELECT count(*) {condition}"), values, |r| {
                r.get::<_, i64>(0).map(|v| v as usize)
            })?;
        let mut stmt = self.db.prepare(&format!(
            "SELECT a.id {condition} ORDER BY a.added_at DESC,a.id LIMIT ?10 OFFSET ?11"
        ))?;
        let ids = stmt
            .query_map(
                params![
                    q.include_hidden,
                    q.text,
                    q.content_type,
                    q.origin,
                    q.since.map(i64::try_from).transpose()?,
                    q.until.map(i64::try_from).transpose()?,
                    q.provider,
                    q.prefix,
                    q.availability,
                    if q.limit == 0 {
                        200i64
                    } else {
                        q.limit.min(1000) as i64
                    },
                    i64::try_from(q.offset)?
                ],
                |r| r.get::<_, String>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Page {
            total,
            assets: ids.iter().map(|id| self.get(id)).collect::<Result<_>>()?,
        })
    }
    pub fn query_ids(&self, q: &CatalogQuery) -> Result<Vec<String>> {
        let mut stmt = self.db.prepare(&format!(
            "SELECT a.id {FILTER} ORDER BY a.added_at DESC,a.id"
        ))?;
        Ok(stmt
            .query_map(
                params![
                    q.include_hidden,
                    q.text,
                    q.content_type,
                    q.origin,
                    q.since.map(i64::try_from).transpose()?,
                    q.until.map(i64::try_from).transpose()?,
                    q.provider,
                    q.prefix,
                    q.availability
                ],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn set_hidden_many(&mut self, ids: &[String], hidden: bool) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for id in ids {
            ensure!(
                tx.execute("UPDATE assets SET hidden=? WHERE id=?", params![hidden, id])? == 1,
                "image no longer exists"
            );
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_hidden(&self, id: &str, hidden: bool) -> Result<()> {
        ensure!(
            self.db
                .execute("UPDATE assets SET hidden=? WHERE id=?", params![hidden, id])?
                == 1,
            "image not found"
        );
        Ok(())
    }
    pub fn link_version(&self, parent: &str, child: &str, recipe: &str) -> Result<()> {
        ensure!(
            parent != child,
            "an image cannot be its own processing ancestor"
        );
        let tx = self.db.unchecked_transaction()?;
        self.get(parent)?;
        self.get(child)?;
        let cycle:bool=self.db.query_row("WITH RECURSIVE descendants(id) AS (SELECT child FROM versions WHERE parent=?1 UNION SELECT v.child FROM versions v JOIN descendants d ON v.parent=d.id) SELECT EXISTS(SELECT 1 FROM descendants WHERE id=?2)",params![child,parent],|r|r.get(0))?;
        ensure!(!cycle, "processing versions cannot form a cycle");
        self.db.execute("INSERT INTO versions(parent,child,recipe) VALUES(?,?,?) ON CONFLICT(parent,child) DO UPDATE SET recipe=excluded.recipe",params![parent,child,recipe])?;
        tx.commit()?;
        Ok(())
    }
    pub fn related_versions(&self, id: &str) -> Result<Vec<(Asset, String, bool)>> {
        let mut stmt=self.db.prepare("SELECT parent,child,recipe FROM versions WHERE parent=? OR child=? ORDER BY parent,child")?;
        let links = stmt
            .query_map(params![id, id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        links
            .into_iter()
            .map(|(parent, child, recipe)| {
                let ancestor = child == id;
                Ok((
                    self.get(if ancestor { &parent } else { &child })?,
                    recipe,
                    ancestor,
                ))
            })
            .collect()
    }
    pub fn set_preferred(&self, id: &str, location: &str) -> Result<()> {
        ensure!(
            self.locations(id)?.iter().any(|l| l.id == location),
            "address does not belong to image"
        );
        self.db.execute(
            "UPDATE assets SET preferred_location=? WHERE id=?",
            params![location, id],
        )?;
        Ok(())
    }
    pub fn mark_location(
        &self,
        id: &str,
        expected_version: &str,
        state: &str,
        checked: u64,
    ) -> Result<bool> {
        ensure!(
            [
                "unknown",
                "available",
                "missing",
                "pending-missing",
                "forbidden",
                "error",
                "deleted"
            ]
            .contains(&state),
            "invalid remote availability"
        );
        Ok(self.db.execute(
            "UPDATE locations SET availability=?,last_checked=? WHERE id=? AND version=?",
            params![state, i64::try_from(checked)?, id, expected_version],
        )? == 1)
    }
    pub fn relate(&self, parent: &str, child: &str, recipe: &str) -> Result<()> {
        ensure!(parent != child, "image cannot derive from itself");
        self.get(parent)?;
        self.get(child)?;
        self.db.execute(
            "INSERT OR IGNORE INTO versions(parent,child,recipe) VALUES(?,?,?)",
            params![parent, child, recipe],
        )?;
        Ok(())
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn settings_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let mut statement = self.db.prepare(
            "SELECT key,value FROM settings WHERE substr(key,1,length(?1))=?1 ORDER BY key",
        )?;
        Ok(statement
            .query_map([prefix], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute("INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?;
        Ok(())
    }
    pub fn save_task(&self, id: &str, kind: &str, body: &str) -> Result<()> {
        self.db.execute("INSERT INTO tasks(id,kind,body) VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![id,kind,body])?;
        Ok(())
    }
    pub fn tasks(&self, kind: &str) -> Result<Vec<(String, String, String)>> {
        let mut statement = self.db.prepare(
            "SELECT id,kind,body FROM tasks WHERE ?1='' OR kind=?1 ORDER BY rowid DESC LIMIT 100",
        )?;
        Ok(statement
            .query_map([kind], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn sync_entities(&self, prefix: &str) -> Result<Vec<String>> {
        self.sync_init()?;
        let mut statement=self.db.prepare("SELECT DISTINCT entity FROM sync_events WHERE substr(entity,1,length(?1))=?1 ORDER BY entity")?;
        Ok(statement
            .query_map([prefix], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn task_optional(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT body FROM tasks WHERE id=?", [id], |r| r.get(0))
            .optional()?)
    }
    pub fn task(&self, id: &str) -> Result<String> {
        Ok(self
            .db
            .query_row("SELECT body FROM tasks WHERE id=?", [id], |r| r.get(0))?)
    }
    pub fn snapshot(&self, destination: &Path) -> Result<()> {
        ensure!(!destination.exists(), "snapshot destination exists");
        self.db.backup("main", destination, None)?;
        Ok(())
    }
    /// Repeated upgrades are idempotent; never derive keys from URLs or cached bytes.
    pub fn versions(&self, id: &str) -> Result<Vec<(String, String, String)>> {
        let mut q=self.db.prepare("SELECT parent,child,recipe FROM versions WHERE parent=?1 OR child=?1 ORDER BY parent,child")?;
        Ok(
            q.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?,
        )
    }
    pub fn preview_key(&self, asset: &Asset) -> Result<Option<String>> {
        Ok(asset
            .content_hash
            .clone()
            .or(self.setting(&format!("preview:{}", asset.id))?))
    }
    pub fn import_legacy(&mut self) -> Result<usize> {
        let path = self.root.join("queue.json");
        if !path.exists() {
            return Ok(0);
        }
        let rows: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(path)?)?;
        let mut count = 0;
        let mut cache_dirty = false;
        for row in rows {
            if row["simulated"].as_bool().unwrap_or(false) || row["status"] != "Done" {
                continue;
            }
            let (Some(id), Some(url)) = (row["id"].as_str(), row["url"].as_str()) else {
                continue;
            };
            let marker = format!("legacy:{id}");
            if self
                .db
                .query_row("SELECT 1 FROM imported WHERE source=?", [&marker], |r| {
                    r.get::<_, u8>(0)
                })
                .optional()?
                .is_some()
            {
                continue;
            }
            cache_dirty = true;
            if row["catalog_asset_id"]
                .as_str()
                .is_some_and(|id| self.get(id).is_ok())
            {
                self.db.execute(
                    "INSERT OR IGNORE INTO imported(source) VALUES(?)",
                    [&marker],
                )?;
                continue;
            }
            if let Some(inbox) = row["imported_record_id"].as_str()
                && self.setting(&format!("inbox:{inbox}"))?.is_some()
            {
                self.db.execute(
                    "INSERT OR IGNORE INTO imported(source) VALUES(?)",
                    [&marker],
                )?;
                continue;
            }
            let provider = row["target"].as_str().unwrap_or_default();
            let asset_id = self.upsert(&RemoteRecord {
                namespace: format!("legacy:{provider}"),
                provider: provider.into(),
                path: None,
                url: url.into(),
                version: String::new(),
                name: row["name"].as_str().unwrap_or_default().into(),
                content_type: String::new(),
                size: row["uploaded_size"]
                    .as_u64()
                    .or_else(|| row["size"].as_u64())
                    .unwrap_or(0),
                added_at: row["added_at"].as_u64().unwrap_or(0),
                origin: row["origin"].as_str().unwrap_or_default().into(),
                content_hash: None,
            })?;
            // Legacy thumbnails remain useful previews, but their digest is not proof of remote content.
            if let Ok(cache_root) = self.root.join("images").canonicalize() {
                for field in ["thumbnail", "source"] {
                    if let Some(path) = row[field]
                        .as_str()
                        .and_then(|s| Path::new(s).canonicalize().ok())
                        && path.starts_with(&cache_root)
                        && std::fs::metadata(&path)
                            .is_ok_and(|m| m.is_file() && m.len() <= 20 << 20)
                        && let Ok(bytes) = std::fs::read(&path)
                        && let Ok(cache) = crate::cache::Cache::open(&self.root)
                        && let Ok(key) = cache.put(&bytes)
                    {
                        self.set_setting(&format!("preview:{asset_id}"), &key)?;
                        break;
                    }
                }
            }
            self.db
                .execute("INSERT OR IGNORE INTO imported(source) VALUES(?)", [marker])?;
            count += 1;
        }
        if cache_dirty {
            let cache = crate::cache::Cache::open(&self.root)?;
            cache.trim(cache.limit()?)?;
        }
        Ok(count)
    }
}
impl Asset {
    pub fn selected_location(&self, provider: &str) -> Option<&Location> {
        if !provider.is_empty() {
            return self.locations.iter().find(|l| l.provider == provider);
        }
        self.preferred_location
            .as_ref()
            .and_then(|id| self.locations.iter().find(|l| &l.id == id))
            .or_else(|| self.locations.first())
    }
}

pub(crate) fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(namespace: &str, path: &str) -> RemoteRecord {
        RemoteRecord {
            namespace: namespace.into(),
            provider: namespace.into(),
            path: Some(path.into()),
            url: format!("https://example.test/{namespace}/{path}"),
            version: "v1".into(),
            name: path.into(),
            content_type: "image/png".into(),
            size: 42,
            added_at: 1,
            origin: "test".into(),
            content_hash: None,
        }
    }
    #[test]
    fn namespaces_hashes_versions_and_status_are_independent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(tmp.path()).unwrap();
        let a = c.upsert(&record("one", "same.png")).unwrap();
        let b = c.upsert(&record("two", "same.png")).unwrap();
        assert_ne!(a, b);
        let mut r = record("three", "a.png");
        r.content_hash = Some(digest(b"same"));
        let known = c.upsert(&r).unwrap();
        r.namespace = "four".into();
        r.provider = "four".into();
        c.upsert(&r).unwrap();
        assert_eq!(c.get(&known).unwrap().locations.len(), 2);
        c.set_hidden(&known, true).unwrap();
        assert_eq!(c.query(&CatalogQuery::default()).unwrap().total, 2);
        let old = c.get(&a).unwrap().locations[0].clone();
        let mut r = record("one", "same.png");
        r.version = "v2".into();
        let next = c.upsert(&r).unwrap();
        assert_ne!(a, next);
        assert!(!c.mark_location(&old.id, "v1", "missing", 2).unwrap());
        assert_eq!(c.get(&next).unwrap().locations[0].availability, "unknown");
    }
    #[test]
    fn rescans_keep_verified_identity_and_excluded_scopes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(tmp.path()).unwrap();
        let mut r = record("one", "pictures/a.png");
        r.content_hash = Some(digest(b"pixels"));
        let id = c.upsert(&r).unwrap();
        c.set_hidden(&id, true).unwrap();
        r.content_hash = None;
        assert_eq!(c.upsert(&r).unwrap(), id);
        assert!(c.get(&id).unwrap().hidden);
        let excluded = c.upsert(&record("one", "private/a.png")).unwrap();
        let sync = c.upsert(&record("one", ".img-sync/a.png")).unwrap();
        c.finish_scan("one", "", &Default::default(), 10, &["private/".into()])
            .unwrap();
        assert_eq!(
            c.get(&id).unwrap().locations[0].availability,
            "pending-missing"
        );
        assert_eq!(
            c.get(&excluded).unwrap().locations[0].availability,
            "unknown"
        );
        assert_eq!(c.get(&sync).unwrap().locations[0].availability, "unknown");
    }
    #[test]
    fn concurrent_writers_and_paging_survive_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        Catalog::open(tmp.path()).unwrap();
        std::thread::scope(|s| {
            for n in 0..16 {
                let root = tmp.path();
                s.spawn(move || {
                    Catalog::open(root)
                        .unwrap()
                        .upsert(&record("test", &format!("{n}.png")))
                        .unwrap();
                });
            }
        });
        let c = Catalog::open(tmp.path()).unwrap();
        let p = c
            .query(&CatalogQuery {
                limit: 5,
                offset: 5,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(p.total, 16);
        assert_eq!(p.assets.len(), 5);
        c.snapshot(&tmp.path().join("copy.db")).unwrap();
        let copy = Connection::open(tmp.path().join("copy.db")).unwrap();
        assert_eq!(
            copy.query_row("SELECT count(*) FROM assets", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            16
        );
    }
    #[test]
    fn hundred_thousand_metadata_rows_page_without_loading_the_catalog() {
        let root = tempfile::tempdir().unwrap();
        let c = Catalog::open(root.path()).unwrap();
        c.db.execute_batch("BEGIN; INSERT INTO settings(key,value) VALUES('sync-applying','1'); WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO assets(id,name,content_type,size,added_at,origin) SELECT printf('a-%06d',x),printf('photo-%06d.png',x),'image/png',100,x,'remote' FROM n; INSERT INTO locations(id,asset_id,namespace,provider,path,url,version) SELECT id,id,'namespace','test','photos/'||name,'https://cdn.test/'||name,'v1' FROM assets; DELETE FROM settings WHERE key='sync-applying'; COMMIT;").unwrap();
        let start = std::time::Instant::now();
        let page = c
            .query(&CatalogQuery {
                provider: "test".into(),
                prefix: "photos/".into(),
                limit: 200,
                offset: 80000,
                ..Default::default()
            })
            .unwrap();
        println!(
            "100000 metadata rows: query page at offset 80000 in {:?}",
            start.elapsed()
        );
        assert_eq!(page.total, 100000);
        assert_eq!(page.assets.len(), 200);
        assert_eq!(page.assets[0].name, "photo-020000.png");
    }
    #[test]
    fn legacy_is_idempotent_and_never_guesses_a_remote_key() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("queue.json"),r#"[{"id":"old","status":"Done","target":"test","url":"https://example.test/a.png","name":"a.png","source":"/private/photo.png"}]"#).unwrap();
        let mut c = Catalog::open(tmp.path()).unwrap();
        assert_eq!(c.import_legacy().unwrap(), 1);
        assert_eq!(c.import_legacy().unwrap(), 0);
        let p = c.query(&CatalogQuery::default()).unwrap();
        assert!(p.assets[0].locations[0].path.is_none());
        assert!(!serde_json::to_string(&p).unwrap().contains("/private"));
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;
    #[test]
    fn processing_relationships_keep_unavailable_ancestors_and_reject_cycles() {
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(root.path()).unwrap();
        let mut record = RemoteRecord {
            namespace: "source".into(),
            provider: "source".into(),
            path: Some("one.png".into()),
            url: "https://source.test/one.png".into(),
            version: "v1".into(),
            name: "one.png".into(),
            content_type: "image/png".into(),
            size: 1,
            added_at: 1,
            origin: String::new(),
            content_hash: None,
        };
        let parent = c.upsert(&record).unwrap();
        record.path = Some("two.png".into());
        record.url = "https://source.test/two.png".into();
        let child = c.upsert(&record).unwrap();
        c.link_version(&parent, &child, "{\"format\":\"png\"}")
            .unwrap();
        assert!(c.link_version(&child, &parent, "loop").is_err());
        assert!(c.link_version(&parent, &parent, "loop").is_err());
        let related = c.related_versions(&child).unwrap();
        assert_eq!(related[0].0.id, parent);
        assert!(related[0].2);
        let parent_location = c.get(&parent).unwrap().locations[0].clone();
        c.mark_location(&parent_location.id, &parent_location.version, "deleted", 2)
            .unwrap();
        assert_eq!(
            c.related_versions(&child).unwrap()[0].0.locations[0].availability,
            "deleted"
        );
        assert!(
            c.set_preferred(&parent, &c.get(&child).unwrap().locations[0].id)
                .is_err()
        );
        let location = c.get(&child).unwrap().locations[0].id.clone();
        c.set_preferred(&child, &location).unwrap();
        assert_eq!(
            c.get(&child).unwrap().selected_location("").unwrap().id,
            location
        );
    }
}
