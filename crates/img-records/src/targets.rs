//! Serializable operation snapshots. Local file paths and execution state never enter sync.
use crate::catalog::{Catalog, Location};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub input_id: String,
    pub name: String,
    pub content_hash: Option<String>,
    pub location: Location,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlan {
    pub version: u32,
    pub task_id: String,
    pub targets: Vec<Target>,
}
impl TargetPlan {
    pub fn capture(catalog: &Catalog, ids: &[String], provider: &str) -> Result<Self> {
        // One SQLite read transaction gives every selected item the same catalog snapshot.
        let tx = catalog.db.unchecked_transaction()?;
        let mut assets = ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|id| catalog.get(id))
            .collect::<Result<Vec<_>>>()?;
        assets.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(a.id.cmp(&b.id)));
        let targets = assets
            .into_iter()
            .map(|asset| {
                let location = asset
                    .selected_location(provider)
                    .context("image has no address in selected storage")?
                    .clone();
                Ok(Target {
                    input_id: asset.id,
                    name: asset.name,
                    content_hash: asset.content_hash,
                    location,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        let plan = Self {
            version: 1,
            task_id: uuid::Uuid::new_v4().to_string(),
            targets,
        };
        plan.validate()?;
        Ok(plan)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && uuid::Uuid::parse_str(&self.task_id).is_ok(),
            "unsupported target plan"
        );
        ensure!(!self.targets.is_empty(), "select at least one image");
        let mut ids = std::collections::BTreeSet::new();
        for target in &self.targets {
            ensure!(
                ids.insert(&target.input_id),
                "duplicate image in target plan"
            );
            ensure!(!target.location.url.is_empty(), "target address is missing");
            if let Some(hash) = &target.content_hash {
                ensure!(
                    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                    "invalid target hash"
                );
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::RemoteRecord;
    #[test]
    fn selection_snapshot_preserves_hidden_targets_order_and_remote_version() {
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(root.path()).unwrap();
        let mut record = RemoteRecord {
            namespace: "one".into(),
            provider: "one".into(),
            path: Some("a.png".into()),
            url: "https://one.test/a.png".into(),
            version: "v1".into(),
            name: "a.png".into(),
            content_type: "image/png".into(),
            size: 10,
            added_at: 1,
            origin: String::new(),
            content_hash: None,
        };
        let a = c.upsert(&record).unwrap();
        c.set_hidden(&a, true).unwrap();
        record.path = Some("b.png".into());
        record.added_at = 2;
        let b = c.upsert(&record).unwrap();
        let plan = TargetPlan::capture(&c, &[a.clone(), b.clone(), a.clone()], "one").unwrap();
        assert_eq!(plan.targets.len(), 2);
        assert_eq!(plan.targets[0].input_id, b);
        assert_eq!(plan.targets[1].input_id, a);
        record.version = "v2".into();
        c.upsert(&record).unwrap();
        assert_eq!(plan.targets[0].location.version, "v1");
        let decoded: TargetPlan =
            serde_json::from_slice(&serde_json::to_vec(&plan).unwrap()).unwrap();
        decoded.validate().unwrap();
        assert!(TargetPlan::capture(&c, &["missing".into()], "").is_err());
    }
}
