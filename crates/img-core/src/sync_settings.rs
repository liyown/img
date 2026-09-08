//! Device-local sync connection. Its credentials never enter the metadata log.
use crate::{
    config::{self, ProviderConfig},
    provider::Provider,
    sync_log::RemoteLog,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncSettings {
    pub version: u32,
    pub credential_ref: String,
    pub kind: String,
    pub endpoint: String,
    pub prefix: String,
    #[serde(default)]
    pub namespace: String,
    pub paused: bool,
}
impl SyncSettings {
    pub fn read(root: &Path) -> Result<Self> {
        let value: Self = serde_json::from_slice(&std::fs::read(root.join("sync.json"))?)?;
        ensure!(value.version == 1, "unsupported sync configuration");
        ensure!(
            value.credential_ref.starts_with("IMG_SYNC_CONNECTION_")
                && value
                    .credential_ref
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "invalid sync credential reference"
        );
        Ok(value)
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        config::write_atomic(&root.join("sync.json"), &serde_json::to_vec_pretty(self)?)
    }
    pub fn configure(root: &Path, provider: &ProviderConfig, prefix: &str) -> Result<Self> {
        let mut connection = provider.clone();
        if connection.public_url.is_empty() {
            connection.public_url = connection.endpoint.clone();
        }
        let provider = &connection;
        let remote = RemoteLog::new(Provider::new("sync", provider)?, prefix)?;
        crate::sync_log::verify_conditions(&remote, &Default::default())?;
        let key = format!("IMG_SYNC_CONNECTION_{}", uuid::Uuid::new_v4().simple());
        img_records::credentials::set(&key, &serde_json::to_vec(provider)?)?;
        let settings = Self {
            version: 1,
            credential_ref: key.clone(),
            kind: provider.kind.clone(),
            endpoint: provider.endpoint.clone(),
            prefix: prefix.into(),
            namespace: remote.provider.namespace(),
            paused: false,
        };
        if let Err(error) = settings.save(root) {
            img_records::credentials::remove(&key);
            return Err(error);
        }
        Ok(settings)
    }
    pub fn remote(&self) -> Result<RemoteLog> {
        let bytes = img_records::credentials::get(&self.credential_ref)
            .context("sync connection credentials are missing on this device")?;
        let provider: ProviderConfig = serde_json::from_slice(&bytes)?;
        RemoteLog::new(Provider::new("sync", &provider)?, &self.prefix)
    }
}
