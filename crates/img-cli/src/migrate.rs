use crate::args::MigrateCommand;
use anyhow::{Result, ensure};
use img_core::{config, control::Control, migrate};
use img_records::{catalog::Catalog, targets::TargetPlan};
use std::{io::Write, path::Path};
pub fn run(config_path: &Path, command: MigrateCommand, control: &Control) -> Result<i32> {
    let config = config::read_global(config_path)?;
    let operation = (|| -> Result<i32> {
        let mut catalog = Catalog::open(&img_records::data_dir()?)?;
        catalog.import_legacy()?;
        match command {
            MigrateCommand::Source {
                task_id,
                input_id,
                file,
            } => {
                let report = migrate::set_local_source(
                    &catalog,
                    &task_id,
                    &input_id,
                    &file,
                    config.upload.max_size,
                )?;
                println!("{}", serde_json::to_string(&report)?);
                Ok(0)
            }
            MigrateCommand::Plan {
                ids,
                selection,
                from,
                to,
                prefix,
                output,
            } => {
                ensure!(!output.exists(), "plan output already exists");
                let selected = if let Some(path) = selection {
                    serde_json::from_slice::<TargetPlan>(&std::fs::read(path)?)?
                } else {
                    TargetPlan::capture(&catalog, &ids, &from)?
                };
                let provider = crate::provider(&config, &to)?;
                let plan = migrate::plan(selected, &provider, &prefix, control)?;
                let mut file = std::fs::File::options()
                    .create_new(true)
                    .write(true)
                    .open(output)?;
                file.write_all(&serde_json::to_vec_pretty(&plan)?)?;
                file.sync_all()?;
                println!("{}", serde_json::to_string(&plan)?);
                Ok(0)
            }
            MigrateCommand::Apply { plan, report } => {
                if let Some(path) = &report {
                    ensure!(!path.exists(), "report output already exists");
                }
                let plan = serde_json::from_slice(&std::fs::read(plan)?)?;
                let result = migrate::apply(&mut catalog, &config, &plan, control)?;
                let success = result.files.iter().all(|file| file.success);
                let mut value = serde_json::to_value(&result)?;
                value["mapping"] = serde_json::to_value(result.mapping())?;
                if let Some(path) = report {
                    let mut file = std::fs::File::options()
                        .create_new(true)
                        .write(true)
                        .open(path)?;
                    file.write_all(&serde_json::to_vec_pretty(&value)?)?;
                    file.sync_all()?;
                }
                println!("{}", value);
                Ok(if success { 0 } else { 1 })
            }
            MigrateCommand::Show { task_id } => {
                ensure!(uuid::Uuid::parse_str(&task_id).is_ok(), "invalid task ID");
                let result: migrate::Report =
                    serde_json::from_str(&catalog.task(&format!("migrate:{task_id}"))?)?;
                let mut value = serde_json::to_value(&result)?;
                value["mapping"] = serde_json::to_value(result.mapping())?;
                println!("{}", value);
                Ok(0)
            }
        }
    })();
    match operation {
        Ok(code) => Ok(code),
        Err(error) => {
            let message = config
                .providers
                .values()
                .fold(error.to_string(), |message, provider| {
                    provider.sanitize(&message)
                });
            println!(
                "{}",
                serde_json::json!({"success":false,"error":message,"error_code":"migration_failed"})
            );
            Ok(1)
        }
    }
}
