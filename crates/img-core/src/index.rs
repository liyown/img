//! Resumable remote enumeration. Incomplete scans never infer missing objects.
use crate::{control::Control, provider::Provider};
use anyhow::{Result, ensure};
use img_records::catalog::{Catalog, RemoteRecord};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scope {
    pub id: String,
    pub provider: String,
    pub namespace: String,
    pub prefix: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scan {
    pub scope: Scope,
    pub pending: VecDeque<(String, Option<String>)>,
    pub visited: BTreeSet<(String, Option<String>)>,
    pub seen: BTreeSet<String>,
    pub complete: bool,
    pub checked_at: u64,
}
impl Scope {
    pub fn new(provider: &Provider, prefix: &str) -> Result<Self> {
        if !prefix.is_empty() {
            crate::pathgen::validate(prefix.trim_end_matches('/'))?;
            ensure!(
                prefix.ends_with('/'),
                "index scope must be a directory ending in /"
            );
        }
        let namespace = provider.namespace();
        Ok(Self {
            id: img_records::catalog::identity(&[&namespace, prefix]),
            provider: provider.name.clone(),
            namespace,
            prefix: prefix.into(),
            enabled: true,
        })
    }
}
fn image_type(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}
pub fn run(
    catalog: &mut Catalog,
    provider: &Provider,
    scope: Scope,
    resume: bool,
    excluded: &[String],
    control: &Control,
) -> Result<Scan> {
    ensure!(
        provider.namespace() == scope.namespace,
        "storage identity changed; select the scope again"
    );
    let task = format!("index:{}", scope.id);
    let mut scan = if resume {
        catalog
            .task(&task)
            .ok()
            .and_then(|s| serde_json::from_str::<Scan>(&s).ok())
            .filter(|s| {
                !s.complete
                    && s.scope.namespace == scope.namespace
                    && s.scope.prefix == scope.prefix
            })
    } else {
        None
    }
    .unwrap_or_else(|| Scan {
        scope: scope.clone(),
        pending: VecDeque::from([(scope.prefix.clone(), None)]),
        visited: BTreeSet::new(),
        seen: BTreeSet::new(),
        complete: false,
        checked_at: 0,
    });
    catalog.set_setting(
        &format!("scope:{}", scope.id),
        &serde_json::to_string(&scope)?,
    )?;
    let scope_entity = format!("scope:{}", scope.id);
    let previous = catalog.sync_entity(&scope_entity)?;
    let value = serde_json::to_value(&scope)?;
    for field in ["provider", "namespace", "prefix", "enabled"] {
        if previous.fields.get(field) != Some(&value[field]) {
            catalog.sync_set(&scope_entity, field, Some(value[field].clone()))?;
        }
    }
    while let Some((prefix, cursor)) = scan.pending.front().cloned() {
        let enabled = catalog
            .setting(&format!("scope:{}", scope.id))?
            .and_then(|s| serde_json::from_str::<Scope>(&s).ok())
            .is_none_or(|s| s.enabled);
        if control.is_cancelled() || !enabled {
            catalog.save_task(&task, "index", &serde_json::to_string(&scan)?)?;
            return Ok(scan);
        }
        ensure!(
            !scan.visited.contains(&(prefix.clone(), cursor.clone())),
            "remote pagination repeated a cursor; scan remains incomplete"
        );
        // Persist the current position before I/O so a failed request remains resumable.
        catalog.save_task(&task, "index", &serde_json::to_string(&scan)?)?;
        let page = provider.list_remote(&prefix, cursor.as_deref())?;
        for item in page.items {
            ensure!(
                item.path.starts_with(&scope.prefix),
                "listing returned an object outside the selected scope"
            );
            if item.path.split('/').any(|s| s == ".img-sync")
                || excluded
                    .iter()
                    .any(|p| !p.is_empty() && item.path.starts_with(p))
            {
                continue;
            }
            if item.directory {
                ensure!(
                    item.path != prefix && item.path.starts_with(&prefix),
                    "listing returned a parent directory"
                );
                if !scan.visited.contains(&(item.path.clone(), None))
                    && !scan.pending.contains(&(item.path.clone(), None))
                {
                    scan.pending.push_back((item.path, None));
                }
                continue;
            }
            let Some(ct) = image_type(&item.path) else {
                continue;
            };
            catalog.upsert(&RemoteRecord {
                namespace: scope.namespace.clone(),
                provider: scope.provider.clone(),
                path: Some(item.path.clone()),
                url: item.url,
                version: item.version,
                name: item.path.rsplit('/').next().unwrap_or(&item.path).into(),
                content_type: ct.into(),
                size: item.size,
                added_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                origin: "remote".into(),
                content_hash: None,
            })?;
            scan.seen.insert(item.path);
        }
        scan.pending.pop_front();
        scan.visited.insert((prefix.clone(), cursor));
        if let Some(next) = page.next {
            ensure!(
                !scan.visited.contains(&(prefix.clone(), Some(next.clone()))),
                "remote pagination repeated a cursor"
            );
            scan.pending.push_front((prefix, Some(next)));
        }
        catalog.save_task(&task, "index", &serde_json::to_string(&scan)?)?;
    }
    scan.checked_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    catalog.finish_scan(
        &scope.namespace,
        &scope.prefix,
        &scan.seen,
        scan.checked_at,
        excluded,
    )?;
    scan.complete = true;
    catalog.save_task(&task, "index", &serde_json::to_string(&scan)?)?;
    catalog.set_setting(
        &format!("scan-time:{}", scope.id),
        &scan.checked_at.to_string(),
    )?;
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_remote_cursor_does_not_mark_unseen_images_missing() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr());
        let provider = Provider::new(
            "test",
            &crate::config::ProviderConfig {
                kind: "s3".into(),
                endpoint,
                bucket: "bucket".into(),
                public_url: "https://cdn.test".into(),
                access_key: "test".into(),
                secret_key: "test".into(),
                allow_insecure: true,
                path_style: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(Scope::new(&provider, "photos/2026/").is_ok());
        assert!(Scope::new(&provider, "photos/../").is_err());
        assert!(Scope::new(&provider, "photos").is_err());
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(root.path()).unwrap();
        let id = c
            .upsert(&RemoteRecord {
                namespace: provider.namespace(),
                provider: "test".into(),
                path: Some("missing.png".into()),
                url: "https://cdn.test/missing.png".into(),
                version: "v1".into(),
                name: "missing.png".into(),
                content_type: "image/png".into(),
                size: 1,
                added_at: 1,
                origin: "remote".into(),
                content_hash: None,
            })
            .unwrap();
        let loc = c.get(&id).unwrap().locations[0].clone();
        c.mark_location(&loc.id, "v1", "available", 1).unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..2 {
                let request = server
                    .recv_timeout(std::time::Duration::from_secs(8))
                    .unwrap()
                    .unwrap();
                request.respond(tiny_http::Response::from_string("<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>same</NextContinuationToken></ListBucketResult>")).unwrap();
            }
        });
        let scope = Scope::new(&provider, "").unwrap();
        assert!(run(&mut c, &provider, scope, false, &[], &Control::default()).is_err());
        assert_eq!(c.get(&id).unwrap().locations[0].availability, "available");
        handle.join().unwrap();
    }
}
