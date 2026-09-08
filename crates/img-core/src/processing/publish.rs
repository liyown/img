//! Publishing processed outputs preserves their exact bytes and records their ancestry.
use super::{
    ProcessingPlan,
    batch::{Output, ProcessingTask},
};
use crate::{
    config::Config,
    control::Control,
    media, network, pathgen,
    provider::{Provider, Request},
};
use anyhow::{Context, Result, ensure};
use img_records::catalog::{Catalog, RemoteRecord, digest};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Clone, Serialize, Deserialize)]
pub struct Published {
    pub input_id: String,
    pub output_order: usize,
    pub source: Output,
    pub parents: Vec<String>,
    pub remote_path: String,
    pub status: String,
    pub success: bool,
    pub url: Option<String>,
    pub version: String,
    pub location: Option<img_records::catalog::Location>,
    pub error: Option<String>,
    pub error_code: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PublishTask {
    pub version: u32,
    pub task_id: String,
    pub process_task_id: String,
    pub provider: String,
    pub namespace: String,
    pub plan: ProcessingPlan,
    pub files: Vec<Published>,
    pub record_warning: Option<String>,
}
impl PublishTask {
    pub fn create(process: &ProcessingTask, target: &Provider, prefix: &str) -> Result<Self> {
        let prefix = prefix.trim_end_matches('/');
        if !prefix.is_empty() {
            pathgen::validate(prefix)?;
        }
        let task_id = uuid::Uuid::new_v4().to_string();
        let prefix = if prefix.is_empty() {
            "processed"
        } else {
            prefix
        };
        let parents = process
            .inputs
            .iter()
            .filter_map(|input| input.source_asset.clone())
            .collect::<Vec<_>>();
        let mut files = vec![];
        for output in process.files.iter().flat_map(|file| &file.outputs) {
            let id = &output.input_id;
            let source_parents = if process.plan.stitch.is_some() {
                parents.clone()
            } else {
                process
                    .inputs
                    .iter()
                    .find(|input| &input.input_id == id)
                    .and_then(|input| input.source_asset.clone())
                    .into_iter()
                    .collect()
            };
            files.push(Published {
                input_id: id.clone(),
                output_order: output.output_order,
                source: output.clone(),
                parents: source_parents,
                remote_path: format!(
                    "{prefix}/{task_id}/{:06}.{}",
                    output.output_order + 1,
                    process.plan.encoding.format.extension()
                ),
                status: "pending".into(),
                success: false,
                url: None,
                version: String::new(),
                location: None,
                error: None,
                error_code: None,
            });
        }
        ensure!(!files.is_empty(), "processing task has no saved outputs");
        Ok(Self {
            version: 1,
            task_id,
            process_task_id: process.task_id.clone(),
            provider: target.name.clone(),
            namespace: target.namespace(),
            plan: process.plan.clone(),
            files,
            record_warning: None,
        })
    }
    pub fn load(catalog: &Catalog, id: &str) -> Result<Self> {
        ensure!(
            uuid::Uuid::parse_str(id).is_ok(),
            "invalid publishing task ID"
        );
        let task: Self = serde_json::from_str(&catalog.task(&format!("publish:{id}"))?)?;
        ensure!(
            task.task_id == id && task.version == 1,
            "unsupported publishing task"
        );
        Ok(task)
    }
    pub fn save(&self, catalog: &Catalog) -> Result<()> {
        catalog.save_task(
            &format!("publish:{}", self.task_id),
            "publish",
            &serde_json::to_string(self)?,
        )
    }
}
pub fn run(
    catalog: &mut Catalog,
    config: &Config,
    task: &mut PublishTask,
    control: &Control,
) -> Result<()> {
    ensure!(
        task.version == 1 && uuid::Uuid::parse_str(&task.task_id).is_ok(),
        "unsupported publishing task"
    );
    task.plan.validate()?;
    let _task_lock =
        img_records::remote_lock::acquire(&catalog.root, "publishing-task", &task.task_id, true)?;
    let cfg = config
        .providers
        .get(&task.provider)
        .context("destination storage is missing")?;
    let provider = Provider::new(&task.provider, cfg)?;
    ensure!(
        provider.namespace() == task.namespace,
        "destination storage changed; create a new task"
    );
    if let Some(body) = catalog.task_optional(&format!("publish:{}", task.task_id))? {
        let saved: PublishTask = serde_json::from_str(&body)?;
        let targets = |files: &[Published]| {
            files
                .iter()
                .map(|file| {
                    (
                        &file.input_id,
                        file.output_order,
                        &file.source.path,
                        &file.source.image.content_hash,
                        &file.remote_path,
                        &file.parents,
                    )
                })
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
        };
        ensure!(
            saved.plan == task.plan
                && saved.provider == task.provider
                && saved.namespace == task.namespace
                && targets(&saved.files)? == targets(&task.files)?,
            "saved publishing targets differ"
        );
        task.files = saved.files;
        task.record_warning = None;
    }
    task.save(catalog)?;
    for index in 0..task.files.len() {
        if control.is_cancelled() {
            break;
        }
        if task.files[index].success {
            continue;
        }
        let operation = (|| -> Result<()> {
            let file = &task.files[index];
            pathgen::validate(&file.remote_path)?;
            let _lease = img_records::remote_lock::acquire(
                &catalog.root,
                &task.namespace,
                &file.remote_path,
                true,
            )?;
            let bytes = media::read_image(&file.source.path, config.upload.max_size)?;
            ensure!(
                digest(&bytes) == file.source.image.content_hash,
                "processed output changed; export it again"
            );
            let ct = media::detect(&bytes)?;
            if provider.supports_remote_management() {
                if let Some(existing) = provider.stat_remote(&file.remote_path, control)? {
                    let observed = provider.read_remote(
                        &file.remote_path,
                        &existing.version,
                        config.upload.max_size,
                        control,
                    )?;
                    ensure!(
                        digest(&observed) == file.source.image.content_hash,
                        "remote target already exists with different content"
                    );
                    task.files[index].url = Some(existing.url);
                    task.files[index].version = existing.version;
                } else {
                    task.files[index].url = None;
                    task.files[index].version.clear();
                }
            } else {
                ensure!(
                    file.status != "uploading" || file.url.is_some(),
                    "the HTTP service cannot verify the previous upload attempt; inspect its history before submitting again"
                );
            }
            if task.files[index].url.is_none() {
                task.files[index].status = "uploading".into();
                task.save(catalog)?;
                let receipt = provider.upload_versioned(
                    Request {
                        name: &format!("output.{}", task.plan.encoding.format.extension()),
                        remote_path: &task.files[index].remote_path,
                        content_type: ct,
                        data: Arc::from(bytes),
                        overwrite: false,
                    },
                    control,
                )?;
                task.files[index].url = Some(receipt.url);
                task.files[index].version = receipt.version;
                task.files[index].status = "verifying".into();
                // Persist the receipt before another network request; retries never repost a known HTTP result.
                task.save(catalog)?;
            }
            let file = &task.files[index];
            let url = file.url.as_deref().context("uploaded URL missing")?;
            let verified = if provider.supports_remote_management() {
                let object = provider
                    .stat_remote(&file.remote_path, control)?
                    .context("uploaded object was not found")?;
                ensure!(
                    !object.version.is_empty(),
                    "storage did not provide a version for verification"
                );
                let verified = provider.read_remote(
                    &file.remote_path,
                    &object.version,
                    config.upload.max_size,
                    control,
                )?;
                task.files[index].version = object.version;
                verified
            } else {
                network::fetch(url, config.upload.max_size, cfg.allow_insecure)?.data
            };
            let file = &task.files[index];
            ensure!(
                digest(&verified) == file.source.image.content_hash,
                "uploaded output failed content verification"
            );
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let asset = catalog.upsert(&RemoteRecord {
                namespace: task.namespace.clone(),
                provider: task.provider.clone(),
                path: provider
                    .supports_remote_management()
                    .then(|| file.remote_path.clone()),
                url: file.url.clone().context("uploaded URL missing")?,
                version: file.version.clone(),
                name: file
                    .source
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                content_type: file.source.image.content_type.clone(),
                size: file.source.image.size,
                added_at: now,
                origin: "tools".into(),
                content_hash: Some(file.source.image.content_hash.clone()),
            })?;
            for parent in &file.parents {
                if parent != &asset {
                    catalog.link_version(parent, &asset, &serde_json::to_string(&task.plan)?)?;
                }
            }
            let location = catalog
                .get(&asset)?
                .locations
                .into_iter()
                .find(|location| {
                    location.namespace == task.namespace && Some(&location.url) == file.url.as_ref()
                })
                .context("published location missing")?;
            catalog.mark_location(&location.id, &location.version, "available", now)?;
            task.files[index].location = Some(location);
            Ok(())
        })();
        let file = &mut task.files[index];
        match operation {
            Ok(()) => {
                file.success = true;
                file.status = "complete".into();
                file.error = None;
                file.error_code = None;
            }
            Err(error) => {
                if file.status != "uploading"
                    || provider.supports_remote_management()
                    || file.url.is_some()
                {
                    file.status = if control.is_cancelled() {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                }
                file.error = Some(cfg.sanitize(&error.to_string()));
                file.error_code = Some(
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
        if task.save(catalog).is_err() {
            task.record_warning=Some("Uploaded URLs are included below, but local progress could not be saved. Retry the same task after restoring storage access.".into());
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::batch::{self, Input};
    #[test]
    fn publish_preserves_export_bytes_links_ancestry_and_retries_verification_without_reupload() {
        let root = tempfile::tempdir().unwrap();
        let mut c = Catalog::open(&root.path().join("data")).unwrap();
        let mut png = std::io::Cursor::new(vec![]);
        image::DynamicImage::new_rgba8(17, 19)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let original = png.into_inner();
        let file = root.path().join("original.png");
        std::fs::write(&file, &original).unwrap();
        let parent = c
            .upsert(&RemoteRecord {
                namespace: "old-storage".into(),
                provider: "old".into(),
                path: Some("original.png".into()),
                url: "https://old.test/original.png".into(),
                version: "v1".into(),
                name: "original.png".into(),
                content_type: "image/png".into(),
                size: original.len() as u64,
                added_at: 1,
                origin: "upload".into(),
                content_hash: None,
            })
            .unwrap();
        let mut input = Input::snapshot(&file, 1 << 20).unwrap();
        input.source_asset = Some(parent.clone());
        let mut processing = ProcessingTask::create(
            vec![input],
            ProcessingPlan::default(),
            None,
            &root.path().join("outputs"),
            1 << 20,
        )
        .unwrap();
        batch::run(&c, &mut processing, &Control::default()).unwrap();
        let expected = std::fs::read(&processing.files[0].outputs[0].path).unwrap();
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr());
        let cfg = crate::config::ProviderConfig {
            kind: "http".into(),
            url: format!("{endpoint}/upload"),
            url_json_path: "url".into(),
            allow_insecure: true,
            ..Default::default()
        };
        let provider = Provider::new("target", &cfg).unwrap();
        let mut config = Config::default();
        config.providers.insert("target".into(), cfg);
        config.upload.recipe.format = "jpeg".into();
        config.upload.max_width = 1;
        let mut task = PublishTask::create(&processing, &provider, "tools").unwrap();
        let thread = std::thread::spawn(move || {
            let mut request = server
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(request.method().as_str(), "POST");
            let mut body = vec![];
            request.as_reader().read_to_end(&mut body).unwrap();
            assert!(body.windows(expected.len()).any(|bytes| bytes == expected));
            request
                .respond(tiny_http::Response::from_string(
                    serde_json::json!({"url":format!("{endpoint}/image.png")}).to_string(),
                ))
                .unwrap();
            let request = server
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(request.method().as_str(), "GET");
            request.respond(tiny_http::Response::empty(503)).unwrap();
            let request = server
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(request.method().as_str(), "GET");
            request
                .respond(tiny_http::Response::from_data(expected))
                .unwrap();
        });
        run(&mut c, &config, &mut task, &Control::default()).unwrap();
        assert!(!task.files[0].success);
        assert!(task.files[0].url.is_some());
        run(&mut c, &config, &mut task, &Control::default()).unwrap();
        assert!(task.files[0].success);
        assert!(task.files[0].location.as_ref().unwrap().path.is_none());
        let asset_id = &task.files[0].location.as_ref().unwrap().asset_id;
        assert_eq!(c.related_versions(asset_id).unwrap()[0].0.id, parent);
        assert_eq!(std::fs::read(file).unwrap(), original);
        run(&mut c, &config, &mut task, &Control::default()).unwrap();
        thread.join().unwrap();
    }
}
