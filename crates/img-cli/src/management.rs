use crate::args::{ConfigCommand, Init, ProviderCommand};
use anyhow::{Context, Result, bail, ensure};
use img_core::{
    config::{self, Config, ProviderConfig},
    output,
};
use std::{io::Write, path::Path};
fn prompt(text: &str) -> Result<String> {
    print!("{text}: ");
    std::io::stdout().flush()?;
    let mut s = String::new();
    ensure!(
        std::io::stdin().read_line(&mut s)? > 0,
        "interactive initialization needs input; use --type and provider flags for scripts"
    );
    Ok(s.trim().into())
}
pub fn init(path: &Path, mut v: Init) -> Result<()> {
    if v.kind.is_empty() {
        v.kind = prompt("Provider type (http/s3/github)")?;
        v.name = prompt("Provider name")?;
        match v.kind.as_str() {
            "http" => v.url = prompt("Upload URL")?,
            "s3" => {
                v.bucket = prompt("Bucket")?;
                v.public_url = prompt("Public URL")?;
                v.endpoint = prompt("Endpoint (empty for AWS)")?;
            }
            "github" => {
                v.owner = prompt("Owner")?;
                v.repo = prompt("Repository")?;
                v.token = prompt("Token environment reference, e.g. ${IMG_GITHUB_TOKEN}")?;
            }
            _ => bail!("unsupported provider type"),
        }
    }
    if v.name.is_empty() {
        v.name = v.kind.clone();
    }
    ensure!(
        !v.name.chars().any(char::is_control),
        "invalid provider name"
    );
    let pc = ProviderConfig {
        kind: v.kind,
        url: v.url,
        url_json_path: v.url_json_path,
        method: v.method,
        file_field: v.file_field,
        endpoint: v.endpoint,
        region: v.region,
        bucket: v.bucket,
        access_key: v.access_key,
        secret_key: v.secret_key,
        session_token: v.session_token,
        public_url: v.public_url,
        path_style: v.path_style,
        allow_insecure: v.allow_insecure,
        owner: v.owner,
        repo: v.repo,
        branch: v.branch,
        token: v.token,
        commit_message: v.commit_message,
        ..Default::default()
    };
    let mut cfg = config::read_global(path)?;
    cfg.providers.insert(v.name.clone(), pc);
    cfg.default_provider = v.name.clone();
    config::save(path, &cfg)?;
    println!(
        "Provider {:?} configured.\nDefault provider: {}",
        v.name, v.name
    );
    Ok(())
}
pub fn provider(path: &Path, command: ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::List => {
            let c = super::load(path)?;
            println!("NAME\tTYPE\tDEFAULT\tSTATUS");
            for (name, p) in c.providers {
                println!(
                    "{}\t{}\t{}\tconfigured",
                    output::clean(&name),
                    p.kind,
                    if c.default_provider == name {
                        "yes"
                    } else {
                        "no"
                    }
                );
            }
        }
        ProviderCommand::Show { name } => {
            let c = super::load(path)?.redacted();
            let p = c.providers.get(&name).context("provider not found")?;
            println!("{}", toml::to_string_pretty(p)?);
        }
        ProviderCommand::Test { name } => {
            let c = super::load(path)?;
            super::provider(&c, &name)?.test()?;
            println!("available");
        }
        ProviderCommand::Use { name } => {
            let mut c = config::read_global(path)?;
            ensure!(c.providers.contains_key(&name), "provider not found");
            c.default_provider = name.clone();
            config::save(path, &c)?;
            println!("Default provider: {}", output::clean(&name));
        }
        ProviderCommand::Remove { name } => {
            let mut c = config::read_global(path)?;
            ensure!(c.providers.remove(&name).is_some(), "provider not found");
            if c.default_provider == name {
                c.default_provider = String::new();
            }
            if c.provider == name {
                c.provider = String::new();
            }
            config::save(path, &c)?;
        }
    }
    Ok(())
}
const KEYS: &[&str] = &[
    "default_provider",
    "output.format",
    "output.copy",
    "output.quiet",
    "upload.path",
    "upload.path_template",
    "upload.reuse",
    "upload.rename",
    "upload.conflict",
    "upload.overwrite",
    "upload.concurrency",
    "upload.max_size",
    "upload.strip_exif",
    "upload.max_width",
    "upload.retry_count",
];
fn get_value<'a>(doc: &'a toml::Value, key: &str) -> Result<&'a toml::Value> {
    ensure!(KEYS.contains(&key), "unsupported config key");
    let mut value = doc;
    for k in key.split('.') {
        value = value.get(k).context("configuration key not found")?;
    }
    Ok(value)
}
pub fn config(path: &Path, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Path => println!("{}", path.display()),
        ConfigCommand::List => println!(
            "{}",
            toml::to_string_pretty(&super::load(path)?.redacted())?
        ),
        ConfigCommand::Validate => {
            super::load(path)?;
            println!("configuration is valid");
        }
        ConfigCommand::Get { key } => {
            let doc = toml::Value::try_from(super::load(path)?.redacted())?;
            let value = get_value(&doc, &key)?;
            if let Some(s) = value.as_str() {
                println!("{}", output::clean(s));
            } else {
                println!("{value}");
            }
        }
        ConfigCommand::Set { key, value } => change(path, &key, Some(&value))?,
        ConfigCommand::Unset { key } => change(path, &key, None)?,
    }
    Ok(())
}
fn change(path: &Path, key: &str, value: Option<&str>) -> Result<()> {
    let mut doc = toml::Value::try_from(config::read_global(path)?)?;
    let defaults = toml::Value::try_from(Config::default())?;
    let default = get_value(&defaults, key)?;
    let new = if let Some(value) = value {
        match default {
            toml::Value::String(_) => toml::Value::String(value.into()),
            toml::Value::Boolean(_) => toml::Value::Boolean(config::parse_bool(value)?),
            toml::Value::Integer(_) => {
                toml::Value::Integer(value.parse().context("expected an integer")?)
            }
            _ => bail!("unsupported config value"),
        }
    } else {
        default.clone()
    };
    let mut dest = &mut doc;
    let parts = key.split('.').collect::<Vec<_>>();
    for k in &parts[..parts.len() - 1] {
        dest = dest.get_mut(*k).context("invalid config key")?;
    }
    dest.as_table_mut()
        .context("invalid config section")?
        .insert(parts.last().unwrap().to_string(), new);
    let c: Config = doc.try_into()?;
    config::save(path, &c)
}
