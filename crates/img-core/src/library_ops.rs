use crate::{config::Config, control::Control, provider::Provider};
use anyhow::{Context, Result, ensure};
use img_records::catalog::{Catalog, Location};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteTarget {
    pub input_id: String,
    pub name: String,
    pub location: Location,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletePlan {
    pub version: u32,
    pub task_id: String,
    pub targets: Vec<DeleteTarget>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ItemResult {
    pub task_id: String,
    pub input_id: String,
    pub output_order: usize,
    pub success: bool,
    pub remote_deleted: bool,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub location: Location,
}
pub fn plan_delete(catalog: &Catalog, ids: &[String], provider: &str) -> Result<DeletePlan> {
    let mut targets = vec![];
    let ids = ids.iter().collect::<std::collections::BTreeSet<_>>();
    let mut assets = ids
        .into_iter()
        .map(|id| catalog.get(id))
        .collect::<Result<Vec<_>>>()?;
    assets.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(a.id.cmp(&b.id)));
    for asset in assets {
        let location = asset
            .selected_location(provider)
            .context("image has no address in selected storage")?
            .clone();
        ensure!(
            location.path.is_some() && !location.version.is_empty(),
            "refresh the remote index before deleting an unassociated or unversioned image"
        );
        targets.push(DeleteTarget {
            input_id: asset.id,
            name: asset.name,
            location,
        });
    }
    ensure!(!targets.is_empty(), "select at least one remote file");
    Ok(DeletePlan {
        version: 1,
        task_id: uuid::Uuid::new_v4().to_string(),
        targets,
    })
}
pub fn delete(
    catalog: &mut Catalog,
    config: &Config,
    plan: &DeletePlan,
    control: &Control,
) -> Result<Vec<ItemResult>> {
    ensure!(
        plan.version == 1 && uuid::Uuid::parse_str(&plan.task_id).is_ok(),
        "unsupported delete plan"
    );
    let task = format!("delete:{}", plan.task_id);
    let mut results: Vec<ItemResult> = catalog
        .task(&task)
        .ok()
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    for (index, target) in plan.targets.iter().enumerate() {
        control.check()?;
        if results.iter().any(|r| {
            r.input_id == target.input_id
                && r.location == target.location
                && (r.success || r.remote_deleted)
        }) {
            continue;
        }
        let mut remote_deleted = false;
        let operation = (|| -> Result<()> {
            let asset = catalog.get(&target.input_id)?;
            let current = asset
                .locations
                .iter()
                .find(|l| l.id == target.location.id)
                .context("selected location no longer exists")?;
            ensure!(
                current.namespace == target.location.namespace
                    && current.path == target.location.path
                    && current.version == target.location.version,
                "selected remote version changed; create a new plan"
            );
            let path = current
                .path
                .as_deref()
                .context("unknown remote object path")?;
            ensure!(!current.version.is_empty(), "remote version is unknown");
            let _lease =
                img_records::remote_lock::acquire(&catalog.root, &current.namespace, path, true)?;
            let provider = Provider::new(
                &current.provider,
                config
                    .providers
                    .get(&current.provider)
                    .context("storage source is missing")?,
            )?;
            ensure!(
                provider.namespace() == current.namespace,
                "storage identity changed; create a new plan"
            );
            provider.delete_remote(path, &current.version)?;
            remote_deleted = true;
            let checked = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            ensure!(
                catalog.mark_location(&current.id, &current.version, "deleted", checked)?,
                "remote deletion succeeded but the catalog version changed; refresh its status"
            );
            Ok(())
        })();
        let (error, error_code) = match &operation {
            Ok(()) => (None, None),
            Err(e) => {
                let failure =
                    crate::failure::Failure::from_error(e, crate::failure::ErrorCode::Unknown);
                (
                    Some(if remote_deleted {
                        "remote deleted; catalog update failed".into()
                    } else {
                        failure.message().into()
                    }),
                    Some(
                        serde_json::to_value(failure.code)?
                            .as_str()
                            .unwrap_or("unknown")
                            .into(),
                    ),
                )
            }
        };
        results.retain(|r| !(r.input_id == target.input_id && r.location.id == target.location.id));
        results.push(ItemResult {
            task_id: plan.task_id.clone(),
            input_id: target.input_id.clone(),
            output_order: index,
            success: operation.is_ok(),
            remote_deleted,
            error,
            error_code,
            location: target.location.clone(),
        });
        catalog
            .save_task(&task, "delete", &serde_json::to_string(&results)?)
            .context(if remote_deleted {
                "remote deletion succeeded but task progress could not be saved"
            } else {
                "could not save delete task progress"
            })?;
    }
    results.sort_by_key(|r| r.output_order);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_delete_checks_versions_protection_and_keeps_other_copies() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let cfg = crate::config::ProviderConfig {
            kind: "s3".into(),
            endpoint: format!("http://{}", server.server_addr()),
            bucket: "images".into(),
            public_url: "https://cdn.test".into(),
            access_key: "test".into(),
            secret_key: "test".into(),
            path_style: true,
            allow_insecure: true,
            ..Default::default()
        };
        let p = Provider::new("one", &cfg).unwrap();
        let mut config = Config::default();
        config.providers.insert("one".into(), cfg);
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(root.path()).unwrap();
        let original = root.path().join("original.png");
        std::fs::write(&original, b"user original").unwrap();
        let record = img_records::catalog::RemoteRecord {
            namespace: p.namespace(),
            provider: "one".into(),
            path: Some("a.png".into()),
            url: "https://cdn.test/a.png".into(),
            version: "v1".into(),
            name: "a.png".into(),
            content_type: "image/png".into(),
            size: 1,
            added_at: 1,
            origin: "test".into(),
            content_hash: Some(img_records::catalog::digest(b"image")),
        };
        let id = c.upsert(&record).unwrap();
        let mut replica = record.clone();
        replica.namespace = "other".into();
        replica.provider = "two".into();
        c.upsert(&replica).unwrap();
        let plan = plan_delete(&c, std::slice::from_ref(&id), "one").unwrap();
        assert_eq!(plan.targets.len(), 1);
        assert!(
            server
                .recv_timeout(std::time::Duration::from_millis(5))
                .unwrap()
                .is_none()
        );
        let cancelled = Control::default();
        cancelled.cancel();
        assert!(delete(&mut c, &config, &plan, &cancelled).is_err());
        let lease =
            img_records::remote_lock::acquire(root.path(), &p.namespace(), "a.png", false).unwrap();
        assert!(!delete(&mut c, &config, &plan, &Control::default()).unwrap()[0].success);
        drop(lease);
        let mut changed = plan.clone();
        changed.targets[0].location.version = "old".into();
        assert!(!delete(&mut c, &config, &changed, &Control::default()).unwrap()[0].success);
        let thread = std::thread::spawn(move || {
            let request = server
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(request.method().as_str(), "DELETE");
            assert!(
                request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("If-Match") && h.value.as_str() == "v1")
            );
            request.respond(tiny_http::Response::empty(204)).unwrap();
        });
        assert!(delete(&mut c, &config, &plan, &Control::default()).unwrap()[0].success);
        thread.join().unwrap();
        // A replay uses persisted success and never issues another delete request.
        assert!(delete(&mut c, &config, &plan, &Control::default()).unwrap()[0].success);
        let locations = c.get(&id).unwrap().locations;
        assert_eq!(
            locations
                .iter()
                .find(|l| l.provider == "one")
                .unwrap()
                .availability,
            "deleted"
        );
        assert_eq!(
            locations
                .iter()
                .find(|l| l.provider == "two")
                .unwrap()
                .availability,
            "unknown"
        );
        assert_eq!(std::fs::read(original).unwrap(), b"user original");
    }
}
