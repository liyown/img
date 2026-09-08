//! Immutable, per-device metadata logs. This module never issues image deletion requests.
use crate::{control::Control, provider::Provider};
use anyhow::{Context, Result, ensure};
use img_records::{
    catalog::{Catalog, digest, identity},
    sync::{Batch, Event},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::Path,
};

const MAX_BATCH_BYTES: u64 = 16 * 1024 * 1024;
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct SyncResult {
    pub pulled: usize,
    pub pushed: usize,
    pub conflicts: usize,
}
pub trait LogStorage {
    fn identity(&self) -> String;
    fn list(&self, control: &Control) -> Result<Vec<String>>;
    fn read(&self, key: &str, control: &Control) -> Result<Vec<u8>>;
    fn create(&self, key: &str, bytes: &[u8], control: &Control) -> Result<bool>;
}
pub struct RemoteLog {
    pub provider: Provider,
    pub prefix: String,
}
impl RemoteLog {
    pub fn new(provider: Provider, prefix: &str) -> Result<Self> {
        provider.sync_storage_supported()?;
        ensure!(
            !prefix.is_empty() && prefix.ends_with('/'),
            "sync requires a dedicated nonempty directory prefix ending in /"
        );
        crate::pathgen::validate(prefix.trim_end_matches('/'))?;
        Ok(Self {
            provider,
            prefix: prefix.into(),
        })
    }
}
impl LogStorage for RemoteLog {
    fn identity(&self) -> String {
        identity(&[&self.provider.namespace(), &self.prefix])
    }
    fn list(&self, control: &Control) -> Result<Vec<String>> {
        let mut pending = VecDeque::from([(self.prefix.clone(), None)]);
        let mut visited = BTreeSet::new();
        let mut files = BTreeSet::new();
        while let Some((prefix, cursor)) = pending.pop_front() {
            control.check()?;
            ensure!(
                visited.insert((prefix.clone(), cursor.clone())),
                "sync listing repeated a cursor"
            );
            let page = self.provider.list_remote(&prefix, cursor.as_deref())?;
            for item in page.items {
                let relative = item
                    .path
                    .strip_prefix(&self.prefix)
                    .context("sync listing escaped its prefix")?;
                if relative.starts_with("_probe/") {
                    continue;
                }
                if item.directory {
                    ensure!(
                        item.path.starts_with(&prefix) && item.path != prefix,
                        "sync listing returned a parent directory"
                    );
                    pending.push_back((item.path, None));
                } else if relative.ends_with(".json") {
                    files.insert(relative.to_owned());
                }
                ensure!(
                    files.len() + visited.len() + pending.len() <= 100_000,
                    "sync listing exceeds safety limit; no batches applied"
                );
            }
            if let Some(next) = page.next {
                pending.push_front((prefix, Some(next)));
            }
        }
        Ok(files.into_iter().collect())
    }
    fn read(&self, key: &str, control: &Control) -> Result<Vec<u8>> {
        crate::pathgen::validate(key)?;
        self.provider.read_remote(
            &format!("{}{key}", self.prefix),
            "",
            MAX_BATCH_BYTES,
            control,
        )
    }
    fn create(&self, key: &str, bytes: &[u8], control: &Control) -> Result<bool> {
        crate::pathgen::validate(key)?;
        self.provider
            .create_sync_object(&format!("{}{key}", self.prefix), bytes.into(), control)
    }
}
pub fn lock(root: &Path) -> Result<File> {
    std::fs::create_dir_all(root)?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("sync.lock"))?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("sync is already running on this device"))?;
    Ok(lock)
}
/// The capability probe only touches a unique disposable metadata object, never a real batch.
pub fn verify_conditions(store: &impl LogStorage, control: &Control) -> Result<()> {
    let key = format!("_probe/{}.json", uuid::Uuid::new_v4());
    let original = b"{\"probe\":1}";
    ensure!(
        store.create(&key, original, control)?,
        "sync probe unexpectedly exists"
    );
    ensure!(
        !store.create(&key, b"{\"probe\":2}", control)?,
        "storage ignores conditional writes; sync disabled"
    );
    ensure!(
        store.read(&key, control)? == original,
        "storage changed a protected sync object"
    );
    Ok(())
}
/// The caller owns the per-device lock. Secrets are exchanged explicitly by the caller;
/// they are never inferred from environment variables or arbitrary local files.
pub fn exchange(
    catalog: &mut Catalog,
    store: &impl LogStorage,
    control: &Control,
    export_secrets: impl Fn(&[Event]) -> Result<BTreeMap<String, String>>,
    import_secrets: impl Fn(&BTreeMap<String, String>) -> Result<()>,
) -> Result<SyncResult> {
    let destination = store.identity();
    let capability = format!("sync-capability:{destination}");
    if catalog.setting(&capability)?.as_deref() != Some("conditional-v1") {
        verify_conditions(store, control)?;
        catalog.set_setting(&capability, "conditional-v1")?;
    }
    // Listing must finish before anything is applied. Authentication/truncation is not deletion.
    let keys = store.list(control)?;
    let staging = tempfile::tempdir()?;
    let event_path = staging.path().join("incoming.jsonl");
    let mut incoming = BufWriter::new(File::create(&event_path)?);
    let mut acknowledged = vec![];
    let mut secrets = BTreeMap::new();
    for key in keys {
        control.check()?;
        let (device, filename) = key.split_once('/').context("invalid sync batch path")?;
        ensure!(
            uuid::Uuid::parse_str(device).is_ok(),
            "invalid sync device directory"
        );
        let hash = filename
            .strip_suffix(".json")
            .context("invalid sync batch suffix")?;
        ensure!(
            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid sync batch identity"
        );
        let receipt = format!("sync-received:{destination}:{key}");
        if catalog.setting(&receipt)?.is_some() {
            continue;
        }
        let bytes = store.read(&key, control)?;
        ensure!(
            digest(&bytes) == hash,
            "damaged sync batch; no incoming changes applied"
        );
        let batch: Batch = serde_json::from_slice(&bytes)?;
        ensure!(
            batch.version == 1 && !batch.events.is_empty(),
            "unsupported or empty sync batch"
        );
        ensure!(
            batch.events.iter().all(|e| e.device == device),
            "batch mixes device identities"
        );
        for (key, value) in batch.secrets {
            ensure!(
                secrets.get(&key).is_none_or(|old| old == &value),
                "conflicting secret payloads"
            );
            secrets.insert(key, value);
        }
        for event in batch.events {
            serde_json::to_writer(&mut incoming, &event)?;
            incoming.write_all(b"\n")?;
        }
        acknowledged.push(receipt);
    }
    // Validate on a disposable consistent snapshot before installing credentials or events.
    // Validation includes causal dependencies across files, independent of listing order.
    incoming.flush()?;
    drop(incoming);
    catalog.snapshot(&staging.path().join("catalog.sqlite3"))?;
    let mut validation = Catalog::open(staging.path())?;
    validation.sync_ingest_stream(read_events(&event_path)?)?;
    validation.sync_materialize_catalog()?;
    drop(validation);
    import_secrets(&secrets)?;
    let pulled = catalog.sync_ingest_stream(read_events(&event_path)?)?;
    let mut acknowledgements = Vec::with_capacity(200);
    for event in read_events(&event_path)? {
        acknowledgements.push(event?);
        if acknowledgements.len() == 200 {
            catalog.sync_ack(&destination, &acknowledgements)?;
            acknowledgements.clear();
        }
    }
    catalog.sync_ack(&destination, &acknowledgements)?;
    for receipt in acknowledged {
        catalog.set_setting(&receipt, "1")?;
    }
    let mut pushed = 0;
    loop {
        control.check()?;
        let events = catalog.sync_outbox(&destination)?;
        if events.is_empty() {
            break;
        }
        let device = &events[0].device;
        let events = events
            .iter()
            .take_while(|e| &e.device == device)
            .cloned()
            .collect::<Vec<_>>();
        let batch = Batch {
            version: 1,
            secrets: export_secrets(&events)?,
            events: events.clone(),
        };
        let bytes = serde_json::to_vec(&batch)?;
        ensure!(
            bytes.len() as u64 <= MAX_BATCH_BYTES,
            "sync batch exceeds size limit"
        );
        let key = format!("{device}/{}.json", digest(&bytes));
        store.create(&key, &bytes, control)?;
        ensure!(
            store.read(&key, control)? == bytes,
            "sync upload verification failed"
        );
        catalog.sync_ack(&destination, &events)?;
        pushed += events.len();
    }
    Ok(SyncResult {
        pulled,
        pushed,
        conflicts: catalog.sync_conflicts()?.len(),
    })
}

fn read_events(path: &Path) -> Result<impl Iterator<Item = Result<Event>>> {
    Ok(
        serde_json::Deserializer::from_reader(BufReader::new(File::open(path)?))
            .into_iter::<Event>()
            .map(|event| event.map_err(Into::into)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    struct Server {
        endpoint: String,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
        objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    }
    impl Server {
        fn start(kind: &str, ignore_conditions: bool) -> Self {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", server.server_addr());
            let stop = Arc::new(AtomicBool::new(false));
            let done = stop.clone();
            let objects = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
            let data = objects.clone();
            let kind = kind.to_owned();
            let thread = std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    let Some(mut req) = server
                        .recv_timeout(std::time::Duration::from_millis(20))
                        .unwrap()
                    else {
                        continue;
                    };
                    let url = url::Url::parse(&format!("http://test{}", req.url())).unwrap();
                    let path = url
                        .path()
                        .trim_start_matches('/')
                        .trim_start_matches("bucket/")
                        .to_string();
                    let mut map = data.lock().unwrap();
                    let response = match req.method().as_str() {
                        "MKCOL" => tiny_http::Response::from_data(vec![]).with_status_code(201),
                        "PUT" => {
                            assert!(
                                req.headers()
                                    .iter()
                                    .any(|h| h.field.equiv("If-None-Match")
                                        && h.value.as_str() == "*")
                            );
                            if map.contains_key(&path) && !ignore_conditions {
                                tiny_http::Response::from_data(vec![]).with_status_code(412)
                            } else {
                                let mut bytes = vec![];
                                req.as_reader().read_to_end(&mut bytes).unwrap();
                                map.insert(path, bytes);
                                tiny_http::Response::from_data(vec![]).with_status_code(201)
                            }
                        }
                        "GET" if url.query().is_none() => match map.get(&path) {
                            Some(bytes) => tiny_http::Response::from_data(bytes.clone()),
                            None => tiny_http::Response::from_data(vec![]).with_status_code(404),
                        },
                        "GET" | "PROPFIND" => {
                            let prefix = if kind == "s3" {
                                url.query_pairs()
                                    .find(|(k, _)| k == "prefix")
                                    .unwrap()
                                    .1
                                    .into_owned()
                            } else {
                                path
                            };
                            let mut dirs = BTreeSet::new();
                            let mut files = vec![];
                            for (key, bytes) in map.iter() {
                                if let Some(relative) = key.strip_prefix(&prefix) {
                                    if let Some((dir, _)) = relative.split_once('/') {
                                        dirs.insert(format!("{prefix}{dir}/"));
                                    } else {
                                        files.push((key, bytes.len()));
                                    }
                                }
                            }
                            let xml = if kind == "s3" {
                                format!("<ListBucketResult><IsTruncated>false</IsTruncated>{}{}</ListBucketResult>", dirs.iter().map(|d|format!("<CommonPrefixes><Prefix>{d}</Prefix></CommonPrefixes>")).collect::<String>(), files.iter().map(|(k,n)|format!("<Contents><Key>{k}</Key><Size>{n}</Size><ETag>v1</ETag></Contents>")).collect::<String>())
                            } else {
                                format!("<d:multistatus xmlns:d=\"DAV:\">{}{}</d:multistatus>", dirs.iter().map(|d|format!("<d:response><d:href>/{d}</d:href><d:propstat><d:status>HTTP/1.1 200 OK</d:status><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>")).collect::<String>(), files.iter().map(|(k,n)|format!("<d:response><d:href>/{k}</d:href><d:propstat><d:status>HTTP/1.1 200 OK</d:status><d:prop><d:getcontentlength>{n}</d:getcontentlength></d:prop></d:propstat></d:response>")).collect::<String>())
                            };
                            tiny_http::Response::from_data(xml.into_bytes())
                                .with_status_code(if kind == "s3" { 200 } else { 207 })
                        }
                        method => panic!("unexpected sync request {method}"),
                    };
                    req.respond(response).unwrap();
                }
            });
            Self {
                endpoint,
                stop,
                thread: Some(thread),
                objects,
            }
        }
        fn log(&self, kind: &str) -> RemoteLog {
            RemoteLog::new(
                Provider::new(
                    "sync",
                    &crate::config::ProviderConfig {
                        kind: kind.into(),
                        endpoint: self.endpoint.clone(),
                        bucket: "bucket".into(),
                        path_style: true,
                        access_key: "test".into(),
                        secret_key: "test".into(),
                        allow_insecure: true,
                        public_url: "https://test.example".into(),
                        ..Default::default()
                    },
                )
                .unwrap(),
                "sync/",
            )
            .unwrap()
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.thread.take().unwrap().join().unwrap();
        }
    }
    fn run(c: &mut Catalog, log: &impl LogStorage) -> Result<SyncResult> {
        exchange(
            c,
            log,
            &Control::default(),
            |_| Ok(BTreeMap::new()),
            |values| {
                ensure!(values.is_empty(), "unexpected credentials");
                Ok(())
            },
        )
    }
    #[test]
    fn two_devices_exchange_immutable_logs_over_webdav_and_s3() {
        for kind in ["webdav", "s3"] {
            let server = Server::start(kind, false);
            let log = server.log(kind);
            let ta = tempfile::tempdir().unwrap();
            let tb = tempfile::tempdir().unwrap();
            let mut a = Catalog::open(ta.path()).unwrap();
            let mut b = Catalog::open(tb.path()).unwrap();
            a.sync_set("asset:image", "name", Some("old".into()))
                .unwrap();
            assert_eq!(run(&mut a, &log).unwrap().pushed, 1);
            assert_eq!(run(&mut b, &log).unwrap().pulled, 1);
            a.sync_set("asset:image", "name", Some("A".into())).unwrap();
            b.sync_set("asset:image", "name", Some("B".into())).unwrap();
            run(&mut b, &log).unwrap();
            run(&mut a, &log).unwrap();
            run(&mut b, &log).unwrap();
            assert_eq!(b.sync_conflicts().unwrap().len(), 1);
            assert_eq!(run(&mut b, &log).unwrap().pulled, 0);
            a.sync_set("asset:image", "name", Some("resolved".into()))
                .unwrap();
            run(&mut a, &log).unwrap();
            run(&mut b, &log).unwrap();
            assert!(b.sync_conflicts().unwrap().is_empty());
            assert_eq!(
                b.sync_entity("asset:image").unwrap().fields["name"],
                "resolved"
            );
            let asset = a
                .upsert(&img_records::catalog::RemoteRecord {
                    namespace: "images".into(),
                    provider: "photos".into(),
                    path: Some("photo.png".into()),
                    url: "https://cdn.test/photo.png".into(),
                    version: "v1".into(),
                    name: "photo.png".into(),
                    content_type: "image/png".into(),
                    size: 123,
                    added_at: 1,
                    origin: "test".into(),
                    content_hash: None,
                })
                .unwrap();
            img_records::cache::Cache::open(ta.path())
                .unwrap()
                .put(b"private cached pixels")
                .unwrap();
            a.set_hidden(&asset, true).unwrap();
            run(&mut a, &log).unwrap();
            run(&mut b, &log).unwrap();
            b.sync_materialize_catalog().unwrap();
            assert!(b.get(&asset).unwrap().hidden);
            assert_eq!(
                b.get(&asset).unwrap().locations[0].url,
                "https://cdn.test/photo.png"
            );
            assert!(!tb.path().join("cache").exists());
            let objects = server.objects.lock().unwrap();
            assert!(
                objects
                    .keys()
                    .all(|k| k.starts_with("sync/") && k.ends_with(".json"))
            );
            assert!(
                objects
                    .values()
                    .all(|v| !String::from_utf8_lossy(v).contains("cache_path")
                        && !String::from_utf8_lossy(v).contains("private cached pixels"))
            );
        }
    }
    #[test]
    fn ignored_conditions_and_damaged_batches_never_apply() {
        let server = Server::start("webdav", true);
        assert!(verify_conditions(&server.log("webdav"), &Control::default()).is_err());
        let server = Server::start("s3", false);
        let log = server.log("s3");
        let t = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(t.path()).unwrap();
        let key = format!("sync/{}/{}.json", uuid::Uuid::new_v4(), "0".repeat(64));
        server
            .objects
            .lock()
            .unwrap()
            .insert(key, b"damaged".to_vec());
        assert!(run(&mut c, &log).is_err());
        assert!(c.sync_events(None).unwrap().is_empty());
    }
    #[test]
    fn local_sync_lock_excludes_other_process_handles() {
        let t = tempfile::tempdir().unwrap();
        let first = lock(t.path()).unwrap();
        assert!(lock(t.path()).is_err());
        drop(first);
        assert!(lock(t.path()).is_ok());
    }
}
