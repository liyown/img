use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::LazyLock,
};

pub static REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap());

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub default_provider: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub provider: String,
    pub output: Output,
    pub upload: Upload,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub allow_plaintext_credentials: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            default_provider: String::new(),
            provider: String::new(),
            output: Output::default(),
            upload: Upload::default(),
            providers: BTreeMap::new(),
            allow_plaintext_credentials: false,
            extra: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    pub format: String,
    pub copy: bool,
    pub quiet: bool,
}
impl Default for Output {
    fn default() -> Self {
        Self {
            format: "url".into(),
            copy: false,
            quiet: false,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Upload {
    pub path: String,
    pub path_template: String,
    pub rename: String,
    pub conflict: String,
    pub overwrite: bool,
    pub concurrency: usize,
    pub max_size: u64,
    pub strip_exif: bool,
    pub max_width: u32,
    pub retry_count: u32,
}
impl Default for Upload {
    fn default() -> Self {
        Self {
            path: String::new(),
            path_template: "{year}/{month}/{filename}".into(),
            rename: "original".into(),
            conflict: "error".into(),
            overwrite: false,
            concurrency: 4,
            max_size: 20 << 20,
            strip_exif: false,
            max_width: 0,
            retry_count: 0,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub kind: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub session_token: String,
    pub public_url: String,
    pub path_style: bool,
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub token: String,
    pub commit_message: String,
    pub url: String,
    pub method: String,
    pub file_field: String,
    pub url_json_path: String,
    pub headers: BTreeMap<String, String>,
    pub fields: BTreeMap<String, String>,
    pub allow_insecure: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    version: Option<u32>,
    provider: Option<String>,
    output: Option<ProjectOutput>,
    upload: Option<ProjectUpload>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectOutput {
    format: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectUpload {
    path: Option<String>,
    path_template: Option<String>,
}

pub fn global_path() -> Result<PathBuf> {
    // Match Go's os.UserConfigDir, including macOS Library/Application Support.
    Ok(directories::BaseDirs::new()
        .context("cannot locate user configuration directory")?
        .config_dir()
        .join("img/config.toml"))
}
pub fn read_global(path: &Path) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).context("invalid configuration TOML"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).context("cannot read configuration"),
    }
}
pub fn apply_project(c: &mut Config, contents: &str) -> Result<()> {
    let p: Project = toml::from_str(contents)
        .context("project .img.toml only accepts provider, output.format and upload paths")?;
    ensure!(
        p.version.is_none_or(|v| v == 1),
        "unsupported project config version"
    );
    if let Some(v) = p.provider {
        c.provider = v;
    }
    if let Some(p) = p.output
        && let Some(v) = p.format
    {
        c.output.format = v;
    }
    if let Some(p) = p.upload {
        if let Some(v) = p.path {
            c.upload.path = v;
        }
        if let Some(v) = p.path_template {
            c.upload.path_template = v;
        }
    }
    Ok(())
}
pub fn load(global: &Path, project: Option<&Path>) -> Result<Config> {
    let mut c = read_global(global)?;
    if let Some(path) = project {
        match std::fs::read_to_string(path) {
            Ok(s) => apply_project(&mut c, &s)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    for (key, dest) in [
        ("IMG_PROVIDER", &mut c.provider),
        ("IMG_DEFAULT_PROVIDER", &mut c.default_provider),
        ("IMG_OUTPUT_FORMAT", &mut c.output.format),
    ] {
        if let Ok(v) = std::env::var(key)
            && !v.is_empty()
        {
            *dest = v;
        }
    }
    if let Ok(v) = std::env::var("IMG_OUTPUT_COPY") {
        c.output.copy = parse_bool(&v)?;
    }
    if let Ok(v) = std::env::var("IMG_UPLOAD_CONCURRENCY") {
        c.upload.concurrency = v.parse().context("invalid IMG_UPLOAD_CONCURRENCY")?;
    }
    c.validate()?;
    Ok(c)
}
pub fn parse_bool(s: &str) -> Result<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" | "t" => Ok(true),
        "false" | "0" | "f" => Ok(false),
        _ => bail!("expected true or false"),
    }
}
pub fn valid_format(s: &str) -> bool {
    matches!(s, "url" | "markdown" | "html" | "json")
}
pub fn is_sensitive(s: &str) -> bool {
    let n = s.to_ascii_lowercase().replace(['-', '_', ' '], "");
    [
        "token",
        "secret",
        "password",
        "authorization",
        "apikey",
        "accesskey",
    ]
    .iter()
    .any(|x| n.contains(x))
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported configuration version (expected 1)"
        );
        ensure!(valid_format(&self.output.format), "unknown output format");
        ensure!(
            (1..=32).contains(&self.upload.concurrency),
            "upload.concurrency must be 1–32"
        );
        ensure!(
            (1..=128 << 20).contains(&self.upload.max_size),
            "upload.max_size must be 1–134217728 bytes"
        );
        ensure!(
            self.upload.retry_count <= 8,
            "upload.retry_count must be 0–8"
        );
        ensure!(
            self.upload.max_width <= 32768,
            "upload.max_width must be 0–32768"
        );
        ensure!(
            matches!(
                self.upload.rename.as_str(),
                "original" | "timestamp" | "hash" | "uuid"
            ),
            "unknown upload.rename"
        );
        ensure!(
            matches!(self.upload.conflict.as_str(), "error" | "overwrite"),
            "unknown upload.conflict"
        );
        for (name, p) in &self.providers {
            p.validate()
                .with_context(|| format!("invalid provider {name:?}"))?;
            if !self.allow_plaintext_credentials {
                for (key, v) in [
                    ("access_key", &p.access_key),
                    ("secret_key", &p.secret_key),
                    ("session_token", &p.session_token),
                    ("token", &p.token),
                ]
                .into_iter()
                .chain(p.headers.iter().map(|(k, v)| (k.as_str(), v)))
                .chain(p.fields.iter().map(|(k, v)| (k.as_str(), v)))
                {
                    if is_sensitive(key) && !v.is_empty() {
                        ensure!(
                            REFERENCE.is_match(v),
                            "provider {name:?} contains plaintext {key}; use an environment reference or allow_plaintext_credentials = true"
                        );
                    }
                }
            }
        }
        for name in [&self.default_provider, &self.provider] {
            ensure!(
                name.is_empty() || self.providers.contains_key(name),
                "provider {name:?} is not configured"
            );
        }
        Ok(())
    }
    pub fn selected(&self, name: &str) -> Result<(&str, &ProviderConfig)> {
        let name = if name.is_empty() {
            if self.provider.is_empty() {
                &self.default_provider
            } else {
                &self.provider
            }
        } else {
            name
        };
        ensure!(!name.is_empty(), "no provider selected; run img init");
        self.providers
            .get_key_value(name)
            .map(|(k, v)| (k.as_str(), v))
            .context("selected provider is not configured")
    }
    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        for p in c.providers.values_mut() {
            for v in [
                &mut p.access_key,
                &mut p.secret_key,
                &mut p.session_token,
                &mut p.token,
            ] {
                if !v.is_empty() {
                    *v = "********".into();
                }
            }
            for (k, v) in p.headers.iter_mut().chain(p.fields.iter_mut()) {
                if is_sensitive(k) && !v.is_empty() {
                    *v = "********".into();
                }
            }
        }
        c
    }
}
impl ProviderConfig {
    pub fn validate(&self) -> Result<()> {
        match self.kind.as_str() {
            "http" => {
                ensure!(
                    !self.url_json_path.is_empty(),
                    "HTTP url_json_path is required"
                );
                crate::network::secure_url(&self.url, self.allow_insecure)?;
                ensure!(
                    self.method.is_empty()
                        || matches!(
                            self.method.to_ascii_uppercase().as_str(),
                            "POST" | "PUT" | "PATCH"
                        ),
                    "unsupported HTTP method"
                );
            }
            "s3" => {
                ensure!(
                    !self.bucket.is_empty() && !self.public_url.is_empty(),
                    "S3 bucket and public_url are required"
                );
                ensure!(
                    self.access_key.is_empty() == self.secret_key.is_empty(),
                    "both access_key and secret_key are required"
                );
                ensure!(
                    !self.bucket.contains(['/', '\\', '\r', '\n']),
                    "invalid S3 bucket name"
                );
                if !self.endpoint.is_empty() {
                    crate::network::secure_url(&self.endpoint, self.allow_insecure)?;
                }
            }
            "github" => {
                ensure!(
                    !self.owner.is_empty() && !self.repo.is_empty() && !self.token.is_empty(),
                    "GitHub owner, repo and token are required"
                );
            }
            _ => bail!("unknown provider type"),
        }
        if !self.public_url.is_empty() {
            crate::network::secure_url(&self.public_url, self.allow_insecure)?;
        }
        Ok(())
    }
    pub fn resolved(&self) -> Result<Self> {
        let mut v = serde_json::to_value(self)?;
        fn expand(v: &mut serde_json::Value) -> Result<()> {
            match v {
                serde_json::Value::String(s) => {
                    *s = resolve_with(s, |key| {
                        if let Ok(v) = std::env::var(key) {
                            return Ok(v);
                        }
                        #[cfg(target_os = "macos")]
                        if key.starts_with("IMG_DESKTOP_") {
                            return security_framework::passwords::get_generic_password(
                                "dev.img.desktop.storage",
                                key,
                            )
                            .map_err(|_| anyhow::anyhow!("cannot read desktop credential {key}"))
                            .and_then(|v| String::from_utf8(v).context("credential is not UTF-8"));
                        }
                        bail!("required environment variable {key} is not set")
                    })?
                }
                serde_json::Value::Object(m) => {
                    for v in m.values_mut() {
                        expand(v)?;
                    }
                }
                serde_json::Value::Array(a) => {
                    for v in a {
                        expand(v)?;
                    }
                }
                _ => (),
            }
            Ok(())
        }
        expand(&mut v)?;
        Ok(serde_json::from_value(v)?)
    }
    pub fn sanitize(&self, text: &str) -> String {
        let mut out = text.to_string();
        for value in [
            &self.access_key,
            &self.secret_key,
            &self.session_token,
            &self.token,
        ]
        .into_iter()
        .chain(self.headers.values())
        .chain(self.fields.values())
        {
            if !value.is_empty() {
                out = out.replace(value, "********");
                for part in value.split_whitespace().skip(1).filter(|s| s.len() >= 4) {
                    out = out.replace(part, "********");
                }
            }
        }
        out.chars().filter(|c| !c.is_control()).take(512).collect()
    }
}
pub fn resolve_with(s: &str, mut lookup: impl FnMut(&str) -> Result<String>) -> Result<String> {
    let mut out = String::new();
    let mut pos = 0;
    for c in REFERENCE.captures_iter(s) {
        let m = c.get(0).unwrap();
        out.push_str(&s[pos..m.start()]);
        out.push_str(&lookup(&c[1])?);
        pos = m.end();
    }
    out.push_str(&s[pos..]);
    Ok(out)
}
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.exists() {
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(parent)?;
    }
    let mut f = tempfile::NamedTempFile::new_in(parent)?;
    f.write_all(bytes)?;
    f.as_file().sync_all()?;
    f.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn save(path: &Path, c: &Config) -> Result<()> {
    c.validate()?;
    write_atomic(path, toml::to_string_pretty(c)?.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_defaults_and_project_boundary() {
        let mut c: Config = toml::from_str("version=1\n[upload]\nmax_width=1000").unwrap();
        assert_eq!(c.upload.concurrency, 4);
        assert_eq!(c.output.format, "url");
        apply_project(&mut c, "[upload]\npath='posts'").unwrap();
        assert_eq!(c.upload.max_width, 1000);
        for s in [
            "[providers.evil]\ntype='http'",
            "[output]\ncopy=true",
            "[upload]\nretry_count=3",
            "allow_plaintext_credentials=true",
        ] {
            assert!(apply_project(&mut c, s).is_err());
        }
    }
    #[test]
    fn references_and_redaction_never_resolve_for_display() {
        assert_eq!(
            resolve_with("Bearer ${TOKEN}", |s| {
                assert_eq!(s, "TOKEN");
                Ok("hello".into())
            })
            .unwrap(),
            "Bearer hello"
        );
        assert!(resolve_with("${MISSING}", |_| bail!("missing")).is_err());
        let mut c = Config::default();
        let mut p = ProviderConfig {
            kind: "http".into(),
            url: "https://example.test".into(),
            url_json_path: "url".into(),
            ..Default::default()
        };
        p.headers
            .insert("Authorization".into(), "Bearer raw-secret".into());
        c.providers.insert("a".into(), p);
        assert!(c.validate().is_err());
        assert!(
            !toml::to_string(&c.redacted())
                .unwrap()
                .contains("raw-secret")
        );
    }
    #[test]
    fn atomic_config_roundtrip_retains_unknown_global_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let c: Config = toml::from_str("version=1\nfuture_option='kept'").unwrap();
        save(&p, &c).unwrap();
        assert_eq!(
            read_global(&p).unwrap().extra["future_option"].as_str(),
            Some("kept")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(p.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
