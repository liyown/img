//! Immutable field events. No image bytes, device paths or executable tasks are accepted.
use crate::catalog::Catalog;
use anyhow::{Context, Result, ensure};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub device: String,
    pub entity: String,
    pub field: String,
    pub parents: Vec<String>,
    pub value: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub version: u32,
    pub events: Vec<Event>,
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conflict {
    pub entity: String,
    pub field: String,
    pub candidates: Vec<Event>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entity {
    pub fields: BTreeMap<String, Value>,
    pub deleted: bool,
    pub conflicts: Vec<Conflict>,
}
impl Event {
    fn validate(&self) -> Result<()> {
        ensure!(
            uuid::Uuid::parse_str(&self.id).is_ok() && uuid::Uuid::parse_str(&self.device).is_ok(),
            "invalid sync event identity"
        );
        ensure!(
            self.parents.len() <= 256
                && self
                    .parents
                    .iter()
                    .all(|p| uuid::Uuid::parse_str(p).is_ok() && p != &self.id),
            "invalid causal parents"
        );
        let (kind, id) = self.entity.split_once(':').context("invalid sync entity")?;
        ensure!(
            !id.is_empty() && id.len() <= 1024,
            "invalid entity identity"
        );
        let allowed: &[&str] = match kind {
            "provider" => &[
                "name",
                "type",
                "endpoint",
                "region",
                "bucket",
                "public_url",
                "path_style",
                "owner",
                "repo",
                "branch",
                "token",
                "access_key",
                "secret_key",
                "session_token",
                "headers",
                "fields",
                "url",
                "method",
                "file_field",
                "url_json_path",
                "path_prefix",
                "commit_message",
                "allow_insecure",
            ],
            "asset" => &[
                "name",
                "content_type",
                "size",
                "added_at",
                "origin",
                "content_hash",
                "hidden",
                "preferred_location",
            ],
            "location" => &[
                "asset_id",
                "namespace",
                "provider",
                "path",
                "url",
                "version",
                "availability",
                "last_checked",
            ],
            "version" => &["parent", "child", "recipe"],
            "scope" => &["namespace", "provider", "prefix", "enabled"],
            "preset" => &["name", "plan"],
            _ => anyhow::bail!("entity type is not portable"),
        };
        ensure!(
            self.field == "$deleted" || allowed.contains(&self.field.as_str()),
            "field is not portable"
        );
        if kind == "provider"
            && matches!(
                self.field.as_str(),
                "token" | "access_key" | "secret_key" | "session_token" | "headers" | "fields"
            )
        {
            fn reference_only(value: &Value) -> bool {
                match value {
                    Value::Null => true,
                    Value::String(s) => s.is_empty() || s.contains("${"),
                    Value::Object(m) => m.values().all(reference_only),
                    _ => false,
                }
            }
            ensure!(
                self.value.as_ref().is_none_or(reference_only),
                "provider secrets must use portable credential references"
            );
        }
        if self.field == "$deleted" {
            ensure!(
                self.value == Some(Value::Bool(true)),
                "invalid deletion event"
            );
        }
        if ["preset", "version"].contains(&kind)
            && let Some(value) = &self.value
        {
            fn portable(v: &Value) -> bool {
                match v {
                    Value::Object(m) => m.iter().all(|(k, v)| {
                        !matches!(
                            k.as_str(),
                            "source"
                                | "output"
                                | "output_dir"
                                | "local_path"
                                | "watermark_path"
                                | "font_file"
                        ) && portable(v)
                    }),
                    Value::Array(a) => a.iter().all(portable),
                    _ => true,
                }
            }
            ensure!(portable(value), "preset includes a device-local reference");
        }
        if kind == "location"
            && self.field == "path"
            && let Some(Value::String(p)) = &self.value
        {
            ensure!(
                !p.starts_with('/')
                    && !p.contains('\\')
                    && !p.split('/').any(|v| v == ".." || v == "."),
                "invalid remote key"
            );
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 65536,
            "sync event too large"
        );
        Ok(())
    }
}
impl Catalog {
    pub fn sync_init(&self) -> Result<String> {
        self.db.execute_batch("CREATE TABLE IF NOT EXISTS sync_events(id TEXT PRIMARY KEY,device TEXT NOT NULL,entity TEXT NOT NULL,body TEXT NOT NULL);CREATE INDEX IF NOT EXISTS sync_entity ON sync_events(entity);CREATE TABLE IF NOT EXISTS sync_sent(destination TEXT NOT NULL,event TEXT NOT NULL,PRIMARY KEY(destination,event));")?;
        if let Some(id) = self.setting("sync-device")? {
            return Ok(id);
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.db.execute(
            "INSERT OR IGNORE INTO settings(key,value) VALUES('sync-device',?)",
            [id],
        )?;
        self.setting("sync-device")?
            .context("device identity missing")
    }
    pub fn sync_revision(&self) -> Result<u64> {
        self.sync_init()?;
        let revision: i64 =
            self.db
                .query_row("SELECT COALESCE(max(rowid),0) FROM sync_events", [], |r| {
                    r.get(0)
                })?;
        Ok(revision as u64)
    }
    pub fn sync_events(&self, entity: Option<&str>) -> Result<Vec<Event>> {
        self.sync_init()?;
        let mut q = self
            .db
            .prepare("SELECT body FROM sync_events WHERE ?1 IS NULL OR entity=?1 ORDER BY rowid")?;
        let bodies = q
            .query_map([entity], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        bodies
            .iter()
            .map(|body| Ok(serde_json::from_str(body)?))
            .collect()
    }
    pub fn sync_heads(&self, entity: &str) -> Result<Vec<String>> {
        let events = self.sync_events(Some(entity))?;
        let parents: HashSet<_> = events.iter().flat_map(|e| e.parents.iter()).collect();
        Ok(events
            .iter()
            .filter(|e| !parents.contains(&e.id))
            .map(|e| e.id.clone())
            .collect())
    }
    pub fn sync_set(&mut self, entity: &str, field: &str, value: Option<Value>) -> Result<Event> {
        let device = self.sync_init()?;
        let event = Event {
            id: uuid::Uuid::new_v4().to_string(),
            device,
            entity: entity.into(),
            field: field.into(),
            parents: self.sync_heads(entity)?,
            value,
        };
        self.sync_ingest(std::slice::from_ref(&event))?;
        Ok(event)
    }
    /// Validate the whole causal batch before writing any event, including duplicate-ID tampering.
    pub fn sync_ingest(&mut self, incoming: &[Event]) -> Result<usize> {
        self.sync_init()?;
        ensure!(incoming.len() <= 100_000, "too many sync events");
        for event in incoming {
            event.validate()?;
        }
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut known = HashMap::<String, Event>::new();
        {
            let mut stmt = tx.prepare("SELECT body FROM sync_events")?;
            for body in stmt.query_map([], |r| r.get::<_, String>(0))? {
                let e: Event = serde_json::from_str(&body?)?;
                known.insert(e.id.clone(), e);
            }
        }
        let mut pending = BTreeMap::new();
        for event in incoming {
            if let Some(old) = known.get(&event.id).or_else(|| pending.get(&event.id)) {
                ensure!(old == event, "sync event identity has conflicting content");
            } else {
                pending.insert(event.id.clone(), event.clone());
            }
        }
        let count = pending.len();
        while !pending.is_empty() {
            let ready = pending
                .values()
                .filter(|e| e.parents.iter().all(|p| known.contains_key(p)))
                .cloned()
                .collect::<Vec<_>>();
            ensure!(
                !ready.is_empty(),
                "sync batch has missing or cyclic parents"
            );
            for event in ready {
                ensure!(
                    event
                        .parents
                        .iter()
                        .all(|p| known[p].entity == event.entity),
                    "causal parents belong to another entity"
                );
                tx.execute(
                    "INSERT INTO sync_events(id,device,entity,body) VALUES(?,?,?,?)",
                    params![
                        event.id,
                        event.device,
                        event.entity,
                        serde_json::to_string(&event)?
                    ],
                )?;
                pending.remove(&event.id);
                known.insert(event.id.clone(), event);
            }
        }
        tx.commit()?;
        Ok(count)
    }
    pub fn sync_entity(&self, entity: &str) -> Result<Entity> {
        let events = self.sync_events(Some(entity))?;
        entity_state(entity, &events)
    }
    pub fn sync_conflicts(&self) -> Result<Vec<Conflict>> {
        let entities: BTreeSet<_> = self
            .sync_events(None)?
            .into_iter()
            .map(|e| e.entity)
            .collect();
        Ok(entities
            .into_iter()
            .map(|e| self.sync_entity(&e))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flat_map(|e| e.conflicts)
            .collect())
    }
    pub fn sync_outbox(&self, destination: &str) -> Result<Vec<Event>> {
        self.sync_init()?;
        let mut stmt=self.db.prepare("SELECT body FROM sync_events WHERE id NOT IN(SELECT event FROM sync_sent WHERE destination=?1) ORDER BY rowid LIMIT 200")?;
        let rows = stmt
            .query_map(params![destination], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }
    pub fn sync_ack(&mut self, destination: &str, events: &[Event]) -> Result<()> {
        let tx = self.db.transaction()?;
        for e in events {
            tx.execute(
                "INSERT OR IGNORE INTO sync_sent(destination,event) VALUES(?,?)",
                params![destination, e.id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn entity_state(entity: &str, events: &[Event]) -> Result<Entity> {
    let by_id: HashMap<_, _> = events.iter().map(|e| (e.id.as_str(), e)).collect();
    let ancestor = |earlier: &str, later: &Event| -> bool {
        let mut queue = later.parents.clone();
        let mut visited = HashSet::new();
        while let Some(id) = queue.pop() {
            if id == earlier {
                return true;
            }
            if visited.insert(id.clone())
                && let Some(e) = by_id.get(id.as_str())
            {
                queue.extend(e.parents.clone());
            }
        }
        false
    };
    let fields: BTreeSet<_> = events.iter().map(|e| e.field.as_str()).collect();
    let mut result = Entity::default();
    let live_deletes = events
        .iter()
        .filter(|e| {
            e.field == "$deleted"
                && !events
                    .iter()
                    .any(|later| later.id != e.id && ancestor(&e.id, later))
        })
        .collect::<Vec<_>>();
    if !live_deletes.is_empty() {
        let edits = events
            .iter()
            .filter(|e| e.field != "$deleted" && live_deletes.iter().any(|d| !ancestor(&e.id, d)))
            .cloned()
            .collect::<Vec<_>>();
        if edits.is_empty() {
            result.deleted = true;
            return Ok(result);
        }
        result.conflicts.push(Conflict {
            entity: entity.into(),
            field: "$deleted".into(),
            candidates: live_deletes.into_iter().cloned().chain(edits).collect(),
        });
    }
    for field in fields.into_iter().filter(|f| *f != "$deleted") {
        let heads = events
            .iter()
            .filter(|e| {
                e.field == field
                    && !events.iter().any(|later| {
                        later.field == field && later.id != e.id && ancestor(&e.id, later)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        if heads.is_empty() {
            continue;
        }
        if heads.iter().all(|h| h.value == heads[0].value) {
            if let Some(value) = &heads[0].value {
                result.fields.insert(field.into(), value.clone());
            }
        } else {
            result.conflicts.push(Conflict {
                entity: entity.into(),
                field: field.into(),
                candidates: heads,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_fields_merge_but_concurrent_writes_and_delete_conflict() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let mut a = Catalog::open(a.path()).unwrap();
        let mut b = Catalog::open(b.path()).unwrap();
        let first = a
            .sync_set("asset:image", "name", Some("old".into()))
            .unwrap();
        b.sync_ingest(&[first]).unwrap();
        a.sync_set("asset:image", "name", Some("A".into())).unwrap();
        b.sync_set("asset:image", "hidden", Some(true.into()))
            .unwrap();
        let ea = a.sync_events(None).unwrap();
        let eb = b.sync_events(None).unwrap();
        a.sync_ingest(&eb).unwrap();
        b.sync_ingest(&ea).unwrap();
        assert!(a.sync_conflicts().unwrap().is_empty());
        assert_eq!(a.sync_entity("asset:image").unwrap().fields["hidden"], true);
        a.sync_set("asset:image", "name", Some("A2".into()))
            .unwrap();
        b.sync_set("asset:image", "name", Some("B2".into()))
            .unwrap();
        a.sync_ingest(&b.sync_events(None).unwrap()).unwrap();
        assert_eq!(a.sync_conflicts().unwrap().len(), 1);
        a.sync_set("asset:image", "name", Some("resolved".into()))
            .unwrap();
        b.sync_ingest(&a.sync_events(None).unwrap()).unwrap();
        assert!(b.sync_conflicts().unwrap().is_empty());
        a.sync_set("asset:image", "$deleted", Some(true.into()))
            .unwrap();
        b.sync_set("asset:image", "name", Some("edit".into()))
            .unwrap();
        a.sync_ingest(&b.sync_events(None).unwrap()).unwrap();
        assert!(
            a.sync_conflicts()
                .unwrap()
                .iter()
                .any(|c| c.field == "$deleted")
        );
    }
    #[test]
    fn damaged_batches_and_device_local_fields_are_rejected_atomically() {
        let t = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(t.path()).unwrap();
        let first = c.sync_set("asset:a", "name", Some("ok".into())).unwrap();
        let mut bad = first.clone();
        bad.value = Some("tampered".into());
        assert!(c.sync_ingest(&[bad]).is_err());
        assert!(
            c.sync_set("asset:a", "cache_path", Some("/private/a.png".into()))
                .is_err()
        );
        assert_eq!(c.sync_events(None).unwrap().len(), 1);
        let mut missing = first;
        missing.id = uuid::Uuid::new_v4().to_string();
        missing.parents = vec![uuid::Uuid::new_v4().to_string()];
        assert!(c.sync_ingest(&[missing]).is_err());
        assert_eq!(c.sync_events(None).unwrap().len(), 1);
    }
}
