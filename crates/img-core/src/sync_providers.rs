//! Portable provider fields refer to immutable keychain slots; only wire batches contain secrets.
use crate::config::{self, Config, REFERENCE};
use anyhow::{Context, Result, ensure};
use img_records::{
    catalog::{Catalog, digest},
    sync::Event,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
const PREFIX: &str = "IMG_DESKTOP_SYNC_";
pub trait Secrets {
    fn get(&self, key: &str) -> Result<String>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn remove(&self, key: &str);
}
pub struct Keychain;
impl Secrets for Keychain {
    fn get(&self, key: &str) -> Result<String> {
        String::from_utf8(img_records::credentials::get(key)?).context("credential is not UTF-8")
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        img_records::credentials::set(key, value.as_bytes())
    }
    fn remove(&self, key: &str) {
        img_records::credentials::remove(key)
    }
}
fn local_resolve(value: &str, secrets: &impl Secrets) -> Result<String> {
    let mut out = String::new();
    let mut end = 0;
    for cap in REFERENCE.captures_iter(value) {
        let whole = cap.get(0).unwrap();
        out.push_str(&value[end..whole.start()]);
        if cap[1].starts_with("IMG_DESKTOP_") {
            out.push_str(&secrets.get(&cap[1])?);
        } else {
            out.push_str(whole.as_str());
        }
        end = whole.end();
    }
    out.push_str(&value[end..]);
    Ok(out)
}
fn portable(
    value: &Value,
    previous: &Value,
    private: bool,
    secrets: &impl Secrets,
) -> Result<Value> {
    Ok(match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(
                    k.clone(),
                    portable(
                        v,
                        &previous[k],
                        private || config::is_sensitive(k) || k == "headers" || k == "fields",
                        secrets,
                    )?,
                );
            }
            Value::Object(out)
        }
        Value::String(text) => {
            let has_keychain = REFERENCE
                .captures_iter(text)
                .any(|c| c[1].starts_with("IMG_DESKTOP_"));
            let environment_only = REFERENCE.is_match(text) && !has_keychain;
            if text.is_empty() || (!private && !has_keychain) || environment_only {
                value.clone()
            } else {
                let plaintext = local_resolve(text, secrets)?;
                if let Some(old) = previous.as_str()
                    && old.contains(PREFIX)
                    && local_resolve(old, secrets).ok().as_deref() == Some(&plaintext)
                {
                    previous.clone()
                } else {
                    let key = format!("{PREFIX}{}", uuid::Uuid::new_v4().simple());
                    secrets.set(&key, &plaintext)?;
                    Value::String(format!("${{{key}}}"))
                }
            }
        }
        _ => value.clone(),
    })
}
pub fn capture(catalog: &mut Catalog, path: &Path, secrets: &impl Secrets) -> Result<String> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let cfg = config::read_global(path)?;
    let previous: Value = catalog
        .setting("sync-provider-baseline")?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or(json!({}));
    let current = portable(
        &serde_json::to_value(&cfg.providers)?,
        &previous,
        false,
        secrets,
    )?;
    for (name, provider) in current.as_object().context("invalid provider map")? {
        for (field, value) in provider.as_object().context("invalid provider fields")? {
            if previous[name][field] != *value {
                catalog.sync_set(&format!("provider:{name}"), field, Some(value.clone()))?;
            }
        }
    }
    if let Some(old) = previous.as_object() {
        for name in old.keys() {
            if current.get(name).is_none() {
                catalog.sync_set(&format!("provider:{name}"), "$deleted", Some(true.into()))?;
            }
        }
    }
    catalog.set_setting("sync-provider-baseline", &serde_json::to_string(&current)?)?;
    Ok(digest(&bytes))
}
pub fn apply(catalog: &Catalog, path: &Path, expected: &str, secrets: &impl Secrets) -> Result<()> {
    let bytes = std::fs::read(path).unwrap_or_default();
    ensure!(
        digest(&bytes) == expected,
        "configuration changed during sync; reload and synchronize again"
    );
    let mut cfg: Config = config::read_global(path)?;
    let baseline: Value = catalog
        .setting("sync-provider-baseline")?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or(json!({}));
    cfg.providers = serde_json::from_value(portable(
        &serde_json::to_value(&cfg.providers)?,
        &baseline,
        false,
        secrets,
    )?)?;
    let entities: BTreeSet<_> = catalog
        .sync_events(None)?
        .into_iter()
        .map(|e| e.entity)
        .filter(|e| e.starts_with("provider:"))
        .collect();
    for entity in entities {
        let name = entity.strip_prefix("provider:").unwrap();
        let state = catalog.sync_entity(&entity)?;
        if state.deleted {
            cfg.providers.remove(name);
            continue;
        }
        let mut provider = cfg
            .providers
            .get(name)
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or(json!({}));
        for (field, value) in state.fields {
            provider[&field] = value;
        }
        let parsed = serde_json::from_value::<config::ProviderConfig>(provider)?;
        // A conflicting new provider is incomplete until the user chooses its fields.
        if !state.conflicts.is_empty() && !cfg.providers.contains_key(name) {
            continue;
        }
        cfg.providers.insert(name.into(), parsed);
    }
    if !cfg.providers.contains_key(&cfg.default_provider) {
        cfg.default_provider.clear();
    }
    if !cfg.providers.contains_key(&cfg.provider) {
        cfg.provider.clear();
    }
    cfg.validate()?;
    // Do not rewrite while another app has edited the file since it was read.
    ensure!(
        std::fs::read(path).unwrap_or_default() == bytes,
        "configuration changed during sync; reload before applying"
    );
    let updated = toml::to_string_pretty(&cfg)?.into_bytes();
    if updated != bytes {
        config::write_atomic(path, &updated)?;
    }
    catalog.set_setting(
        "sync-provider-baseline",
        &serde_json::to_string(&cfg.providers)?,
    )?;
    Ok(())
}
fn keys(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            for cap in REFERENCE.captures_iter(s) {
                if cap[1].starts_with(PREFIX) {
                    out.insert(cap[1].into());
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                keys(v, out);
            }
        }
        Value::Object(m) => {
            for v in m.values() {
                keys(v, out);
            }
        }
        _ => {}
    }
}
pub fn export(events: &[Event], secrets: &impl Secrets) -> Result<BTreeMap<String, String>> {
    let mut refs = BTreeSet::new();
    for event in events {
        if let Some(v) = &event.value {
            keys(v, &mut refs);
        }
    }
    refs.into_iter()
        .map(|key| Ok((key.clone(), secrets.get(&key)?)))
        .collect()
}
pub fn import(values: &BTreeMap<String, String>, secrets: &impl Secrets) -> Result<()> {
    let mut new = vec![];
    for (key, value) in values {
        let suffix = key
            .strip_prefix(PREFIX)
            .context("invalid synced credential slot")?;
        ensure!(
            suffix.len() == 32 && suffix.bytes().all(|v| v.is_ascii_hexdigit()),
            "invalid synced credential slot"
        );
        match secrets.get(key) {
            Ok(old) => ensure!(
                &old == value,
                "synced credential slot changed its immutable value"
            ),
            Err(_) => new.push((key, value)),
        }
    }
    let mut written: Vec<&str> = vec![];
    for (key, value) in new {
        if let Err(error) = secrets.set(key, value) {
            for key in written {
                secrets.remove(key);
            }
            return Err(error);
        }
        written.push(key);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Memory(RefCell<BTreeMap<String, String>>);
    impl Secrets for Memory {
        fn get(&self, k: &str) -> Result<String> {
            self.0.borrow().get(k).cloned().context("missing")
        }
        fn set(&self, k: &str, v: &str) -> Result<()> {
            self.0.borrow_mut().insert(k.into(), v.into());
            Ok(())
        }
        fn remove(&self, k: &str) {
            self.0.borrow_mut().remove(k);
        }
    }
    #[test]
    fn provider_secrets_only_appear_in_explicit_wire_payloads() {
        let t = tempfile::tempdir().unwrap();
        let cfg = t.path().join("config.toml");
        std::fs::write(&cfg,"version=1\nallow_plaintext_credentials=true\n[providers.test]\ntype='s3'\nendpoint='https://s3.test'\nbucket='images'\npublic_url='https://cdn.test'\naccess_key='private-key'\nsecret_key='private-secret'\nsession_token='${AWS_SESSION_TOKEN}'\n").unwrap();
        let mut c = Catalog::open(&t.path().join("a")).unwrap();
        let secrets = Memory::default();
        let expected = capture(&mut c, &cfg, &secrets).unwrap();
        let events = c.sync_events(None).unwrap();
        let json = serde_json::to_string(&events).unwrap();
        assert!(!json.contains("private-secret"));
        assert!(json.contains("${AWS_SESSION_TOKEN}"));
        assert!(
            export(&events, &secrets)
                .unwrap()
                .values()
                .any(|v| v == "private-secret")
        );
        capture(&mut c, &cfg, &secrets).unwrap();
        assert_eq!(c.sync_events(None).unwrap().len(), events.len());
        apply(&c, &cfg, &expected, &secrets).unwrap();
        assert!(
            !std::fs::read_to_string(&cfg)
                .unwrap()
                .contains("private-secret")
        );
        capture(&mut c, &cfg, &secrets).unwrap();
        assert_eq!(c.sync_events(None).unwrap().len(), events.len());
        let received = Memory::default();
        import(&export(&events, &secrets).unwrap(), &received).unwrap();
        let mut tampered = export(&events, &secrets).unwrap();
        *tampered.values_mut().next().unwrap() = "different".into();
        assert!(import(&tampered, &received).is_err());
    }
}
