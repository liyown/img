use crate::args::{PresetCommand, TaskCommand};
use anyhow::{Context, Result, ensure};
use img_core::{
    config,
    control::Control,
    processing::{
        ProcessingPlan,
        batch::{self, ProcessingTask},
    },
};
use img_records::catalog::Catalog;
use std::path::Path;
pub fn run(config_path: &Path, command: TaskCommand, control: &Control) -> Result<i32> {
    match execute(config_path, command, control) {
        Ok(code) => Ok(code),
        Err(error) => {
            let config = config::read_global(config_path).unwrap_or_default();
            let message = config
                .providers
                .values()
                .fold(error.to_string(), |message, provider| {
                    provider.sanitize(&message)
                });
            println!(
                "{}",
                serde_json::json!({"success":false,"error":message,"error_code":"task_failed"})
            );
            Ok(1)
        }
    }
}
fn execute(config_path: &Path, command: TaskCommand, control: &Control) -> Result<i32> {
    let mut c = Catalog::open(&img_records::data_dir()?)?;
    match command {
        TaskCommand::Upload{id,provider,prefix}=>{
            let source=ProcessingTask::load(&c,id.trim_start_matches("process:"))?;
            let config=config::read_global(config_path)?;let provider=crate::provider(&config,&provider)?;
            let mut task=img_core::processing::publish::PublishTask::create(&source,&provider,&prefix)?;
            img_core::processing::publish::run(&mut c,&config,&mut task,control)?;
            println!("{}",serde_json::to_string(&task)?);return Ok(if task.files.iter().all(|file|file.success) && task.record_warning.is_none(){0}else{1});
        },
        TaskCommand::List{kind}=>println!("{}",serde_json::to_string(&c.tasks(&kind)?.into_iter().map(|(id,kind,body)|{let value=serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();serde_json::json!({"id":id,"kind":kind,"files":value["files"],"complete":value["complete"]})}).collect::<Vec<_>>())?),
        TaskCommand::Show{id}=>println!("{}",c.task(&id)?),
        TaskCommand::Retry{id}=>{
            let (kind,task_id)=id.split_once(':').context("use the full task ID from tasks list")?;
            match kind {
                "index"=>{
                    let scan:img_core::index::Scan=serde_json::from_str(&c.task(&id)?)?;let config=config::read_global(config_path)?;let provider=crate::provider(&config,&scan.scope.provider)?;let mut scope=scan.scope;scope.enabled=true;
                    let excluded=img_core::sync_settings::SyncSettings::read(&c.root).ok().filter(|settings|settings.namespace==scope.namespace).map(|settings|vec![settings.prefix]).unwrap_or_default();
                    let scan=img_core::index::run(&mut c,&provider,scope,true,&excluded,control)?;println!("{}",serde_json::to_string(&scan)?);return Ok(if scan.complete{0}else{1});
                },
                "publish"=>{let config=config::read_global(config_path)?;let mut task=img_core::processing::publish::PublishTask::load(&c,task_id)?;img_core::processing::publish::run(&mut c,&config,&mut task,control)?;println!("{}",serde_json::to_string(&task)?);return Ok(if task.files.iter().all(|file|file.success) && task.record_warning.is_none(){0}else{1});},
                "process"=>{let mut task=ProcessingTask::load(&c,task_id)?;batch::run(&c,&mut task,control)?;let result=task.summary();println!("{result}");return Ok(if result["complete"]==true{0}else{1});},
                "migrate"=>{let saved:img_core::migrate::Report=serde_json::from_str(&c.task(&id)?)?;let config=config::read_global(config_path)?;let report=img_core::migrate::apply(&mut c,&config,&saved.plan,control)?;let success=report.files.iter().all(|f|f.success);let mut result=serde_json::to_value(&report)?;result["mapping"]=serde_json::to_value(report.mapping())?;println!("{result}");return Ok(if success{0}else{1});},
                _=>anyhow::bail!("this task needs its original operation and confirmation"),
            }
        }
    }
    Ok(0)
}
pub fn presets(command: PresetCommand) -> Result<i32> {
    let mut c = Catalog::open(&img_records::data_dir()?)?;
    match command {
        PresetCommand::List => {
            let mut presets = vec![];
            for entity in c.sync_entities("preset:")? {
                let state = c.sync_entity(&entity)?;
                if state.deleted {
                    continue;
                }
                presets.push(serde_json::json!({"id":entity.trim_start_matches("preset:"),"name":state.fields.get("name"),"plan":state.fields.get("plan"),"conflict":!state.conflicts.is_empty()}));
            }
            println!("{}", serde_json::to_string(&presets)?);
        }
        PresetCommand::Save { name, recipe, id } => {
            ensure!(
                !name.trim().is_empty() && name.chars().count() <= 64,
                "preset name must contain 1–64 characters"
            );
            let plan: ProcessingPlan = serde_json::from_slice(&std::fs::read(recipe)?)?;
            plan.validate()?;
            let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            ensure!(uuid::Uuid::parse_str(&id).is_ok(), "invalid preset ID");
            c.sync_set_fields(
                &format!("preset:{id}"),
                &[
                    ("name", name.trim().into()),
                    ("plan", serde_json::to_value(plan)?),
                ],
            )?;
            println!("{}", serde_json::json!({"id":id,"saved":true}));
        }
        PresetCommand::Remove { id } => {
            ensure!(uuid::Uuid::parse_str(&id).is_ok(), "invalid preset ID");
            c.sync_set(&format!("preset:{id}"), "$deleted", Some(true.into()))?;
            println!("{}", serde_json::json!({"removed":true}));
        }
    }
    Ok(0)
}
