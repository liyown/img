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
                json!({"success":false,"error":failure.message(),"code":failure.code,"detail_code":detail_code(&error),"retry_after_seconds":failure.retry_after_seconds})
            );
            Ok(1)
        }
    }
}
// Only export stable classifications, never server bodies or credential-bearing errors.
fn detail_code(error: &anyhow::Error) -> Option<&'static str> {
    let text = format!("{error:#}").to_lowercase();
    let transport = matches!(
        img_core::failure::Failure::from_error(error, img_core::failure::ErrorCode::Unknown).code,
        img_core::failure::ErrorCode::Network | img_core::failure::ErrorCode::Timeout
    );
    if transport && (text.contains("certificate") || text.contains("invalid peer")) {
        Some("tls_error")
    } else if text.contains("sync is already running") {
        Some("sync_busy")
    } else if text.contains("ignores conditional writes")
        || text.contains("changed a protected sync object")
    {
        Some("conditional_writes_unsupported")
    } else if text.contains("sync connection credentials are missing") {
        Some("credentials_missing")
    } else if text.contains("damaged sync batch") || text.contains("conflicting content") {
        Some("corrupt_remote_data")
    } else if error.downcast_ref::<url::ParseError>().is_some()
        || text.contains("invalid sync storage configuration")
    {
        Some("invalid_config")
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sync_details_classify_local_failures_without_exporting_error_text() {
        for (message, expected) in [
            ("sync is already running", "sync_busy"),
            (
                "service ignores conditional writes",
                "conditional_writes_unsupported",
            ),
            (
                "sync connection credentials are missing on this device",
                "credentials_missing",
            ),
            ("damaged sync batch", "corrupt_remote_data"),
            ("invalid sync storage configuration", "invalid_config"),
        ] {
            assert_eq!(detail_code(&anyhow::anyhow!(message)), Some(expected));
        }
        assert_eq!(
            detail_code(&anyhow::anyhow!("untrusted response with secret")),
            None
        );
        // A server body mentioning certificates is not itself a TLS failure.
        assert_eq!(detail_code(&anyhow::anyhow!("certificate secret")), None);
    }
}
