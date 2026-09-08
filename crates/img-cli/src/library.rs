use crate::args::LibraryCommand;
use anyhow::{Context, Result, ensure};
use img_core::{config, control::Control, link_check, network};
use img_records::catalog::{Catalog, CatalogQuery};
use std::{
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
pub fn run(config_path: &Path, command: LibraryCommand, control: &Control) -> Result<i32> {
    match execute(config_path, command, control) {
        Ok(status) => Ok(status),
        Err(error) => {
            let message = match config::read_global(config_path) {
                Ok(cfg) => cfg
                    .providers
                    .values()
                    .fold(error.to_string(), |message, provider| {
                        provider.sanitize(&message)
                    }),
                Err(_) => "Cannot read storage configuration".into(),
            };
            println!(
                "{}",
                serde_json::json!({"success":false,"error":message,"code":"library_operation_failed"})
            );
            Ok(1)
        }
    }
}
fn execute(config_path: &Path, command: LibraryCommand, control: &Control) -> Result<i32> {
    let root = img_records::data_dir()?;
    let mut c = Catalog::open(&root)?;
    c.import_legacy()?;
    let mut failed = false;
    match command {
        LibraryCommand::DeletePlan {
            ids,
            provider,
            output,
        } => {
            let plan = img_core::library_ops::plan_delete(&c, &ids, &provider)?;
            ensure!(!output.exists(), "plan output already exists");
            let mut file = std::fs::File::options()
                .create_new(true)
                .write(true)
                .open(output)?;
            file.write_all(&serde_json::to_vec_pretty(&plan)?)?;
            file.sync_all()?;
            println!("{}", serde_json::to_string(&plan)?);
        }
        LibraryCommand::Delete { plan } => {
            let plan = serde_json::from_slice(&std::fs::read(plan)?)?;
            let config = config::read_global(config_path)?;
            let files = img_core::library_ops::delete(&mut c, &config, &plan, control)?;
            failed = files.iter().any(|r| !r.success);
            println!("{}", serde_json::json!({"success":!failed,"files":files}));
        }
        LibraryCommand::Preview {
            id,
            provider,
            cache_only,
        } => {
            let asset = c.get(&id)?;
            let location = asset
                .selected_location(&provider)
                .context("no matching address")?
                .clone();
            let cfg = config::read_global(config_path)?;
            let bytes = if let Some(key) = &location.path {
                let p = crate::provider(&cfg, &location.provider)?;
                ensure!(
                    p.namespace() == location.namespace,
                    "storage identity changed; refresh first"
                );
                p.read_remote(key, &location.version, cfg.upload.max_size, control)?
            } else {
                network::fetch(&location.url, cfg.upload.max_size, false)?.data
            };
            let ct = img_core::media::inspect(&bytes, cfg.upload.max_size)?.to_string();
            let hash = img_records::catalog::digest(&bytes);
            if asset.content_hash.as_ref().is_some_and(|h| h != &hash) {
                c.mark_location(
                    &location.id,
                    &location.version,
                    "unknown",
                    SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                )?;
                anyhow::bail!("remote image changed; refresh before previewing this version");
            }
            let cache = img_records::cache::Cache::open(&root)?;
            let key = cache.put(&bytes)?;
            let lease = cache.lease(&key)?;
            let asset_id = if cache_only {
                c.set_setting(&format!("preview:{}", asset.id), &key)?;
                asset.id.clone()
            } else {
                c.upsert(&img_records::catalog::RemoteRecord {
                    namespace: location.namespace,
                    provider: location.provider,
                    path: location.path,
                    url: location.url,
                    version: location.version.clone(),
                    name: asset.name,
                    content_type: ct,
                    size: bytes.len() as u64,
                    added_at: asset.added_at,
                    origin: asset.origin,
                    content_hash: Some(hash),
                })?
            };
            c.mark_location(
                &location.id,
                &location.version,
                "available",
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            )?;
            cache.trim(cache.limit()?)?;
            println!(
                "{}",
                serde_json::json!({"id":asset_id,"cache_key":key,"path":lease.path})
            );
        }
        LibraryCommand::Scopes { id, enabled } => {
            if let Some(enabled) = enabled {
                let id = id.context("--enabled requires --id")?;
                let key = format!("scope:{id}");
                let mut scope: img_core::index::Scope =
                    serde_json::from_str(&c.setting(&key)?.context("index scope not found")?)?;
                scope.enabled = enabled;
                c.sync_set(&key, "enabled", Some(enabled.into()))?;
                c.set_setting(&key, &serde_json::to_string(&scope)?)?;
            }
            let scopes = c
                .settings_prefix("scope:")?
                .into_iter()
                .map(|(_, s)| serde_json::from_str::<serde_json::Value>(&s))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            println!("{}", serde_json::to_string(&scopes)?);
        }
        LibraryCommand::Index {
            provider,
            prefix,
            resume,
            scope_id,
        } => {
            let cfg = config::read_global(config_path)?;
            let provider = crate::provider(&cfg, &provider)?;
            let scope = img_core::index::Scope::new(&provider, &prefix)?;
            if let Some(expected) = scope_id {
                ensure!(
                    scope.id == expected,
                    "storage identity changed; choose the indexing scope again"
                );
                let saved: img_core::index::Scope = serde_json::from_str(
                    &c.setting(&format!("scope:{expected}"))?
                        .context("index scope no longer exists")?,
                )?;
                ensure!(saved.enabled, "index scope is paused");
            }
            let excluded = img_core::sync_settings::SyncSettings::read(&root)
                .ok()
                .filter(|s| s.namespace == scope.namespace)
                .map(|s| vec![s.prefix])
                .unwrap_or_default();
            let scan = img_core::index::run(&mut c, &provider, scope, resume, &excluded, control)?;
            failed = !scan.complete;
            println!("{}", serde_json::to_string(&scan)?);
        }
        LibraryCommand::List {
            search,
            provider,
            prefix,
            content_type,
            origin,
            availability,
            since,
            until,
            hidden,
            limit,
            offset,
        } => println!(
            "{}",
            serde_json::to_string(&c.query(&CatalogQuery {
                text: search,
                provider,
                prefix,
                content_type,
                origin,
                availability,
                since,
                until,
                include_hidden: hidden,
                limit,
                offset,
            })?)?
        ),
        LibraryCommand::Show { id } => {
            let mut value = serde_json::to_value(c.get(&id)?)?;
            value["versions"]=serde_json::to_value(c.related_versions(&id)?.into_iter().map(|(asset,recipe,ancestor)|serde_json::json!({"image":asset,"recipe":recipe,"ancestor":ancestor})).collect::<Vec<_>>())?;
            println!("{}", value);
        }
        LibraryCommand::Hide { ids, restore } => {
            c.set_hidden_many(&ids, !restore)?;
            println!("{}", serde_json::json!({"updated":ids.len()}));
        }
        LibraryCommand::Cache { clear, limit_mib } => {
            let cache = img_records::cache::Cache::open(&root)?;
            if let Some(limit) = limit_mib {
                cache.set_limit(
                    limit
                        .checked_mul(1024 * 1024)
                        .context("cache limit overflow")?,
                )?;
            }
            let removed = cache.trim(if clear { 0 } else { cache.limit()? })?;
            let (bytes, protected_bytes) = cache.usage()?;
            println!(
                "{}",
                serde_json::json!({"removed_bytes":removed,"bytes":bytes,"protected_bytes":protected_bytes,"limit_bytes":cache.limit()?})
            );
        }
        LibraryCommand::Check {
            ids,
            plan,
            provider,
            allow_insecure,
        } => {
            let plan = read_plan(&c, plan.as_deref(), &ids, &provider)?;
            let targets = &plan.targets;
            let mut results = vec![];
            for (index, asset) in targets.iter().enumerate() {
                control.check()?;
                let location = &asset.location;
                let result = link_check::check(&location.url, allow_insecure);
                failed |= !result.accessible;
                let state = if result.accessible {
                    "available"
                } else {
                    match result.code {
                        "access_denied" => "forbidden",
                        "not_found" => "pending-missing",
                        _ => "error",
                    }
                };
                c.mark_location(
                    &location.id,
                    &location.version,
                    state,
                    SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                )?;
                results.push(
                    serde_json::json!({"id":asset.input_id,"input_id":asset.input_id,"task_id":plan.task_id,"output_order":index,"success":result.accessible,"location_id":location.id,"location":location,"check":result}),
                );
            }
            println!("{}", serde_json::json!({"success":!failed,"files":results}));
        }
        LibraryCommand::Download {
            ids,
            plan,
            output_dir,
            provider,
        } => {
            ensure!(output_dir.is_dir(), "choose an existing output directory");
            let cfg = config::read_global(config_path)?;
            let plan = read_plan(&c, plan.as_deref(), &ids, &provider)?;
            let targets = &plan.targets;
            let mut results = vec![];
            for (index, asset) in targets.iter().enumerate() {
                control.check()?;
                let result = (|| -> Result<_> {
                    let location = &asset.location;
                    let bytes = if let Some(key) = &location.path {
                        let p = crate::provider(&cfg, &location.provider)?;
                        ensure!(
                            p.namespace() == location.namespace,
                            "provider identity changed; refresh first"
                        );
                        p.read_remote(key, &location.version, cfg.upload.max_size, control)?
                    } else {
                        network::fetch(&location.url, cfg.upload.max_size, false)?.data
                    };
                    img_core::media::inspect(&bytes, cfg.upload.max_size)?;
                    if let Some(expected) = &asset.content_hash {
                        ensure!(
                            &img_records::catalog::digest(&bytes) == expected,
                            "remote content no longer matches the selected image"
                        );
                    }
                    let name = Path::new(&asset.name)
                        .file_name()
                        .and_then(|v| v.to_str())
                        .filter(|v| !v.contains(['\\', '/']) && *v != "." && *v != "..")
                        .unwrap_or("image");
                    let path = output_dir.join(format!("{:04}-{name}", index + 1));
                    ensure!(!path.exists(), "destination already exists");
                    let mut file = tempfile::NamedTempFile::new_in(&output_dir)?;
                    file.write_all(&bytes)?;
                    file.as_file().sync_all()?;
                    file.persist_noclobber(&path).map_err(|e| e.error)?;
                    Ok(path)
                })();
                match result {
                    Ok(path) => results
                        .push(serde_json::json!({"id":asset.input_id,"input_id":asset.input_id,"task_id":plan.task_id,"output_order":index,"success":true,"output":path,"location":asset.location})),
                    Err(e) => {
                        failed = true;
                        results.push(serde_json::json!({"id":asset.input_id,"input_id":asset.input_id,"task_id":plan.task_id,"output_order":index,"success":false,"error":e.to_string(),"error_code":img_core::failure::Failure::from_error(&e,img_core::failure::ErrorCode::Unknown).code,"location":asset.location}));
                    }
                }
            }
            println!("{}", serde_json::json!({"success":!failed,"files":results}));
        }
    }
    Ok(if failed { 1 } else { 0 })
}

fn read_plan(
    catalog: &Catalog,
    path: Option<&Path>,
    ids: &[String],
    provider: &str,
) -> Result<img_records::targets::TargetPlan> {
    let plan = match path {
        Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        None => img_records::targets::TargetPlan::capture(catalog, ids, provider)?,
    };
    plan.validate()?;
    Ok(plan)
}
