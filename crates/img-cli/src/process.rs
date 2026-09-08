use crate::args::Process;
use anyhow::{Result, ensure};
use img_core::{
    config,
    control::Control,
    media,
    processing::{
        ProcessingPlan,
        batch::{self, Input, ProcessingTask},
    },
};
use img_records::catalog::Catalog;
use std::{io::Write, path::Path};
pub fn run(config_path: &Path, args: Process, control: &Control) -> Result<i32> {
    let structured =
        args.recipe.is_some() || args.resume.is_some() || args.inputs_manifest.is_some();
    match execute(config_path, args, control) {
        Ok(code) => Ok(code),
        Err(error) if structured => {
            println!(
                "{}",
                serde_json::json!({"success":false,"error":error.to_string(),"error_code":"processing_failed"})
            );
            Ok(1)
        }
        Err(error) => Err(error),
    }
}
fn execute(config_path: &Path, args: Process, control: &Control) -> Result<i32> {
    if let Some(id) = &args.resume {
        let c = Catalog::open(&img_records::data_dir()?)?;
        let mut task = ProcessingTask::load(&c, id)?;
        batch::run(&c, &mut task, control)?;
        let result = task.summary();
        println!("{result}");
        return Ok(if result["complete"] == true { 0 } else { 1 });
    }
    if args.recipe.is_none() {
        ensure!(
            args.files.len() == 1 && args.output_dir.is_none() && args.inputs_manifest.is_none(),
            "batch processing requires --recipe and --output-dir"
        );
        let output = args
            .output
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--output is required"))?;
        ensure!(!output.exists(), "output already exists; choose a new file");
        let cfg = config::read_global(config_path)?;
        let options = args.processing.options();
        let bytes = media::read_image(&args.files[0], cfg.upload.max_size)?;
        let ct = media::detect(&bytes)?;
        let original_size = bytes.len();
        let result = media::process_recipe(
            bytes,
            ct,
            options.strip_exif || cfg.upload.strip_exif,
            if options.max_width > 0 {
                options.max_width
            } else {
                cfg.upload.max_width
            },
            options.optimize,
            options.recipe.as_ref().unwrap_or(&cfg.upload.recipe),
        )?;
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut saved = tempfile::NamedTempFile::new_in(parent)?;
        saved.write_all(&result.data)?;
        saved.as_file().sync_all()?;
        saved.persist_noclobber(output).map_err(|e| e.error)?;
        println!(
            "{}",
            serde_json::json!({"original_size":original_size,"size":result.data.len(),"content_type":result.content_type,"output":output,"info":media::info(output)})
        );
        return Ok(0);
    }
    let plan: ProcessingPlan = serde_json::from_slice(&std::fs::read(args.recipe.unwrap())?)?;
    let legacy = &args.processing;
    ensure!(
        legacy.preset.is_none()
            && legacy.image_format.is_none()
            && legacy.quality.is_none()
            && legacy.max_edge.is_none()
            && legacy.watermark_opacity.is_none()
            && !legacy.optimize
            && !legacy.strip_exif
            && legacy.resize == 0,
        "with --recipe, set format, compression, geometry and watermark opacity in the recipe; --watermark supplies only the local image resource"
    );
    plan.validate()?;
    let limit = 256 << 20;
    let inputs = if let Some(manifest) = args.inputs_manifest {
        serde_json::from_slice::<Vec<Input>>(&std::fs::read(manifest)?)?
    } else {
        img_records::files::collect(&args.files, true, 10000)?
            .iter()
            .map(|path| Input::snapshot(path, limit))
            .collect::<Result<Vec<_>>>()?
    };
    let watermark = args
        .processing
        .watermark
        .as_ref()
        .map(|path| Input::snapshot(path, 20 << 20))
        .transpose()?;
    if let Some(output) = args.output {
        ensure!(
            inputs.len() == 1 && plan.stitch.is_none() && plan.split.is_none(),
            "multiple outputs require --output-dir"
        );
        ensure!(!output.exists(), "output already exists; choose a new file");
        let original = media::read_image(&inputs[0].path, limit)?;
        ensure!(
            img_records::catalog::digest(&original) == inputs[0].content_hash,
            "input changed; choose it again"
        );
        let mark = watermark
            .map(|input| media::read_image(&input.path, 20 << 20))
            .transpose()?;
        let mut images = img_core::processing::process(&original, &plan, mark.as_deref(), control)?;
        let image = images.remove(0);
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&image.data)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(&output).map_err(|e| e.error)?;
        let mut result = serde_json::to_value(&image)?;
        result["output"] = serde_json::to_value(&output)?;
        result["info"] = serde_json::to_value(media::info(&output))?;
        println!("{result}");
        return Ok(0);
    }
    let mut task = ProcessingTask::create(
        inputs,
        plan,
        watermark,
        &args
            .output_dir
            .ok_or_else(|| anyhow::anyhow!("--output-dir is required"))?,
        limit,
    )?;
    if args.prepare {
        let catalog = Catalog::open(&img_records::data_dir()?)?;
        task.save(&catalog)?;
        println!(
            "{}",
            serde_json::json!({"task_id":task.task_id,"prepared":true})
        );
        return Ok(0);
    }
    if args.preview {
        batch::preview(&mut task, control)?;
    } else {
        let c = Catalog::open(&img_records::data_dir()?)?;
        batch::run(&c, &mut task, control)?;
    }
    let mut result = task.summary();
    if args.preview && result["complete"] == true {
        result["preview"] = batch::preview_artifact(&task)?;
    }
    println!("{result}");
    Ok(if result["complete"] == true { 0 } else { 1 })
}
