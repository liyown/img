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
        LibraryCommand::Index {
            provider,
            prefix,
            resume,
        } => {
            let cfg = config::read_global(config_path)?;
            let provider = crate::provider(&cfg, &provider)?;
            let scope = img_core::index::Scope::new(&provider, &prefix)?;
            let scan = img_core::index::run(&mut c, &provider, scope, resume, &[], control)?;
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
                include_hidden: hidden,
                limit,
                offset,
                ..Default::default()
            })?)?
        ),
        LibraryCommand::Show { id } => println!("{}", serde_json::to_string(&c.get(&id)?)?),
        LibraryCommand::Hide { ids, restore } => {
            for id in &ids {
                c.get(id)?;
            }
            for id in &ids {
                c.set_hidden(id, !restore)?;
            }
            println!("{}", serde_json::json!({"updated":ids.len()}));
        }
        LibraryCommand::Cache { clear } => {
            let cache = img_records::cache::Cache::open(&root)?;
            println!(
                "{}",
                serde_json::json!({"removed_bytes":cache.trim(if clear{0}else{img_records::cache::DEFAULT_LIMIT})?})
            );
        }
        LibraryCommand::Check {
            ids,
            provider,
            allow_insecure,
        } => {
            let targets = ids.iter().map(|id| c.get(id)).collect::<Result<Vec<_>>>()?;
            let mut results = vec![];
            for asset in targets {
                control.check()?;
                let Some(location) = asset.selected_location(&provider) else {
                    failed = true;
                    results.push(serde_json::json!({"id":asset.id,"error":"no matching address"}));
                    continue;
                };
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
                    serde_json::json!({"id":asset.id,"location_id":location.id,"check":result}),
                );
            }
            println!("{}", serde_json::json!({"success":!failed,"files":results}));
        }
        LibraryCommand::Download {
            ids,
            output_dir,
            provider,
        } => {
            ensure!(output_dir.is_dir(), "choose an existing output directory");
            let cfg = config::read_global(config_path)?;
            let targets = ids.iter().map(|id| c.get(id)).collect::<Result<Vec<_>>>()?;
            let mut results = vec![];
            for (index, asset) in targets.iter().enumerate() {
                control.check()?;
                let result = (|| -> Result<_> {
                    let location = asset
                        .selected_location(&provider)
                        .context("no matching address")?;
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
                        .push(serde_json::json!({"id":asset.id,"success":true,"output":path})),
                    Err(e) => {
                        failed = true;
                        results.push(serde_json::json!({"id":asset.id,"success":false,"error":e.to_string()}));
                    }
                }
            }
            println!("{}", serde_json::json!({"success":!failed,"files":results}));
        }
    }
    Ok(if failed { 1 } else { 0 })
}
