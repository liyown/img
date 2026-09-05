use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

const KEYCHAIN_SERVICE: &str = "dev.img.desktop.storage";
const SECRET_PREFIX: &str = "IMG_DESKTOP_";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    S3,
    R2,
    Oss,
    Github,
    Http,
}

#[derive(Clone, Copy)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
    pub secret: bool,
    pub required: bool,
}

impl ProviderKind {
    pub const ALL: [Self; 5] = [Self::S3, Self::R2, Self::Oss, Self::Github, Self::Http];
    pub fn label(self) -> &'static str {
        match self {
            Self::S3 => "Amazon S3 / 兼容 S3",
            Self::R2 => "Cloudflare R2",
            Self::Oss => "阿里云 OSS（S3 兼容）",
            Self::Github => "GitHub",
            Self::Http => "自定义 HTTP",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::S3 => "s3",
            Self::R2 => "r2",
            Self::Oss => "oss",
            Self::Github => "github",
            Self::Http => "http",
        }
    }
    fn engine_type(self) -> &'static str {
        match self {
            Self::S3 | Self::R2 | Self::Oss => "s3",
            Self::Github => "github",
            Self::Http => "http",
        }
    }
    pub fn fields(self) -> Vec<Field> {
        let text = |key, label, placeholder, required| Field {
            key,
            label,
            placeholder,
            secret: false,
            required,
        };
        let secret = |key, label, required| Field {
            key,
            label,
            placeholder: "输入后安全保存",
            secret: true,
            required,
        };
        match self {
            Self::S3 | Self::R2 | Self::Oss => vec![
                text(
                    "endpoint",
                    "服务地址",
                    match self {
                        Self::R2 => "https://<账户 ID>.r2.cloudflarestorage.com",
                        Self::Oss => "https://oss-cn-hangzhou.aliyuncs.com",
                        _ => "https://…（Amazon S3 可留空）",
                    },
                    self != Self::S3,
                ),
                text(
                    "region",
                    "区域",
                    if self == Self::R2 {
                        "auto"
                    } else {
                        "us-east-1"
                    },
                    false,
                ),
                text("bucket", "存储桶", "images", true),
                text(
                    "public_url",
                    "公开访问地址",
                    "https://img.example.com",
                    true,
                ),
                secret("access_key", "Access Key ID", false),
                secret("secret_key", "Secret Access Key", false),
                secret("session_token", "临时会话令牌（可选）", false),
            ],
            Self::Github => vec![
                text("owner", "用户或组织", "GitHub 用户名", true),
                text("repo", "仓库名称", "images", true),
                text("branch", "分支", "main", false),
                text(
                    "public_url",
                    "自定义访问地址",
                    "可选，留空使用 GitHub 地址",
                    false,
                ),
                secret("token", "访问令牌", true),
                text("commit_message", "提交说明", "upload: {path}", false),
            ],
            Self::Http => vec![
                text("url", "上传接口", "https://example.com/api/upload", true),
                text("file_field", "文件字段", "file", false),
                text("url_json_path", "返回链接字段", "data.url", true),
                secret("authorization", "Authorization 请求头", false),
            ],
        }
    }
}

#[derive(Clone)]
pub struct ProviderDraft {
    pub original_name: Option<String>,
    pub name: String,
    pub kind: ProviderKind,
    pub values: BTreeMap<String, String>,
    pub path_style: bool,
    pub allow_insecure: bool,
    pub method: String,
    pub headers: Vec<ExtraField>,
    pub fields: Vec<ExtraField>,
}

#[derive(Clone, Default)]
pub struct ExtraField {
    pub key: String,
    pub value: String,
    pub original_key: Option<String>,
}

impl ProviderDraft {
    pub fn new(kind: ProviderKind) -> Self {
        let mut values = BTreeMap::new();
        for (key, value) in match kind {
            ProviderKind::R2 => vec![("region", "auto")],
            ProviderKind::Github => vec![("branch", "main")],
            ProviderKind::Http => vec![("file_field", "file"), ("url_json_path", "data.url")],
            _ => vec![],
        } {
            values.insert(key.into(), value.into());
        }
        Self {
            original_name: None,
            name: String::new(),
            kind,
            values,
            path_style: matches!(kind, ProviderKind::S3 | ProviderKind::R2),
            allow_insecure: false,
            method: "POST".into(),
            headers: vec![],
            fields: vec![],
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("APERTURE_CONFIG_PATH") {
        return Ok(path.into());
    }
    Ok(directories::BaseDirs::new()
        .context("无法确定用户目录")?
        .config_dir()
        .join("img/config.toml"))
}

fn load(path: &Path) -> Result<(Table, Option<Vec<u8>>)> {
    if !path.exists() {
        return Ok((Table::new(), None));
    }
    let bytes = std::fs::read(path).context("无法读取存储设置")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("存储设置格式无法读取"))?;
    // Parser diagnostics may contain credential-bearing source lines.
    let table =
        toml::from_str(text).map_err(|_| anyhow::anyhow!("存储设置格式无法读取，原文件已保留"))?;
    Ok((table, Some(bytes)))
}

pub fn configured_providers() -> Result<(Vec<(String, String)>, String)> {
    let (table, _) = load(&config_path()?)?;
    let providers = table
        .get("providers")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .map(|(name, p)| {
            (
                name.clone(),
                p.get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect();
    Ok((
        providers,
        table
            .get("default_provider")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
    ))
}

fn field_value<'a>(table: &'a Table, key: &str) -> &'a str {
    if key == "authorization" {
        table
            .get("headers")
            .and_then(|v| v.get("Authorization"))
            .and_then(Value::as_str)
            .unwrap_or("")
    } else {
        table.get(key).and_then(Value::as_str).unwrap_or("")
    }
}

pub fn edit_provider(name: &str) -> Result<ProviderDraft> {
    let (table, _) = load(&config_path()?)?;
    let provider = table
        .get("providers")
        .and_then(|v| v.get(name))
        .and_then(Value::as_table)
        .context("存储源已不存在")?;
    let kind = match provider
        .get("desktop_preset")
        .and_then(Value::as_str)
        .unwrap_or_else(|| field_value(provider, "type"))
    {
        "r2" => ProviderKind::R2,
        "oss" => ProviderKind::Oss,
        "s3" => ProviderKind::S3,
        "github" => ProviderKind::Github,
        "http" => ProviderKind::Http,
        _ => anyhow::bail!("暂不支持编辑此存储源类型"),
    };
    Ok(ProviderDraft {
        original_name: Some(name.into()),
        name: name.into(),
        kind,
        // Secrets remain empty in the editor. Saving an empty field preserves its existing value.
        values: kind
            .fields()
            .iter()
            .filter(|f| !f.secret)
            .map(|f| (f.key.into(), field_value(provider, f.key).into()))
            .collect(),
        allow_insecure: provider
            .get("allow_insecure")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        method: provider
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("POST")
            .into(),
        headers: extra_fields(provider, "headers"),
        fields: extra_fields(provider, "fields"),
        path_style: provider
            .get("path_style")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn extra_fields(provider: &Table, group: &str) -> Vec<ExtraField> {
    provider
        .get(group)
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(key, _)| !(group == "headers" && key.eq_ignore_ascii_case("authorization")))
        .map(|(key, _)| ExtraField {
            key: key.clone(),
            value: String::new(),
            original_key: Some(key.clone()),
        })
        .collect()
}

fn set_field(table: &mut Table, key: &str, value: String) -> Result<()> {
    let target = if key == "authorization" {
        table
            .entry("headers")
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .context("请求头设置无法读取")?
    } else {
        table
    };
    let key = if key == "authorization" {
        "Authorization"
    } else {
        key
    };
    if value.is_empty() {
        target.remove(key);
    } else {
        target.insert(key.into(), Value::String(value));
    }
    Ok(())
}

fn validate_provider(draft: &ProviderDraft, provider: &Table) -> Result<()> {
    ensure!(!draft.name.trim().is_empty(), "请填写存储源名称");
    ensure!(
        draft.name.len() <= 100 && !draft.name.chars().any(char::is_control),
        "存储源名称无效"
    );
    for field in draft.kind.fields() {
        ensure!(
            !field.required || !field_value(provider, field.key).is_empty(),
            "请填写{}",
            field.label
        );
    }
    if draft.kind.engine_type() == "s3" {
        ensure!(
            field_value(provider, "access_key").is_empty()
                == field_value(provider, "secret_key").is_empty(),
            "Access Key ID 和 Secret Access Key 需要一起填写"
        );
    }
    let allow_insecure = provider
        .get("allow_insecure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    for key in ["url", "endpoint", "public_url"] {
        let value = field_value(provider, key);
        if value.is_empty() {
            continue;
        }
        let parsed = url::Url::parse(value)
            .map_err(|_| anyhow::anyhow!("请填写有效的服务或公开访问地址"))?;
        ensure!(
            parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.fragment().is_none(),
            "地址不能包含用户名、密码或锚点"
        );
        ensure!(
            parsed.scheme() == "https" || (allow_insecure && parsed.scheme() == "http"),
            "服务和公开访问地址需要使用 HTTPS"
        );
    }
    Ok(())
}

pub trait CredentialStore {
    fn set(&self, key: &str, value: &[u8]) -> Result<()>;
    fn get(&self, key: &str) -> Result<Vec<u8>>;
    fn delete(&self, key: &str);
}
pub struct SystemCredentials;
impl CredentialStore for SystemCredentials {
    fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, key, value)
                .map_err(|_| anyhow::anyhow!("无法保存到系统钥匙串，请检查系统授权"))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (key, value);
            anyhow::bail!("当前系统暂不支持安全保存凭据")
        }
    }
    fn get(&self, key: &str) -> Result<Vec<u8>> {
        #[cfg(target_os = "macos")]
        {
            security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, key)
                .map_err(|_| anyhow::anyhow!("无法读取存储凭据，请在设置中重新填写并保存"))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = key;
            anyhow::bail!("当前系统暂不支持读取凭据")
        }
    }
    fn delete(&self, key: &str) {
        #[cfg(target_os = "macos")]
        {
            let _ = security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, key);
        }
    }
}

fn save_document(path: &Path, table: &Table, expected: Option<&[u8]>) -> Result<()> {
    let parent = path.parent().context("存储设置路径无效")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(toml::to_string_pretty(table)?.as_bytes())?;
    file.as_file().sync_all()?;
    ensure!(
        std::fs::read(path).ok().as_deref() == expected,
        "设置已在其他地方更新，请重新打开编辑后再保存"
    );
    file.persist(path)
        .map_err(|_| anyhow::anyhow!("无法保存存储设置"))?;
    Ok(())
}

pub fn save_provider(
    path: &Path,
    draft: &ProviderDraft,
    credentials: &impl CredentialStore,
) -> Result<()> {
    let (mut document, before) = load(path)?;
    let providers = document
        .entry("providers")
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .context("存储源设置无法读取")?;
    let mut provider = if let Some(original) = &draft.original_name {
        ensure!(original == &draft.name, "已有存储源的名称不能更改");
        providers
            .get(original)
            .and_then(Value::as_table)
            .context("存储源已不存在，请重新添加")?
            .clone()
    } else {
        ensure!(
            !providers.contains_key(&draft.name),
            "此名称已存在，请选择其他名称"
        );
        Table::new()
    };
    let mut secrets = vec![];
    provider.insert(
        "allow_insecure".into(),
        Value::Boolean(draft.allow_insecure),
    );
    if draft.kind == ProviderKind::Http {
        ensure!(
            ["POST", "PUT", "PATCH"].contains(&draft.method.as_str()),
            "上传请求方式无效"
        );
        provider.insert("method".into(), Value::String(draft.method.clone()));
        for (group, rows) in [("headers", &draft.headers), ("fields", &draft.fields)] {
            let old = provider
                .get(group)
                .and_then(Value::as_table)
                .cloned()
                .unwrap_or_default();
            let mut entries = Table::new();
            if group == "headers" {
                if let Some(auth) = old
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                    .map(|(_, v)| v.clone())
                {
                    entries.insert("Authorization".into(), auth);
                }
            }
            let mut seen = BTreeSet::new();
            for row in rows {
                let name = row.key.trim();
                ensure!(
                    !name.is_empty() && name.len() <= 200 && !name.chars().any(char::is_control),
                    "额外字段名称不能为空或包含换行"
                );
                if group == "headers" {
                    ensure!(
                        name.bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)),
                        "请求头名称无效"
                    );
                    ensure!(
                        ![
                            "authorization",
                            "content-type",
                            "content-length",
                            "host",
                            "transfer-encoding"
                        ]
                        .contains(&name.to_ascii_lowercase().as_str()),
                        "此请求头由应用管理，请使用授权字段填写 Authorization"
                    );
                }
                let comparison = if group == "headers" {
                    name.to_lowercase()
                } else {
                    name.to_string()
                };
                ensure!(seen.insert(comparison), "额外字段名称重复");
                ensure!(!row.value.contains(['\r', '\n']), "字段值不能包含换行");
                let value = if row.value.is_empty() {
                    row.original_key
                        .as_ref()
                        .and_then(|key| old.get(key))
                        .cloned()
                        .unwrap_or(Value::String(String::new()))
                } else {
                    let key = format!(
                        "{SECRET_PREFIX}{}",
                        uuid::Uuid::new_v4().simple().to_string().to_uppercase()
                    );
                    secrets.push((key.clone(), row.value.as_bytes().to_vec()));
                    Value::String(format!("${{{key}}}"))
                };
                entries.insert(name.into(), value);
            }
            provider.insert(group.into(), Value::Table(entries));
        }
    }
    for field in draft.kind.fields() {
        let value = draft.values.get(field.key).map(|v| v.trim()).unwrap_or("");
        if field.secret {
            if value.is_empty() {
                continue;
            }
            let key = format!(
                "{SECRET_PREFIX}{}",
                uuid::Uuid::new_v4().simple().to_string().to_uppercase()
            );
            set_field(&mut provider, field.key, format!("${{{key}}}"))?;
            secrets.push((key, value.as_bytes().to_vec()));
        } else {
            set_field(&mut provider, field.key, value.to_owned())?;
        }
    }
    provider.insert(
        "type".into(),
        Value::String(draft.kind.engine_type().into()),
    );
    provider.insert(
        "desktop_preset".into(),
        Value::String(draft.kind.id().into()),
    );
    if draft.kind.engine_type() == "s3" {
        provider.insert("path_style".into(), Value::Boolean(draft.path_style));
    }
    validate_provider(draft, &provider)?;
    providers.insert(draft.name.clone(), Value::Table(provider));
    document.entry("version").or_insert(Value::Integer(1));
    if document
        .get("default_provider")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        document.insert("default_provider".into(), Value::String(draft.name.clone()));
    }
    let mut saved = vec![];
    let result = (|| -> Result<()> {
        for (key, value) in &secrets {
            credentials.set(key, value)?;
            saved.push(key);
        }
        save_document(path, &document, before.as_deref())
    })();
    if result.is_err() {
        for key in saved {
            credentials.delete(key);
        }
    }
    if result.is_ok() {
        if let Some(before) = &before {
            cleanup_unused_credentials(before, &document, credentials);
        }
    }
    result
}

fn cleanup_unused_credentials(before: &[u8], after: &Table, credentials: &impl CredentialStore) {
    if let Ok(previous) = toml::from_str::<Table>(std::str::from_utf8(before).unwrap_or("")) {
        let mut old = BTreeSet::new();
        let mut kept = BTreeSet::new();
        credential_keys(&Value::Table(previous), &mut old);
        credential_keys(&Value::Table(after.clone()), &mut kept);
        for key in old.difference(&kept) {
            credentials.delete(key);
        }
    }
}

pub fn remove_provider(path: &Path, name: &str, credentials: &impl CredentialStore) -> Result<()> {
    let (mut document, before) = load(path)?;
    let providers = document
        .get_mut("providers")
        .and_then(Value::as_table_mut)
        .context("存储源已不存在")?;
    ensure!(providers.remove(name).is_some(), "存储源已不存在");
    let replacement = providers.keys().next().cloned().unwrap_or_default();
    for key in ["default_provider", "provider"] {
        if document.get(key).and_then(Value::as_str) == Some(name) {
            document.insert(key.into(), Value::String(replacement.clone()));
        }
    }
    save_document(path, &document, before.as_deref())?;
    if let Some(before) = before {
        cleanup_unused_credentials(&before, &document, credentials);
    }
    Ok(())
}

pub fn test_provider(name: &str, binary: &Path) -> Result<()> {
    let (config, environment) = engine_config(&config_path()?, name, &SystemCredentials)?;
    let directory = tempfile::tempdir()?;
    let mut file = tempfile::NamedTempFile::new_in(directory.path())?;
    file.write_all(config.as_bytes())?;
    let mut command = std::process::Command::new(binary);
    command
        .current_dir(directory.path())
        .env_remove("IMG_PROVIDER")
        .env_remove("IMG_DEFAULT_PROVIDER")
        .env_remove("IMG_OUTPUT_FORMAT")
        .env_remove("IMG_UPLOAD_CONCURRENCY")
        .envs(environment)
        .arg("--config")
        .arg(file.path())
        .args(["provider", "test", name]);
    let result = crate::engine::run(command, &crate::engine::Control::default())?;
    ensure!(
        result.success,
        "连接测试失败，请检查服务地址、凭据和访问权限"
    );
    Ok(())
}

pub fn set_default(name: &str) -> Result<()> {
    let path = config_path()?;
    let (mut table, before) = load(&path)?;
    ensure!(
        table.get("providers").and_then(|v| v.get(name)).is_some(),
        "存储源已不存在"
    );
    table.insert("default_provider".into(), Value::String(name.into()));
    save_document(&path, &table, before.as_deref())
}

fn credential_keys(value: &Value, keys: &mut BTreeSet<String>) {
    match value {
        Value::String(value) => {
            for part in value.split("${").skip(1) {
                if let Some((key, _)) = part.split_once('}') {
                    if key.starts_with(SECRET_PREFIX)
                        && key
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    {
                        keys.insert(key.to_owned());
                    }
                }
            }
        }
        Value::Table(table) => {
            for value in table.values() {
                credential_keys(value, keys);
            }
        }
        Value::Array(values) => {
            for value in values {
                credential_keys(value, keys);
            }
        }
        _ => {}
    }
}

pub fn engine_config(
    path: &Path,
    target: &str,
    credentials: &impl CredentialStore,
) -> Result<(String, BTreeMap<String, String>)> {
    let (mut table, _) = load(path)?;
    let provider = table
        .get("providers")
        .and_then(|v| v.get(target))
        .context("请先在设置中添加存储源")?
        .clone();
    let mut keys = BTreeSet::new();
    credential_keys(&provider, &mut keys);
    let mut environment = BTreeMap::new();
    for key in keys {
        let value = String::from_utf8(credentials.get(&key)?)
            .map_err(|_| anyhow::anyhow!("存储凭据无法读取"))?;
        environment.insert(key, value);
    }
    table.insert(
        "providers".into(),
        Value::Table([(target.to_owned(), provider)].into_iter().collect()),
    );
    table.remove("provider");
    table.insert("default_provider".into(), Value::String(target.into()));
    table.entry("version").or_insert(Value::Integer(1));
    table.insert(
        "output".into(),
        Value::Table(
            [
                ("quiet".into(), Value::Boolean(false)),
                ("copy".into(), Value::Boolean(false)),
                ("format".into(), Value::String("json".into())),
            ]
            .into_iter()
            .collect(),
        ),
    );
    Ok((toml::to_string_pretty(&table)?, environment))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct MemoryCredentials(RefCell<BTreeMap<String, Vec<u8>>>);
    impl CredentialStore for MemoryCredentials {
        fn set(&self, key: &str, value: &[u8]) -> Result<()> {
            self.0.borrow_mut().insert(key.into(), value.into());
            Ok(())
        }
        fn get(&self, key: &str) -> Result<Vec<u8>> {
            self.0
                .borrow()
                .get(key)
                .cloned()
                .context("missing credential")
        }
        fn delete(&self, key: &str) {
            self.0.borrow_mut().remove(key);
        }
    }
    #[test]
    fn form_saves_without_plaintext_secrets_and_preserves_other_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "version = 1\n[upload]\nrename = 'uuid'\n[output]\nquiet = true\n[providers.other]\ntype = 'http'\nurl = 'https://example.com'\nurl_json_path = 'url'\n").unwrap();
        let credentials = MemoryCredentials::default();
        let mut draft = ProviderDraft::new(ProviderKind::Github);
        draft.name = "photos".into();
        draft.values.extend([
            ("owner".into(), "test".into()),
            ("repo".into(), "images".into()),
            ("token".into(), "test-only-secret".into()),
        ]);
        save_provider(&path, &draft, &credentials).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("test-only-secret"));
        assert!(saved.contains("other"));
        let (snapshot, environment) = engine_config(&path, "photos", &credentials).unwrap();
        let config: Table = toml::from_str(&snapshot).unwrap();
        assert_eq!(config["upload"]["rename"].as_str(), Some("uuid"));
        assert_eq!(config["output"]["quiet"].as_bool(), Some(false));
        assert_eq!(environment.values().next().unwrap(), "test-only-secret");
        assert_eq!(config["providers"].as_table().unwrap().len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        assert!(save_provider(&path, &draft, &credentials).is_err());
        draft.original_name = Some(draft.name.clone());
        draft.values.insert("token".into(), String::new());
        save_provider(&path, &draft, &credentials).unwrap();
        assert_eq!(
            engine_config(&path, "photos", &credentials).unwrap().1,
            environment
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn advanced_http_fields_preserve_and_delete_credentials_transactionally() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        let credentials = MemoryCredentials::default();
        let mut draft = ProviderDraft::new(ProviderKind::Http);
        draft.name = "custom".into();
        draft.allow_insecure = true;
        draft.method = "PATCH".into();
        draft
            .values
            .insert("url".into(), "http://127.0.0.1/upload".into());
        draft.headers.push(ExtraField {
            key: "X-Token".into(),
            value: "header-secret".into(),
            original_key: None,
        });
        draft.fields.push(ExtraField {
            key: "album".into(),
            value: "private-album".into(),
            original_key: None,
        });
        save_provider(&path, &draft, &credentials).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("header-secret") && !text.contains("private-album"));
        assert_eq!(credentials.0.borrow().len(), 2);
        draft.original_name = Some(draft.name.clone());
        draft.headers[0].original_key = Some("X-Token".into());
        draft.headers[0].value.clear();
        draft.fields.clear();
        save_provider(&path, &draft, &credentials).unwrap();
        assert_eq!(credentials.0.borrow().len(), 1);
        let (config, values) = engine_config(&path, "custom", &credentials).unwrap();
        assert_eq!(values.values().next().unwrap(), "header-secret");
        assert!(config.contains("PATCH"));
        remove_provider(&path, "custom", &credentials).unwrap();
        assert!(credentials.0.borrow().is_empty());
        let table: Table = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(table["default_provider"].as_str(), Some(""));
        assert!(table["providers"].as_table().unwrap().is_empty());
    }
    #[test]
    fn invalid_input_and_malformed_config_do_not_overwrite_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let credentials = MemoryCredentials::default();
        let mut draft = ProviderDraft::new(ProviderKind::R2);
        draft.name = "r2".into();
        assert!(save_provider(&path, &draft, &credentials).is_err());
        assert!(!path.exists());
        assert!(credentials.0.borrow().is_empty());
        std::fs::write(&path, "invalid secret=\"").unwrap();
        let error = save_provider(&path, &draft, &credentials)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("invalid secret"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid secret=\"");
    }

    #[test]
    fn credential_failure_rolls_back_new_entries_without_changing_config() {
        struct RejectSecondWrite(MemoryCredentials);
        impl CredentialStore for RejectSecondWrite {
            fn set(&self, key: &str, value: &[u8]) -> Result<()> {
                ensure!(self.0.0.borrow().is_empty(), "keychain unavailable");
                self.0.set(key, value)
            }
            fn get(&self, key: &str) -> Result<Vec<u8>> {
                self.0.get(key)
            }
            fn delete(&self, key: &str) {
                self.0.delete(key);
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let original = "version = 1\n[upload]\nrename = 'uuid'\n";
        std::fs::write(&path, original).unwrap();
        let credentials = RejectSecondWrite(MemoryCredentials::default());
        let mut draft = ProviderDraft::new(ProviderKind::R2);
        draft.name = "test".into();
        draft.values.extend([
            ("endpoint".into(), "https://example.test".into()),
            ("bucket".into(), "images".into()),
            ("public_url".into(), "https://cdn.example.test".into()),
            ("access_key".into(), "test-access".into()),
            ("secret_key".into(), "test-secret".into()),
        ]);
        assert!(save_provider(&path, &draft, &credentials).is_err());
        assert!(credentials.0.0.borrow().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
