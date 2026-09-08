//! SQLite triggers capture local metadata changes in the same transaction as the change.
use crate::{
    catalog::{Asset, Catalog, Location},
    sync::Entity,
};
use anyhow::{Result, ensure};
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const ASSET: &[&str] = &[
    "name",
    "content_type",
    "size",
    "added_at",
    "origin",
    "content_hash",
    "hidden",
    "preferred_location",
];
const LOCATION: &[&str] = &[
    "asset_id",
    "namespace",
    "provider",
    "path",
    "url",
    "version",
    "availability",
    "last_checked",
];
impl Catalog {
    pub(crate) fn sync_install_capture(&self) -> Result<()> {
        self.sync_init()?;
        let uuid = "lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-4'||substr(lower(hex(randomblob(2))),2)||'-8'||substr(lower(hex(randomblob(2))),2)||'-'||lower(hex(randomblob(6)))";
        for (table, kind, fields) in [
            ("assets", "asset", ASSET),
            ("locations", "location", LOCATION),
            ("versions", "version", &["parent", "child", "recipe"][..]),
        ] {
            for field in fields {
                let entity = if kind == "version" {
                    "'version:'||json_array(NEW.parent,NEW.child)".to_string()
                } else {
                    format!("'{kind}:'||NEW.id")
                };
                let value = if *field == "hidden" {
                    "json(CASE NEW.hidden WHEN 0 THEN 'false' ELSE 'true' END)".to_string()
                } else {
                    format!("NEW.\"{field}\"")
                };
                let insert = format!(
                    r#"INSERT INTO sync_events(id,device,entity,body)
                    SELECT eid, device, {entity}, json_object('id',eid,'device',device,'entity',{entity},'field','{field}',
                        'parents',json((SELECT json_group_array(e.id) FROM sync_events e WHERE e.entity={entity}
                            AND NOT EXISTS(SELECT 1 FROM sync_events child,json_each(child.body,'$.parents') p WHERE child.entity=e.entity AND p.value=e.id))),
                        'value',{value})
                    FROM (SELECT {uuid} AS eid,(SELECT value FROM settings WHERE key='sync-device') AS device);"#
                );
                for (operation, condition) in [
                    ("INSERT", String::new()),
                    (
                        "UPDATE",
                        format!(" AND OLD.\"{field}\" IS NOT NEW.\"{field}\""),
                    ),
                ] {
                    self.db.execute_batch(&format!(r#"CREATE TRIGGER IF NOT EXISTS sync_{table}_{field}_{operation} AFTER {operation} ON {table}
                        WHEN NOT EXISTS(SELECT 1 FROM settings WHERE key='sync-applying' AND value='1'){condition}
                        BEGIN {insert} END;"#))?;
                }
            }
        }
        Ok(())
    }
    /// Idempotent initial seeding for databases written before capture triggers existed.
    pub fn sync_seed_catalog(&mut self) -> Result<()> {
        let mut rows = vec![];
        for (table, kind, fields) in [
            ("assets", "asset", ASSET),
            ("locations", "location", LOCATION),
        ] {
            let mut stmt=self.db.prepare(&format!("SELECT id FROM {table} WHERE NOT EXISTS(SELECT 1 FROM sync_events WHERE entity='{kind}:'||{table}.id)"))?;
            let ids = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in ids {
                let value = if kind == "asset" {
                    serde_json::to_value(self.get(&id)?)?
                } else {
                    let aid: String = self.db.query_row(
                        "SELECT asset_id FROM locations WHERE id=?",
                        [&id],
                        |r| r.get(0),
                    )?;
                    serde_json::to_value(
                        self.locations(&aid)?
                            .into_iter()
                            .find(|l| l.id == id)
                            .unwrap(),
                    )?
                };
                for field in fields {
                    rows.push((format!("{kind}:{id}"), *field, value[*field].clone()));
                }
            }
        }
        for (entity, field, value) in rows {
            self.sync_set(&entity, field, Some(value))?;
        }
        Ok(())
    }
    /// Apply portable metadata only. Deletions change local visibility/state, never remote storage.
    pub fn sync_materialize_catalog(&mut self) -> Result<usize> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut grouped = BTreeMap::<String, Vec<crate::sync::Event>>::new();
        {
            let mut stmt = tx.prepare("SELECT body FROM sync_events WHERE entity LIKE 'asset:%' OR entity LIKE 'location:%' OR entity LIKE 'scope:%' OR entity LIKE 'version:%' ORDER BY rowid")?;
            for body in stmt.query_map([], |r| r.get::<_, String>(0))? {
                let event: crate::sync::Event = serde_json::from_str(&body?)?;
                grouped.entry(event.entity.clone()).or_default().push(event);
            }
        }
        let states = grouped
            .into_iter()
            .map(|(name, events)| Ok((name.clone(), crate::sync::entity_state(&name, &events)?)))
            .collect::<Result<Vec<_>>>()?;
        tx.execute("INSERT INTO settings(key,value) VALUES('sync-applying','1') ON CONFLICT(key) DO UPDATE SET value='1'",[])?;
        let mut changed = 0;
        for (entity, state) in &states {
            let Some(id) = entity.strip_prefix("asset:") else {
                continue;
            };
            if state.deleted {
                changed += tx.execute("UPDATE assets SET hidden=1 WHERE id=?", [id])?;
                continue;
            }
            // Missing/conflicting fields cannot create a half-defined asset. Existing fields remain intact.
            let old:Option<String>=tx.query_row("SELECT json_object('id',id,'name',name,'content_type',content_type,'size',size,'added_at',added_at,'origin',origin,'content_hash',content_hash,'hidden',json(CASE hidden WHEN 0 THEN 'false' ELSE 'true' END),'preferred_location',preferred_location,'locations',json('[]')) FROM assets WHERE id=?",[id],|r|r.get(0)).optional()?;
            let mut value:Value=old.map(|s|serde_json::from_str(&s)).transpose()?.unwrap_or(json!({"id":id,"locations":[],"hidden":false,"preferred_location":null,"content_hash":null}));
            merge(&mut value, state);
            let Ok(a) = serde_json::from_value::<Asset>(value) else {
                continue;
            };
            ensure!(
                a.size <= i64::MAX as u64 && a.added_at <= i64::MAX as u64,
                "synced asset exceeds metadata range"
            );
            changed+=tx.execute("INSERT INTO assets(id,name,content_type,size,added_at,origin,content_hash,hidden,preferred_location) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,content_type=excluded.content_type,size=excluded.size,added_at=excluded.added_at,origin=excluded.origin,content_hash=excluded.content_hash,hidden=excluded.hidden,preferred_location=excluded.preferred_location",params![a.id,a.name,a.content_type,a.size as i64,a.added_at as i64,a.origin,a.content_hash,a.hidden,a.preferred_location])?;
        }
        for (entity, state) in &states {
            let Some(id) = entity.strip_prefix("location:") else {
                continue;
            };
            if state.deleted {
                changed += tx.execute(
                    "UPDATE locations SET availability='deleted' WHERE id=?",
                    [id],
                )?;
                continue;
            }
            if !state.conflicts.is_empty() {
                // Conflicting observations require a fresh check; device clocks never decide existence.
                tx.execute(
                    "UPDATE locations SET availability='unknown',last_checked=NULL WHERE id=?",
                    [id],
                )?;
                continue;
            }
            let mut value = json!({"id":id,"path":null,"last_checked":null});
            merge(&mut value, state);
            let Ok(l) = serde_json::from_value::<Location>(value) else {
                continue;
            };
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM assets WHERE id=?)",
                [&l.asset_id],
                |r| r.get(0),
            )?;
            if !exists {
                continue;
            }
            ensure!(
                l.last_checked.is_none_or(|v| v <= i64::MAX as u64),
                "synced observation exceeds metadata range"
            );
            changed+=tx.execute("INSERT INTO locations(id,asset_id,namespace,provider,path,url,version,availability,last_checked) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET asset_id=excluded.asset_id,namespace=excluded.namespace,provider=excluded.provider,path=excluded.path,url=excluded.url,version=excluded.version,availability=excluded.availability,last_checked=excluded.last_checked",params![l.id,l.asset_id,l.namespace,l.provider,l.path,l.url,l.version,l.availability,l.last_checked.map(|v|v as i64)])?;
        }
        for (entity, state) in &states {
            let Some(id) = entity.strip_prefix("scope:") else {
                continue;
            };
            let mut value = json!({"id":id});
            merge(&mut value, state);
            if state.deleted {
                value["enabled"] = false.into();
            }
            if !state.conflicts.is_empty() {
                continue;
            }
            if ["namespace", "provider", "prefix"]
                .iter()
                .all(|key| value[key].is_string())
                && value["enabled"].is_boolean()
            {
                tx.execute("INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("scope:{id}"),serde_json::to_string(&value)?])?;
            }
        }
        for (entity, state) in &states {
            if !entity.starts_with("version:") || state.deleted || !state.conflicts.is_empty() {
                continue;
            }
            if let (
                Some(Value::String(parent)),
                Some(Value::String(child)),
                Some(Value::String(recipe)),
            ) = (
                state.fields.get("parent"),
                state.fields.get("child"),
                state.fields.get("recipe"),
            ) {
                ensure!(parent != child, "version cannot be its own parent");
                tx.execute("INSERT INTO versions(parent,child,recipe) VALUES(?,?,?) ON CONFLICT(parent,child) DO UPDATE SET recipe=excluded.recipe",params![parent,child,recipe])?;
            }
        }
        tx.execute("DELETE FROM settings WHERE key='sync-applying'", [])?;
        tx.commit()?;
        Ok(changed)
    }
}
use rusqlite::OptionalExtension;
fn merge(value: &mut Value, state: &Entity) {
    for (key, v) in &state.fields {
        value[key] = v.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CatalogQuery, RemoteRecord};
    #[test]
    fn local_changes_are_transactional_and_received_metadata_does_not_echo() {
        let ta = tempfile::tempdir().unwrap();
        let tb = tempfile::tempdir().unwrap();
        let mut a = Catalog::open(ta.path()).unwrap();
        let mut b = Catalog::open(tb.path()).unwrap();
        let id = a
            .upsert(&RemoteRecord {
                namespace: "bucket".into(),
                provider: "source".into(),
                path: Some("image.png".into()),
                url: "https://cdn.test/image.png".into(),
                version: "v1".into(),
                name: "image.png".into(),
                content_type: "image/png".into(),
                size: 12,
                added_at: 1,
                origin: "test".into(),
                content_hash: None,
            })
            .unwrap();
        let events = a.sync_events(None).unwrap();
        assert!(!events.is_empty());
        b.sync_ingest(&events).unwrap();
        b.sync_materialize_catalog().unwrap();
        assert_eq!(b.query(&CatalogQuery::default()).unwrap().total, 1);
        assert_eq!(b.sync_events(None).unwrap().len(), events.len());
        a.set_hidden(&id, true).unwrap();
        b.sync_ingest(&a.sync_events(None).unwrap()).unwrap();
        b.sync_materialize_catalog().unwrap();
        assert!(b.get(&id).unwrap().hidden);
        assert!(b.get(&id).unwrap().locations[0].availability == "unknown");
        let previous = a.sync_events(None).unwrap().len();
        a.db.execute_batch("BEGIN; UPDATE assets SET name='temporary'; ROLLBACK;")
            .unwrap();
        assert_eq!(a.sync_events(None).unwrap().len(), previous);
    }
}
