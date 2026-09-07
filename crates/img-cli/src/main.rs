mod args;
use args::RemoteCommand;
mod management;
mod markdown;
mod platform;
mod serve;
mod watch;
use anyhow::{Result, ensure};
use args::{Cli, Command};
use clap::{CommandFactory, Parser};
use img_core::{
    config::{self, Config},
    control::Control,
    media, network, output,
    provider::Provider,
    upload,
};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
};

fn main() {
    let args = normalized_args(std::env::args_os().collect());
    let cli = Cli::parse_from(args);
    let control = Control::default();
    let stopped = control.clone();
    if let Err(e) = ctrlc::set_handler(move || {
        stopped.cancel();
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            std::process::exit(130);
        });
    }) {
        eprintln!("Error: cannot install interruption handler: {e}");
        std::process::exit(2);
    }
    let code = match run(cli, &control) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {}", output::clean(&format!("{e:#}")));
            2
        }
    };
    std::process::exit(if control.is_cancelled() { 130 } else { code });
}
fn normalized_args(mut args: Vec<OsString>) -> Vec<OsString> {
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" {
            i += 2;
            continue;
        }
        if args[i].to_string_lossy().starts_with("--config=") {
            i += 1;
            continue;
        }
        let command = args[i].to_string_lossy();
        if ![
            "backup",
            "restore",
            "remote",
            "upload",
            "check",
            "import-config",
            "watch",
            "restore-document",
            "process",
            "fetch",
            "screenshot",
            "serve",
            "rewrite",
            "info",
            "init",
            "provider",
            "config",
            "completion",
            "version",
            "install-cli",
            "help",
            "--help",
            "-h",
            "--version",
            "-V",
        ]
        .contains(&command.as_ref())
        {
            args.insert(i, "upload".into());
        }
        break;
    }
    args
}
fn load(path: &Path) -> Result<Config> {
    config::load(path, Some(&std::env::current_dir()?.join(".img.toml")))
}
fn provider(cfg: &Config, name: &str) -> Result<Provider> {
    let (name, p) = cfg.selected(name)?;
    Provider::new(name, p)
}
fn report(
    cfg: &Config,
    format: &str,
    results: &[upload::FileResult],
    copy: bool,
    quiet: bool,
) -> Result<i32> {
    for result in results {
        if !result.record_warning.is_empty() {
            eprintln!("Warning: {}", result.record_warning);
        }
    }
    if !quiet && !cfg.output.quiet {
        println!("{}", output::render(format, results, false)?);
    }
    if copy
        && results.iter().any(|r| r.success)
        && let Err(e) = platform::clipboard(&output::render(format, results, true)?)
    {
        eprintln!("Warning: upload succeeded, but clipboard copy failed: {e}");
    }
    Ok(upload::exit_code(results))
}
fn run(cli: Cli, control: &Control) -> Result<i32> {
    let path = cli.config.unwrap_or(config::global_path()?);
    match cli.command {
        Command::Remote {
            provider: name,
            command,
        } => {
            let cfg = load(&path)?;
            let provider = provider(&cfg, &name)?;
            match command {
                RemoteCommand::List { prefix, cursor } => println!(
                    "{}",
                    serde_json::to_string(&provider.list_remote(&prefix, cursor.as_deref())?)?
                ),
                RemoteCommand::Delete { path, version, yes } => {
                    ensure!(
                        yes,
                        "remote deletion requires --yes and the version from remote list"
                    );
                    provider.delete_remote(&path, &version)?;
                    println!("{}", serde_json::json!({"deleted":path}));
                }
            }
        }
        Command::Backup {
            destination,
            without_config,
            without_records,
            include_cache,
            include_credentials,
        } => {
            let manifest = img_records::backup::export(
                &img_records::data_dir()?,
                &path,
                &destination,
                img_records::backup::Options {
                    config: !without_config,
                    records: !without_records,
                    cache: include_cache,
                    credentials: include_credentials,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
        Command::Restore {
            source,
            apply,
            include_credentials,
        } => {
            let manifest = img_records::backup::inspect(&source)?;
            if apply {
                let recovery = img_records::backup::restore(
                    &source,
                    &img_records::data_dir()?,
                    &path,
                    include_credentials,
                )?;
                println!(
                    "{}",
                    serde_json::json!({"restored":true,"recovery":recovery})
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            }
        }
        Command::Process {
            file,
            output,
            processing,
        } => {
            ensure!(!output.exists(), "output already exists; choose a new file");
            let cfg = config::read_global(&path)?;
            let options = processing.options();
            let bytes = media::read_image(&file, cfg.upload.max_size)?;
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
            saved.persist_noclobber(&output).map_err(|e| e.error)?;
            let info = media::info(&output);
            println!(
                "{}",
                serde_json::json!({"original_size":original_size,"size":result.data.len(),"content_type":result.content_type,"output":output,"info":info})
            );
        }
        Command::RestoreDocument { backup, target } => {
            ensure!(backup != target, "backup and target must differ");
            let original = std::fs::read(&target)?;
            let saved =
                markdown::replace_with_backup(&target, &original, &std::fs::read(&backup)?)?;
            println!(
                "Restored document; previous contents saved to {}",
                saved.display()
            );
        }
        Command::Watch {
            directory,
            processing,
            interval,
            new_only,
        } => {
            let cfg = load(&path)?;
            let provider = provider(&cfg, &processing.provider)?;
            return watch::run(
                &directory,
                &provider,
                &cfg.upload,
                &processing.options(),
                interval,
                new_only,
                control,
            );
        }
        Command::ImportConfig { file, apply } => management::import_config(&path, &file, apply)?,
        Command::Check {
            urls,
            allow_insecure,
        } => {
            let results: Vec<_> = urls
                .iter()
                .map(|url| img_core::link_check::check(url, allow_insecure))
                .collect();
            println!("{}", serde_json::to_string(&results)?);
            return Ok(if results.iter().all(|r| r.accessible) {
                0
            } else {
                1
            });
        }
        Command::Version => {
            println!(
                "img {}\nimplementation: Rust\ncommit: {}\nbuilt: {}",
                env!("CARGO_PKG_VERSION"),
                option_env!("IMG_BUILD_COMMIT").unwrap_or("unknown"),
                option_env!("IMG_BUILD_DATE").unwrap_or("local")
            );
        }
        Command::Completion { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "img", &mut std::io::stdout());
        }
        Command::InstallCli { dir } => {
            let (path, on_path) = platform::install_cli(dir.as_deref())?;
            println!("Installed CLI: {}", path.display());
            if !on_path {
                println!(
                    "Add {} to PATH to use img from any terminal.",
                    path.parent().unwrap().display()
                );
            }
        }
        Command::Init(v) => management::init(&path, *v)?,
        Command::Config { command } => management::config(&path, command)?,
        Command::Provider { command } => management::provider(&path, command)?,
        Command::Upload(v) => {
            let mut v = v;
            if v.recursive {
                let mut files = vec![];
                for input in &v.files {
                    if network::is_url(input) {
                        files.push(input.clone());
                    } else {
                        files.extend(
                            img_records::files::collect(&[input.into()], true, 10_000)?
                                .into_iter()
                                .map(|p| p.to_string_lossy().into_owned()),
                        );
                    }
                }
                ensure!(files.len() <= 10_000, "image batch exceeds 10000 files");
                v.files = files;
            }
            let setup = (|| {
                ensure!(
                    v.name.is_empty() || v.files.len() == 1,
                    "--name requires exactly one file"
                );
                ensure!(
                    !v.progress || v.files.len() == 1,
                    "--progress requires exactly one file"
                );
                let cfg = load(&path)?;
                let p = provider(&cfg, &v.processing.provider)?;
                Ok::<_, anyhow::Error>((cfg, p))
            })();
            let (cfg, p) = match setup {
                Ok(value) => value,
                Err(error) if v.format.as_deref() == Some("json") => {
                    let failure = img_core::failure::Failure::from_error(
                        &error,
                        img_core::failure::ErrorCode::InvalidConfig,
                    );
                    let results: Vec<_> = v
                        .files
                        .iter()
                        .map(|source| upload::FileResult::failure(source, failure.clone()))
                        .collect();
                    println!("{}", output::render("json", &results, false)?);
                    return Ok(2);
                }
                Err(error) => return Err(error),
            };
            let mut opts = v.processing.options();
            opts.name = v.name;
            let control = if v.progress {
                control.clone().with_reporter(|p| {
                    let mut stderr = std::io::stderr().lock();
                    let _ = serde_json::to_writer(&mut stderr, &p);
                    let _ = writeln!(stderr);
                })
            } else {
                control.clone()
            };
            if v.verbose {
                eprintln!("Using provider {}", output::clean(&p.name));
            }
            let results = upload::run(&p, &cfg.upload, &v.files, &opts, &control);
            if v.verbose {
                for r in &results {
                    if r.success && r.original_size > 0 {
                        eprintln!(
                            "Processed {}: {} → {} bytes",
                            output::clean(&r.local_path),
                            r.original_size,
                            r.size
                        );
                    }
                }
            }
            return report(
                &cfg,
                v.format.as_deref().unwrap_or(&cfg.output.format),
                &results,
                !v.no_copy && (v.copy || cfg.output.copy),
                v.quiet,
            );
        }
        Command::Fetch(v) => {
            let image = network::fetch(&v.url, v.max_size, v.allow_insecure)?;
            let typ = media::inspect(&image.data, v.max_size)?;
            control.check()?;
            // The original must never replace a file or follow a destination symlink.
            let parent = v
                .output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
            tmp.write_all(&image.data)?;
            tmp.as_file().sync_all()?;
            tmp.persist_noclobber(&v.output).map_err(|e| e.error)?;
            println!(
                "{}",
                serde_json::json!({"name":image.name,"size":image.data.len(),"content_type":typ})
            );
        }
        Command::Info(v) => {
            let info = v.files.iter().map(|p| media::info(p)).collect::<Vec<_>>();
            if v.format == "json" {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                for i in info {
                    if !i.error.is_empty() {
                        println!(
                            "{}  error: {}",
                            output::clean(&i.path),
                            output::clean(&i.error)
                        );
                    } else {
                        println!(
                            "{}  {}  {}×{}  {} bytes{}",
                            output::clean(&i.path),
                            i.content_type,
                            i.width,
                            i.height,
                            i.size,
                            if i.has_exif { "  EXIF" } else { "" }
                        );
                    }
                }
            }
        }
        Command::Screenshot(v) => {
            if let Some(output) = &v.output {
                let image = platform::screenshot(v.region, v.window)?;
                std::fs::copy(image.path(), output)?;
                println!("{}", serde_json::json!({"path": output}));
                return Ok(0);
            }
            let cfg = load(&path)?;
            let p = provider(&cfg, &v.processing.provider)?;
            let image = platform::screenshot(v.region, v.window)?;
            let results = upload::run(
                &p,
                &cfg.upload,
                &[image.path().to_string_lossy().into_owned()],
                &v.processing.options_for("screenshot"),
                control,
            );
            return report(
                &cfg,
                v.format.as_deref().unwrap_or(&cfg.output.format),
                &results,
                !v.no_copy,
                false,
            );
        }
        Command::Rewrite(v) => {
            if v.dry_run {
                let mut reports = vec![];
                if v.files.is_empty() {
                    let mut doc = String::new();
                    std::io::stdin().read_to_string(&mut doc)?;
                    reports.push(serde_json::json!({"document":"stdin", "references":markdown::preview(&doc,&std::env::current_dir()?)}));
                } else {
                    for file in &v.files {
                        let path = file.canonicalize()?;
                        reports.push(serde_json::json!({"document":file,"references":markdown::preview(&std::fs::read_to_string(&path)?,path.parent().unwrap())}));
                    }
                }
                println!("{}", serde_json::to_string_pretty(&reports)?);
                return Ok(0);
            }
            if let Some(report) = &v.report {
                ensure!(
                    !report.exists(),
                    "report destination already exists; choose a new file"
                );
            }
            let cfg = load(&path)?;
            let p = provider(&cfg, &v.processing.provider)?;
            let opts = v.processing.options_for("rewrite");
            let mut total_ok = 0;
            let mut total_failed = 0;
            let mut results = vec![];
            if v.files.is_empty() {
                let mut doc = String::new();
                std::io::stdin().read_to_string(&mut doc)?;
                let (out, ok, failed, rows) = markdown::rewrite(
                    &doc,
                    &std::env::current_dir()?,
                    &p,
                    &cfg.upload,
                    &opts,
                    control,
                )?;
                print!("{out}");
                results.extend(rows);
                total_ok += ok;
                total_failed += failed;
            } else {
                for file in &v.files {
                    control.check()?;
                    let result = (|| -> Result<()> {
                        let doc = std::fs::read_to_string(file)?;
                        let dir = file.canonicalize()?.parent().unwrap().to_path_buf();
                        let (out, ok, failed, rows) =
                            markdown::rewrite(&doc, &dir, &p, &cfg.upload, &opts, control)?;
                        results.extend(rows);
                        if v.stdout {
                            print!("{out}");
                        } else if out != doc {
                            let backup = markdown::replace_with_backup(
                                file,
                                doc.as_bytes(),
                                out.as_bytes(),
                            )?;
                            eprintln!("Backup: {}", output::clean(&backup.to_string_lossy()));
                        }
                        total_ok += ok;
                        total_failed += failed;
                        eprintln!(
                            "Rewrite {}: {ok} uploaded, {failed} failed",
                            output::clean(&file.to_string_lossy())
                        );
                        Ok(())
                    })();
                    if let Err(e) = result {
                        eprintln!(
                            "Error: {}: {}",
                            output::clean(&file.to_string_lossy()),
                            output::clean(&e.to_string())
                        );
                        total_failed += 1;
                    }
                }
            }
            if let Some(report) = &v.report {
                let parent = report
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                let mut output = tempfile::NamedTempFile::new_in(parent)?;
                output.write_all(&serde_json::to_vec_pretty(&results)?)?;
                output.as_file().sync_all()?;
                output.persist_noclobber(report).map_err(|e| e.error)?;
            }
            return Ok(if total_failed == 0 {
                0
            } else if total_ok > 0 {
                3
            } else {
                1
            });
        }
        Command::Serve(v) => {
            let cfg = load(&path)?;
            let p = provider(&cfg, &v.processing.provider)?;
            serve::run(
                p,
                cfg.upload,
                v.processing.options_for("editor"),
                &v.bind,
                v.port,
                control.clone(),
            )?;
        }
    }
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_upload_keeps_flags_after_files_and_config() {
        let cli = Cli::try_parse_from(normalized_args(
            ["img", "--config", "test.toml", "a.png", "--format", "json"]
                .into_iter()
                .map(OsString::from)
                .collect(),
        ))
        .unwrap();
        assert_eq!(cli.config.unwrap(), Path::new("test.toml"));
        match cli.command {
            Command::Upload(v) => {
                assert_eq!(v.files, ["a.png"]);
                assert_eq!(v.format.as_deref(), Some("json"));
            }
            _ => panic!(),
        };
    }
}
