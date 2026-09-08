use crate::args::SyncCommand;
use anyhow::{Context, Result, ensure};
use img_core::{control::Control, sync_log, sync_settings::SyncSettings};
use img_records::catalog::Catalog;
use serde_json::json;
pub fn run(config_path: &std::path::Path, command: SyncCommand, control: &Control) -> Result<i32> {
    match execute(config_path, command, control) {
        Ok(code) => Ok(code),
        Err(error) => {
            let failure = img_core::failure::Failure::from_error(
                &error,
                img_core::failure::ErrorCode::Unknown,
            );
            println!(
                "{}",
                json!({"success":false,"error":failure.message(),"code":failure.code,"retry_after_seconds":failure.retry_after_seconds})
            );
            Ok(1)
        }
    }
}
fn execute(config_path: &std::path::Path, command: SyncCommand, control: &Control) -> Result<i32> {
    let root = img_records::data_dir()?;
    let _lock = sync_log::lock(&root)?;
    let mut catalog = Catalog::open(&root)?;
    let output = match command {
        SyncCommand::Configure { file, prefix } => {
            let text = std::fs::read_to_string(file)?;
            let provider = toml::from_str(&text)
                .map_err(|_| anyhow::anyhow!("invalid sync storage configuration"))?;
            serde_json::to_value(SyncSettings::configure(&root, &provider, &prefix)?)?
        }
        SyncCommand::Status => {
            let settings = SyncSettings::read(&root).ok();
            json!({"configured":settings.is_some(),"settings":settings,"last_result":catalog.setting("sync-last-result")?.map(|s|serde_json::from_str::<serde_json::Value>(&s)).transpose()?,"conflicts":catalog.sync_conflicts()?.len()})
        }
        SyncCommand::Pause | SyncCommand::Resume => {
            let mut settings = SyncSettings::read(&root)?;
            settings.paused = matches!(command, SyncCommand::Pause);
            settings.save(&root)?;
            json!({"paused":settings.paused})
        }
        SyncCommand::Conflicts => serde_json::to_value(catalog.sync_conflicts()?)?,
        SyncCommand::Resolve {
            entity,
            field,
            event,
        } => {
            let conflict = catalog
                .sync_conflicts()?
                .into_iter()
                .find(|c| c.entity == entity && c.field == field)
                .context("conflict no longer exists")?;
            let selected = conflict
                .candidates
                .into_iter()
                .find(|e| e.id == event)
                .context("selected event does not belong to this conflict")?;
            catalog.sync_set(&entity, &selected.field, selected.value)?;
            catalog.sync_materialize_catalog()?;
            json!({"resolved":true})
        }
        SyncCommand::Run => {
            let settings = SyncSettings::read(&root)?;
            ensure!(!settings.paused, "sync is paused");
            let remote = settings.remote()?;
            let secrets = img_core::sync_providers::Keychain;
            let expected = img_core::sync_providers::capture(&mut catalog, config_path, &secrets)?;
            catalog.sync_seed_catalog()?;
            let result = sync_log::exchange(
                &mut catalog,
                &remote,
                control,
                |events| img_core::sync_providers::export(events, &secrets),
                |values| img_core::sync_providers::import(values, &secrets),
            )?;
            catalog.sync_materialize_catalog()?;
            img_core::sync_providers::apply(&catalog, config_path, &expected, &secrets)?;
            let output = serde_json::to_value(result)?;
            catalog.set_setting("sync-last-result", &serde_json::to_string(&output)?)?;
            output
        }
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(0)
}
