//! Recoverable copies: immutable destinations, byte verification and no source deletion.
use crate::{
    config::Config,
    control::Control,
    media, network, pathgen,
    provider::{Provider, Request},
};
use anyhow::{Context, Result, ensure};
use img_records::{
    catalog::{Catalog, RemoteRecord, digest},
    targets::{Target, TargetPlan},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub input_id: String,
    pub path: String,
    pub existing_version: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    pub version: u32,
    pub task_id: String,
    pub source: TargetPlan,
    pub provider: String,
    pub namespace: String,
    pub destinations: Vec<Destination>,
    pub preserve_source: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ItemResult {
    pub task_id: String,
    pub input_id: String,
    pub output_order: usize,
    pub status: String,
    pub success: bool,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub content_hash: Option<String>,
    pub source_url: String,
    pub url: Option<String>,
    pub location: Option<img_records::catalog::Location>,
    pub destination_path: String,
}
#[derive(Serialize, Deserialize)]
pub struct Report {
    pub plan: MigrationPlan,
    pub files: Vec<ItemResult>,
    #[serde(default)]
    pub local_sources: std::collections::BTreeMap<String, crate::processing::batch::Input>,
}
/// Attach a user-selected recovery source without starting any remote operation.
pub fn set_local_source(
    catalog: &Catalog,
    task_id: &str,
    input_id: &str,
    path: &std::path::Path,
    limit: u64,
) -> Result<Report> {
    ensure!(
        uuid::Uuid::parse_str(task_id).is_ok(),
        "invalid migration task ID"
    );
    let _lock = img_records::remote_lock::acquire(&catalog.root, "migration-task", task_id, true)?;
    let mut report: Report = serde_json::from_str(&catalog.task(&format!("migrate:{task_id}"))?)?;
    report.plan.validate()?;
    let source = report
        .plan
        .source
        .targets
        .iter()
        .find(|target| target.input_id == input_id)
        .context("image is not in this migration")?;
    let result = report
        .files
        .iter()
        .find(|file| file.input_id == input_id)
        .context("migration result missing")?;
    ensure!(!result.success, "this migration item is already complete");
    let snapshot = crate::processing::batch::Input::snapshot(path, limit)?;
    if let Some(hash) = result
        .content_hash
        .as_ref()
        .or(source.content_hash.as_ref())
    {
        ensure!(
            hash == &snapshot.content_hash,
            "selected image does not match the known original hash"
        );
    }
    report.local_sources.insert(input_id.into(), snapshot);
    store(catalog, &report)?;
    Ok(report)
}
impl Report {
    pub fn mapping(&self) -> Vec<serde_json::Value> {
        self.files.iter().filter(|item|item.success).map(|item|serde_json::json!({"old":item.source_url,"new":item.url,"input_id":item.input_id,"content_hash":item.content_hash})).collect()
    }
}
impl MigrationPlan {
    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        ensure!(
            self.version == 1 && uuid::Uuid::parse_str(&self.task_id).is_ok(),
            "unsupported migration plan"
        );
        ensure!(self.preserve_source, "migration must preserve source files");
        ensure!(
            !self.provider.is_empty() && !self.namespace.is_empty(),
            "missing destination storage"
        );
        ensure!(
            self.destinations.len() == self.source.targets.len(),
            "migration targets do not match"
        );
        let mut paths = HashSet::new();
        for (source, target) in self.source.targets.iter().zip(&self.destinations) {
            ensure!(
                source.input_id == target.input_id,
                "migration targets changed order"
            );
            pathgen::validate(&target.path)?;
            ensure!(
                !target.path.ends_with('/') && paths.insert(&target.path),
                "duplicate destination path"
            );
            ensure!(
                source.location.namespace != self.namespace
                    || source.location.path.as_ref() != Some(&target.path),
                "destination equals source"
            );
        }
        Ok(())
    }
}
/// Planning performs reads only. Destinations use a task directory to avoid unrelated objects.
pub fn plan(
    source: TargetPlan,
    target: &Provider,
    prefix: &str,
    control: &Control,
) -> Result<MigrationPlan> {
    source.validate()?;
    ensure!(
        target.supports_remote_management(),
        "migration requires storage with authenticated reads and conditional writes"
    );
    let prefix = prefix.trim_end_matches('/');
    if !prefix.is_empty() {
        pathgen::validate(prefix)?;
    }
    let task_id = uuid::Uuid::new_v4().to_string();
    let directory = if prefix.is_empty() {
        format!("copies/{task_id}")
    } else {
        format!("{prefix}/{task_id}")
    };
    let mut destinations = vec![];
    for (order, source) in source.targets.iter().enumerate() {
        control.check()?;
        let name: String = source
            .name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .take(120)
            .collect();
        let name = if name.is_empty() { "image" } else { &name };
        let path = format!("{directory}/{:04}-{name}", order + 1);
        let existing_version = target.stat_remote(&path, control)?.map(|item| item.version);
        destinations.push(Destination {
            input_id: source.input_id.clone(),
            path,
            existing_version,
        });
    }
    let plan = MigrationPlan {
        version: 1,
        task_id,
        source,
        provider: target.name.clone(),
        namespace: target.namespace(),
        destinations,
        preserve_source: true,
    };
    plan.validate()?;
    Ok(plan)
}
fn source_bytes(
    catalog: &Catalog,
    cfg: &Config,
    source: &Target,
    limit: u64,
    control: &Control,
) -> Result<Vec<u8>> {
    let read = (|| -> Result<Vec<u8>> {
        if let Some(path) = &source.location.path {
            let provider = Provider::new(
                &source.location.provider,
                cfg.providers
                    .get(&source.location.provider)
                    .context("source storage is missing")?,
            )?;
            ensure!(
                provider.namespace() == source.location.namespace,
                "source storage identity changed"
            );
            ensure!(
                !source.location.version.is_empty() || source.content_hash.is_some(),
                "source version is unknown; refresh the index first"
            );
            provider.read_remote(path, &source.location.version, limit, control)
        } else {
            Ok(network::fetch(&source.location.url, limit, false)?.data)
        }
    })();
    let bytes = match read {
        Ok(bytes) => bytes,
        Err(error) => {
            control.check()?;
            let Some(hash) = &source.content_hash else {
                return Err(error.context("source unavailable; no verified original is cached"));
            };
            let cache = img_records::cache::Cache::open(&catalog.root)?;
            if let Ok(lease) = cache.lease(hash) {
                let bytes = media::read_image(&lease.path, limit)?;
                ensure!(
                    digest(&bytes).eq_ignore_ascii_case(hash),
                    "cached source hash does not match"
                );
                bytes
            } else {
                // Copies are alternatives only with an already verified matching content hash.
                let asset = catalog.get(&source.input_id)?;
                let mut found = None;
                for location in asset
                    .locations
                    .iter()
                    .filter(|location| location.id != source.location.id)
                {
                    let Some(path) = &location.path else {
                        continue;
                    };
                    let Some(config) = cfg.providers.get(&location.provider) else {
                        continue;
                    };
                    if let Ok(p) = Provider::new(&location.provider, config)
                        && p.namespace() == location.namespace
                        && let Ok(bytes) = p.read_remote(path, &location.version, limit, control)
                        && digest(&bytes).eq_ignore_ascii_case(hash)
                    {
                        found = Some(bytes);
                        break;
                    }
                    control.check()?;
                }
                found.with_context(|| {
                    format!("source unavailable and no verified copy can be read: {error}")
                })?
            }
        }
    };
    media::inspect(&bytes, limit)?;
    if let Some(hash) = &source.content_hash {
        ensure!(
            digest(&bytes).eq_ignore_ascii_case(hash),
            "source content changed; create a new plan"
        );
    }
    Ok(bytes)
}
fn store(catalog: &Catalog, report: &Report) -> Result<()> {
    catalog.save_task(
        &format!("migrate:{}", report.plan.task_id),
        "migrate",
        &serde_json::to_string(report)?,
    )
}
/// A retry uses the exact persisted plan and hash. Uncertain uploads are read back before any PUT.
pub fn apply(
    catalog: &mut Catalog,
    cfg: &Config,
    plan: &MigrationPlan,
    control: &Control,
) -> Result<Report> {
    plan.validate()?;
    control.check()?;
    let _task_lock =
        img_records::remote_lock::acquire(&catalog.root, "migration-task", &plan.task_id, true)?;
    let provider = Provider::new(
        &plan.provider,
        cfg.providers
            .get(&plan.provider)
            .context("destination storage is missing")?,
    )?;
    ensure!(
        provider.supports_remote_management() && provider.namespace() == plan.namespace,
        "destination storage identity changed; create a new plan"
    );
    let key = format!("migrate:{}", plan.task_id);
    let previous = catalog.task_optional(&key)?;
    let mut report = if let Some(body) = previous {
        serde_json::from_str::<Report>(&body)?
    } else {
        Report {
            plan: plan.clone(),
            local_sources: Default::default(),
            files: plan
                .source
                .targets
                .iter()
                .zip(&plan.destinations)
                .enumerate()
                .map(|(order, (source, destination))| ItemResult {
                    task_id: plan.task_id.clone(),
                    input_id: source.input_id.clone(),
                    output_order: order,
                    status: "pending".into(),
                    success: false,
                    error: None,
                    error_code: None,
                    content_hash: None,
                    source_url: source.location.url.clone(),
                    url: None,
                    location: None,
                    destination_path: destination.path.clone(),
                })
                .collect(),
        }
    };
    ensure!(
        serde_json::to_value(&report.plan)? == serde_json::to_value(plan)?,
        "saved migration plan differs; create a new task"
    );
    ensure!(
        report.files.len() == plan.source.targets.len(),
        "invalid saved migration progress"
    );
    // Must be durable before any remote write.
    store(catalog, &report)?;
    for (index, (source, destination)) in plan
        .source
        .targets
        .iter()
        .zip(&plan.destinations)
        .enumerate()
    {
        if control.is_cancelled() {
            break;
        }
        if report.files[index].success {
            continue;
        }
        let operation = (|| -> Result<()> {
            ensure!(
                destination.existing_version.is_none(),
                "destination already exists; create a new plan"
            );
            let _lease = img_records::remote_lock::acquire(
                &catalog.root,
                &plan.namespace,
                &destination.path,
                true,
            )?;
            control.stage("preparing", 1);
            let existing = provider.stat_remote(&destination.path, control)?;
            let bytes = if let Some(existing) = &existing {
                let hash = report.files[index]
                    .content_hash
                    .as_ref()
                    .context("destination already exists without a saved upload intent")?;
                let bytes = provider.read_remote(
                    &destination.path,
                    &existing.version,
                    cfg.upload.max_size,
                    control,
                )?;
                ensure!(
                    digest(&bytes).eq_ignore_ascii_case(hash),
                    "destination already exists with different content"
                );
                bytes
            } else {
                if let Some(local) = report.local_sources.get(&source.input_id) {
                    let bytes = media::read_image(&local.path, cfg.upload.max_size)?;
                    ensure!(
                        digest(&bytes) == local.content_hash,
                        "selected local original changed; choose it again"
                    );
                    if let Some(hash) = &source.content_hash {
                        ensure!(
                            hash == &local.content_hash,
                            "local original does not match the known image hash"
                        );
                    }
                    bytes
                } else {
                    source_bytes(catalog, cfg, source, cfg.upload.max_size, control)?
                }
            };
            let content_type = media::inspect(&bytes, cfg.upload.max_size)?.to_owned();
            let hash = digest(&bytes);
            if let Some(expected) = &report.files[index].content_hash {
                ensure!(
                    expected == &hash,
                    "source content changed after upload intent was saved"
                );
            }
            report.files[index].content_hash = Some(hash.clone());
            report.files[index].status = "uploading".into();
            report.files[index].error = None;
            report.files[index].error_code = None;
            store(catalog, &report)?;
            let size = bytes.len() as u64;
            if existing.is_none() {
                provider.upload_versioned(
                    Request {
                        name: &source.name,
                        remote_path: &destination.path,
                        content_type: &content_type,
                        data: Arc::from(bytes),
                        overwrite: false,
                    },
                    control,
                )?;
            }
            control.check()?;
            report.files[index].status = "verifying".into();
            store(catalog, &report)?;
            let object = provider
                .stat_remote(&destination.path, control)?
                .context("uploaded object could not be found")?;
            ensure!(
                !object.version.is_empty(),
                "destination did not provide a version for verification"
            );
            let verified = provider.read_remote(
                &destination.path,
                &object.version,
                cfg.upload.max_size,
                control,
            )?;
            ensure!(
                digest(&verified) == hash,
                "uploaded object failed content verification"
            );
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let asset_id = catalog.record_copy(
                source,
                &RemoteRecord {
                    namespace: plan.namespace.clone(),
                    provider: plan.provider.clone(),
                    path: Some(destination.path.clone()),
                    url: object.url.clone(),
                    version: object.version.clone(),
                    name: source.name.clone(),
                    content_type,
                    size,
                    added_at: now,
                    origin: "migration".into(),
                    content_hash: Some(hash),
                },
            )?;
            let mut location = catalog
                .get(&asset_id)?
                .locations
                .into_iter()
                .find(|l| {
                    l.namespace == plan.namespace && l.path.as_ref() == Some(&destination.path)
                })
                .context("verified destination was not recorded")?;
            catalog.mark_location(&location.id, &location.version, "available", now)?;
            location.availability = "available".into();
            location.last_checked = Some(now);
            report.files[index].location = Some(location);
            report.files[index].url = Some(object.url);
            Ok(())
        })();
        let item = &mut report.files[index];
        match operation {
            Ok(()) => {
                item.status = "complete".into();
                item.success = true;
                item.error = None;
                item.error_code = None;
            }
            Err(error) => {
                item.status = if control.is_cancelled() {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                item.success = false;
                item.error = Some(
                    cfg.providers
                        .values()
                        .fold(error.to_string(), |message, provider| {
                            provider.sanitize(&message)
                        }),
                );
                item.error_code = Some(
                    serde_json::to_value(
                        crate::failure::Failure::from_error(
                            &error,
                            crate::failure::ErrorCode::Unknown,
                        )
                        .code,
                    )?
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
                );
            }
        }
        store(catalog, &report).context(
            "remote copy may exist, but progress could not be saved; retry this same plan",
        )?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    struct Storage {
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        requests: Arc<Mutex<Vec<(String, String)>>>,
        lose_upload: Arc<AtomicBool>,
        running: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
        config: Config,
    }
    impl Storage {
        fn new() -> Self {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let cfg = crate::config::ProviderConfig {
                kind: "s3".into(),
                endpoint: format!("http://{}", server.server_addr()),
                bucket: "test".into(),
                public_url: "https://images.test".into(),
                access_key: "test".into(),
                secret_key: "test".into(),
                path_style: true,
                allow_insecure: true,
                ..Default::default()
            };
            let mut config = Config::default();
            config.providers.insert("source".into(), cfg.clone());
            config.providers.insert("target".into(), cfg);
            let files = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));
            let requests = Arc::new(Mutex::new(vec![]));
            let lose_upload = Arc::new(AtomicBool::new(false));
            let running = Arc::new(AtomicBool::new(true));
            let (f, q, lose, run) = (
                files.clone(),
                requests.clone(),
                lose_upload.clone(),
                running.clone(),
            );
            let thread = std::thread::spawn(move || {
                while run.load(Ordering::SeqCst) {
                    let Some(mut request) = server.recv_timeout(Duration::from_millis(50)).unwrap()
                    else {
                        continue;
                    };
                    let path = request.url().strip_prefix("/test/").unwrap().to_string();
                    let method = request.method().as_str().to_string();
                    q.lock().unwrap().push((method.clone(), path.clone()));
                    let response =
                        match method.as_str() {
                            "HEAD" | "GET" => {
                                if let Some(bytes) = f.lock().unwrap().get(&path) {
                                    let etag = format!("\"{}\"", digest(bytes));
                                    if request.headers().iter().any(|h| {
                                        h.field.equiv("If-Match") && h.value.as_str() != etag
                                    }) {
                                        tiny_http::Response::from_data(vec![]).with_status_code(412)
                                    } else {
                                        tiny_http::Response::from_data(bytes.clone()).with_header(
                                            tiny_http::Header::from_bytes("ETag", etag).unwrap(),
                                        )
                                    }
                                } else {
                                    tiny_http::Response::from_data(vec![]).with_status_code(404)
                                }
                            }
                            "PUT" => {
                                assert!(
                                    request
                                        .headers()
                                        .iter()
                                        .any(|h| h.field.equiv("If-None-Match")
                                            && h.value.as_str() == "*")
                                );
                                let mut data = vec![];
                                request.as_reader().read_to_end(&mut data).unwrap();
                                let mut map = f.lock().unwrap();
                                if let std::collections::hash_map::Entry::Vacant(entry) =
                                    map.entry(path)
                                {
                                    let version = format!("\"{}\"", digest(&data));
                                    entry.insert(data);
                                    tiny_http::Response::from_data(vec![])
                                        .with_status_code(if lose.swap(false, Ordering::SeqCst) {
                                            503
                                        } else {
                                            200
                                        })
                                        .with_header(
                                            tiny_http::Header::from_bytes("ETag", version).unwrap(),
                                        )
                                } else {
                                    tiny_http::Response::from_data(vec![]).with_status_code(412)
                                }
                            }
                            _ => panic!("unexpected write: {method}"),
                        };
                    let _ = request.respond(response);
                }
            });
            Self {
                files,
                requests,
                lose_upload,
                running,
                thread: Some(thread),
                config,
            }
        }
        fn source(&self, c: &mut Catalog, name: &str) -> String {
            let bytes = [b"\x89PNG\r\n\x1a\n".as_slice(), name.as_bytes()].concat();
            self.files
                .lock()
                .unwrap()
                .insert(name.into(), bytes.clone());
            let p = Provider::new("source", &self.config.providers["source"]).unwrap();
            c.upsert(&RemoteRecord {
                namespace: p.namespace(),
                provider: "source".into(),
                path: Some(name.into()),
                url: format!("https://images.test/{name}"),
                version: format!("\"{}\"", digest(&bytes)),
                name: name.into(),
                content_type: "image/png".into(),
                size: bytes.len() as u64,
                added_at: 1,
                origin: "test".into(),
                content_hash: None,
            })
            .unwrap()
        }
        fn plan(&self, c: &Catalog, ids: &[String]) -> MigrationPlan {
            plan(
                TargetPlan::capture(c, ids, "").unwrap(),
                &Provider::new("target", &self.config.providers["target"]).unwrap(),
                "moved",
                &Control::default(),
            )
            .unwrap()
        }
        fn puts(&self) -> usize {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.0 == "PUT")
                .count()
        }
    }
    impl Drop for Storage {
        fn drop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            self.thread.take().unwrap().join().unwrap();
        }
    }
    #[test]
    fn local_recovery_requires_known_hash_and_preserves_the_chosen_file() {
        let storage = Storage::new();
        let root = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(root.path()).unwrap();
        let id = storage.source(&mut catalog, "missing.png");
        let mut plan = storage.plan(&catalog, std::slice::from_ref(&id));
        let bytes = storage.files.lock().unwrap().remove("missing.png").unwrap();
        plan.source.targets[0].content_hash = Some(digest(&bytes));
        let failed = apply(&mut catalog, &storage.config, &plan, &Control::default()).unwrap();
        assert!(!failed.files[0].success);
        assert_eq!(storage.puts(), 0);
        let file = root.path().join("reselected.png");
        std::fs::write(&file, b"\x89PNG\r\n\x1a\nwrong").unwrap();
        assert!(set_local_source(&catalog, &plan.task_id, &id, &file, 1 << 20).is_err());
        std::fs::write(&file, &bytes).unwrap();
        set_local_source(&catalog, &plan.task_id, &id, &file, 1 << 20).unwrap();
        assert_eq!(storage.puts(), 0);
        let done = apply(&mut catalog, &storage.config, &plan, &Control::default()).unwrap();
        assert!(done.files[0].success);
        assert_eq!(storage.puts(), 1);
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert!(catalog.sync_events(None).unwrap().iter().all(|event| {
            !serde_json::to_string(event)
                .unwrap()
                .contains(file.to_str().unwrap())
        }));
    }
    #[test]
    fn uncertain_upload_retry_reads_back_without_duplicate_and_preserves_new_source_version() {
        let storage = Storage::new();
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(root.path()).unwrap();
        let id = storage.source(&mut c, "source.png");
        let plan = storage.plan(&c, std::slice::from_ref(&id));
        let original = storage.files.lock().unwrap()["source.png"].clone();
        storage.lose_upload.store(true, Ordering::SeqCst);
        let first = apply(&mut c, &storage.config, &plan, &Control::default()).unwrap();
        assert!(!first.files[0].success);
        assert_eq!(storage.puts(), 1);
        assert!(first.mapping().is_empty());
        let source = c.get(&id).unwrap().locations[0].clone();
        // A later scanner observes a newer object while this task is interrupted.
        c.upsert(&RemoteRecord {
            namespace: source.namespace,
            provider: source.provider,
            path: source.path,
            url: source.url,
            version: "new-version".into(),
            name: "source.png".into(),
            content_type: "image/png".into(),
            size: 1,
            added_at: 2,
            origin: "index".into(),
            content_hash: None,
        })
        .unwrap();
        let result = apply(&mut c, &storage.config, &plan, &Control::default()).unwrap();
        assert!(result.files[0].success);
        assert_eq!(storage.puts(), 1);
        assert_eq!(result.mapping().len(), 1);
        assert_eq!(storage.files.lock().unwrap()["source.png"], original);
        let current: String = rusqlite::Connection::open(root.path().join("catalog.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT version FROM locations WHERE id=?",
                [&source.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(current, "new-version");
        let again = apply(&mut c, &storage.config, &plan, &Control::default()).unwrap();
        assert!(again.files[0].success);
        assert_eq!(storage.puts(), 1);
        let mut altered = plan.clone();
        altered.destinations[0].path.push_str("changed");
        assert!(apply(&mut c, &storage.config, &altered, &Control::default()).is_err());
    }
    #[test]
    fn cancelled_or_unsaved_intent_never_uploads_and_conflicts_are_preserved() {
        let storage = Storage::new();
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(root.path()).unwrap();
        let id = storage.source(&mut c, "source.png");
        let plan = storage.plan(&c, &[id]);
        assert_eq!(storage.puts(), 0);
        let cancelled = Control::default();
        cancelled.cancel();
        assert!(apply(&mut c, &storage.config, &plan, &cancelled).is_err());
        assert_eq!(storage.puts(), 0);
        rusqlite::Connection::open(root.path().join("catalog.sqlite3")).unwrap().execute_batch("CREATE TRIGGER fail_task BEFORE INSERT ON tasks BEGIN SELECT RAISE(ABORT,'disk full'); END;").unwrap();
        assert!(apply(&mut c, &storage.config, &plan, &Control::default()).is_err());
        assert_eq!(storage.puts(), 0);
        rusqlite::Connection::open(root.path().join("catalog.sqlite3"))
            .unwrap()
            .execute_batch("DROP TRIGGER fail_task;")
            .unwrap();
        storage.files.lock().unwrap().insert(
            plan.destinations[0].path.clone(),
            b"someone else's object".to_vec(),
        );
        let report = apply(&mut c, &storage.config, &plan, &Control::default()).unwrap();
        assert!(!report.files[0].success);
        assert_eq!(storage.puts(), 0);
        assert_eq!(
            storage.files.lock().unwrap()[&plan.destinations[0].path],
            b"someone else's object"
        );
    }
    #[test]
    fn partial_success_keeps_order_and_retries_only_missing_items() {
        let storage = Storage::new();
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(root.path()).unwrap();
        let one = storage.source(&mut c, "one.png");
        let two = storage.source(&mut c, "two.png");
        let plan = storage.plan(&c, &[one, two]);
        let missing = plan.source.targets[1].location.path.clone().unwrap();
        let bytes = storage.files.lock().unwrap().remove(&missing).unwrap();
        let first = apply(&mut c, &storage.config, &plan, &Control::default()).unwrap();
        assert!(first.files[0].success);
        assert!(!first.files[1].success);
        assert_eq!(storage.puts(), 1);
        storage.files.lock().unwrap().insert(missing, bytes);
        let result = apply(&mut c, &storage.config, &plan, &Control::default()).unwrap();
        assert!(result.files.iter().all(|r| r.success));
        assert_eq!(storage.puts(), 2);
        assert_eq!(result.files[1].output_order, 1);
        // Verified source and destination belong to the same image after copying.
        for item in result.files {
            let asset = c
                .get(&format!("sha256:{}", item.content_hash.unwrap()))
                .unwrap();
            assert_eq!(asset.locations.len(), 2);
        }
    }
}
