use crate::args::ReferenceCommand;
use anyhow::{Context, Result, ensure};
use img_core::{control::Control, references};
use img_records::catalog::Catalog;
use std::{io::Write, path::Path};
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn run(command: ReferenceCommand, control: &Control) -> Result<i32> {
    let result = (|| -> Result<i32> {
        let catalog = Catalog::open(&img_records::data_dir()?)?;
        match command {
            ReferenceCommand::Scan {
                directory,
                mapping,
                migration,
                output,
            } => {
                if let Some(path) = &output {
                    ensure!(!path.exists(), "plan output already exists");
                }
                let mappings = if let Some(path) = mapping {
                    ensure!(
                        path.metadata()?.len() <= 16 << 20,
                        "mapping file exceeds 16 MiB"
                    );
                    let raw: Vec<serde_json::Value> =
                        serde_json::from_slice(&std::fs::read(path)?)?;
                    let pairs = raw
                        .iter()
                        .map(|item| {
                            Ok((
                                item["old"]
                                    .as_str()
                                    .context("mapping old URL missing")?
                                    .to_owned(),
                                item["new"]
                                    .as_str()
                                    .context("mapping new URL missing")?
                                    .to_owned(),
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    references::verify_mappings(&pairs, control)?
                } else if let Some(id) = migration {
                    ensure!(
                        uuid::Uuid::parse_str(&id).is_ok(),
                        "invalid migration task ID"
                    );
                    references::migration_mappings(&serde_json::from_str(
                        &catalog.task(&format!("migrate:{id}"))?,
                    )?)?
                } else {
                    vec![]
                };
                let plan = references::scan(&directory, mappings, control)?;
                references::save_plan(&catalog, &plan)?;
                if let Some(path) = output {
                    write_new(&path, &serde_json::to_vec_pretty(&plan)?)?;
                }
                println!("{}", serde_json::to_string(&plan)?);
            }
            ReferenceCommand::Show { task_id } => println!(
                "{}",
                serde_json::to_string(&references::load(&catalog, &task_id)?)?
            ),
            ReferenceCommand::Apply { task_id, yes } => {
                ensure!(
                    yes,
                    "review references show first, then pass --yes to write documents"
                );
                let report = references::load(&catalog, &task_id)?;
                let result = references::apply(&catalog, &report.plan, control)?;
                println!("{}", serde_json::to_string(&result)?);
                return Ok(if result.complete && result.record_warning.is_none() {
                    0
                } else {
                    1
                });
            }
            ReferenceCommand::Restore { task_id } => {
                let report = references::load(&catalog, &task_id)?;
                let plan = references::restore_plan(&report, control)?;
                references::save_plan(&catalog, &plan)?;
                println!("{}", serde_json::to_string(&plan)?);
            }
            ReferenceCommand::Export { task_id, directory } => {
                let report = references::load(&catalog, &task_id)?;
                ensure!(
                    !directory.exists(),
                    "export directory already exists; choose a new directory"
                );
                std::fs::create_dir(&directory)?;
                let mut backups = vec![];
                for file in &report.files {
                    control.check()?;
                    if let Some(backup) = &file.backup {
                        let original = report
                            .plan
                            .files
                            .iter()
                            .find(|document| document.relative == file.relative)
                            .context("backup document missing")?;
                        let metadata = backup.symlink_metadata()?;
                        ensure!(
                            metadata.is_file()
                                && !metadata.file_type().is_symlink()
                                && metadata.len() <= 16 << 20,
                            "invalid backup file"
                        );
                        let bytes = std::fs::read(backup)?;
                        ensure!(
                            img_records::catalog::digest(&bytes) == original.original_hash,
                            "backup has changed"
                        );
                        let name = format!("{:04}.md.backup", backups.len() + 1);
                        write_new(&directory.join(&name), &bytes)?;
                        backups.push(serde_json::json!({"document":file.relative,"backup":name,"content_hash":original.original_hash}));
                    }
                }
                write_new(
                    &directory.join("report.json"),
                    &serde_json::to_vec_pretty(&report)?,
                )?;
                write_new(
                    &directory.join("backups.json"),
                    &serde_json::to_vec_pretty(&backups)?,
                )?;
                println!(
                    "{}",
                    serde_json::json!({"exported":true,"directory":directory,"backups":backups.len()})
                );
            }
        }
        Ok(0)
    })();
    match result {
        Ok(code) => Ok(code),
        Err(error) => {
            println!(
                "{}",
                serde_json::json!({"success":false,"error":error.to_string(),"error_code":"reference_operation_failed"})
            );
            Ok(1)
        }
    }
}
