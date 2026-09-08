use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Options {
    pub config: bool,
    pub records: bool,
    pub cache: bool,
    pub credentials: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub version: u8,
    pub root: PathBuf,
    pub options: Options,
    pub files: BTreeMap<String, String>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn safe_name(name: &str) -> bool {
    !name.contains('\\')
        && !name.starts_with('/')
        && name
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != ".." && !s.contains(':'))
}
fn allowed(name: &str) -> bool {
    matches!(
        name,
        "config.toml"
            | "credentials.json"
            | "catalog.sqlite3"
            | "queue.json"
            | "preferences.json"
            | "upload-options.json"
            | "shortcuts.json"
    ) || name.starts_with("cache/")
        || name.starts_with("images/")
        || name.starts_with("upload-inbox/")
}
fn restored_cache_path(source: &str, previous_root: &Path, new_root: &Path) -> Option<PathBuf> {
    // Normalize separators from the exporting OS before using this OS's Path parser.
    let source = source.replace('\\', "/");
    let previous = previous_root.to_string_lossy().replace('\\', "/");
    let relative = source.strip_prefix(&format!("{}/", previous.trim_end_matches('/')))?;
    (safe_name(relative) && relative.starts_with("images/")).then(|| new_root.join(relative))
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("missing parent")?)?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn credentials(value: &mut toml::Value, include: bool, keys: &mut Vec<String>, sensitive: bool) {
    match value {
        toml::Value::String(s) => {
            for part in s.split("\x24{").skip(1) {
                if let Some((key, _)) = part.split_once('}')
                    && key.starts_with("IMG_DESKTOP_")
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    keys.push(key.into());
                }
            }
            if !include && sensitive {
                let mut residual = s.clone();
                while let Some(start) = residual.find("\x24{") {
                    let Some(end) = residual[start..].find('}') else {
                        break;
                    };
                    residual.replace_range(start..=start + end, "");
                }
                if !matches!(residual.trim(), "" | "Bearer" | "Basic") {
                    *s = String::new();
                }
            }
        }
        toml::Value::Table(values) => {
            for (key, value) in values {
                let key = key.to_ascii_lowercase().replace(['-', '_', ' '], "");
                let private = sensitive
                    || [
                        "secret",
                        "token",
                        "password",
                        "accesskey",
                        "apikey",
                        "authorization",
                        "headers",
                        "fields",
                    ]
                    .iter()
                    .any(|s| key.contains(s));
                credentials(value, include, keys, private);
            }
        }
        toml::Value::Array(values) => {
            for value in values {
                credentials(value, include, keys, sensitive);
            }
        }
        _ => {}
    }
}
pub fn export(
    root: &Path,
    config: &Path,
    destination: &Path,
    options: Options,
) -> Result<Manifest> {
    export_inner(root, config, destination, options, false)
}
fn export_inner(
    root: &Path,
    config: &Path,
    destination: &Path,
    options: Options,
    catalog_locked: bool,
) -> Result<Manifest> {
    ensure!(!destination.exists(), "backup destination already exists");
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    if root.exists() {
        ensure!(
            !parent.canonicalize()?.starts_with(root.canonicalize()?),
            "backup destination must be outside the application data directory"
        );
    }
    let temp = tempfile::tempdir_in(parent)?;
    let mut manifest = Manifest {
        version: 1,
        root: root.to_path_buf(),
        options,
        files: BTreeMap::new(),
    };
    let mut add = |name: &str, bytes: Vec<u8>| -> Result<()> {
        write(&temp.path().join(name), &bytes)?;
        manifest.files.insert(name.into(), digest(&bytes));
        Ok(())
    };
    if options.config && config.exists() {
        let mut value: toml::Value = toml::from_str(&std::fs::read_to_string(config)?)
            .map_err(|_| anyhow::anyhow!("invalid config; backup cancelled"))?;
        let mut keys = vec![];
        credentials(&mut value, options.credentials, &mut keys, false);
        add("config.toml", toml::to_string_pretty(&value)?.into_bytes())?;
        if options.credentials {
            let secrets = keys
                .into_iter()
                .map(|key| crate::credentials::get(&key).map(|value| (key, value)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            add("credentials.json", serde_json::to_vec(&secrets)?)?;
        }
    }
    for name in ["preferences.json", "upload-options.json", "shortcuts.json"] {
        if options.config && root.join(name).is_file() {
            add(name, std::fs::read(root.join(name))?)?;
        }
    }
    if options.records && root.join("queue.json").is_file() {
        add("queue.json", std::fs::read(root.join("queue.json"))?)?;
    }
    if options.records && root.join("catalog.sqlite3").is_file() {
        let snapshot_dir = tempfile::tempdir()?;
        let snapshot = snapshot_dir.path().join("catalog.sqlite3");
        if catalog_locked {
            rusqlite::Connection::open(root.join("catalog.sqlite3"))?
                .backup("main", &snapshot, None)?;
        } else {
            crate::catalog::Catalog::open(root)?.snapshot(&snapshot)?;
        }
        // Cache metadata is device local and cannot imply the backup contains image bytes.
        if !options.cache {
            let db = rusqlite::Connection::open(&snapshot)?;
            db.execute("DELETE FROM cache", [])?;
        }
        add("catalog.sqlite3", std::fs::read(snapshot)?)?;
    }
    let mut pending = vec![];
    if options.cache {
        pending.push(root.join("images"));
        pending.push(root.join("cache"));
    }
    if options.records {
        pending.push(root.join("upload-inbox"));
    }
    let mut count = 0;
    while let Some(path) = pending.pop() {
        if !path.exists() {
            continue;
        }
        let metadata = path.symlink_metadata()?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "backup refuses symbolic links"
        );
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else if metadata.is_file() {
            count += 1;
            ensure!(count <= 100_000, "backup exceeds 100000 files");
            let name = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            if name.starts_with("upload-inbox/.") {
                continue;
            }
            if !options.cache && name.starts_with("upload-inbox/") && name.ends_with("/image") {
                continue;
            }
            add(&name, std::fs::read(&path)?)?;
        }
    }
    write(
        &temp.path().join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    std::fs::rename(temp.path(), destination)?;
    Ok(manifest)
}
pub fn inspect(source: &Path) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(source.join("manifest.json"))?)?;
    ensure!(manifest.version == 1, "unsupported backup version");
    ensure!(manifest.files.len() <= 100_000, "backup exceeds file limit");
    for (name, expected) in &manifest.files {
        ensure!(
            safe_name(name) && allowed(name),
            "backup contains an invalid path"
        );
        let mut cursor = source.to_owned();
        for part in name.split('/') {
            cursor.push(part);
            ensure!(
                !cursor.symlink_metadata()?.file_type().is_symlink(),
                "backup contains a symbolic link"
            );
        }
        ensure!(
            digest(&std::fs::read(cursor)?) == *expected,
            "backup checksum mismatch"
        );
    }
    if manifest.files.contains_key("catalog.sqlite3") {
        let staging = tempfile::tempdir()?;
        std::fs::copy(
            source.join("catalog.sqlite3"),
            staging.path().join("catalog.sqlite3"),
        )?;
        let catalog = crate::catalog::Catalog::open(staging.path())?;
        let integrity: String = catalog
            .db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        ensure!(integrity == "ok", "backup library database is damaged");
    }
    Ok(manifest)
}
pub fn restore(
    source: &Path,
    root: &Path,
    config: &Path,
    include_credentials: bool,
) -> Result<PathBuf> {
    let manifest = inspect(source)?;
    std::fs::create_dir_all(root)?;
    let canonical_root = root.canonicalize()?;
    let root = canonical_root.as_path();
    let parent = config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let canonical_config = parent
        .canonicalize()?
        .join(config.file_name().context("invalid config filename")?);
    let config = canonical_config.as_path();
    ensure!(
        !source.canonicalize()?.starts_with(root.canonicalize()?),
        "restore source must be outside the data directory"
    );
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("session.lock"))?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("close img before restoring its data"))?;
    let catalog_lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("catalog.lock"))?;
    catalog_lock
        .try_lock()
        .map_err(|_| anyhow::anyhow!("close library operations before restoring"))?;
    if root.join("catalog.sqlite3").exists() {
        let db = rusqlite::Connection::open(root.join("catalog.sqlite3"))?;
        let busy: i64 = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        ensure!(busy == 0, "library has an active writer; restore cancelled");
    }
    let rollback = root
        .parent()
        .unwrap()
        .join(format!("img-before-restore-{}", uuid::Uuid::new_v4()));
    export_inner(
        root,
        config,
        &rollback,
        Options {
            config: true,
            records: true,
            cache: true,
            credentials: include_credentials,
        },
        true,
    )?;
    // Keep exact pre-restore bytes privately for rollback, including plaintext
    // credentials in legacy configurations. This directory is never uploaded.
    if config.exists() {
        let original = std::fs::read(config)?;
        write(&rollback.join("config.toml"), &original)?;
        let mut old: Manifest =
            serde_json::from_slice(&std::fs::read(rollback.join("manifest.json"))?)?;
        let mut sanitized: toml::Value = toml::from_str(std::str::from_utf8(&original)?)?;
        let before = sanitized.clone();
        credentials(&mut sanitized, false, &mut vec![], false);
        old.options.credentials |= sanitized != before;
        old.files.insert("config.toml".into(), digest(&original));
        write(
            &rollback.join("manifest.json"),
            &serde_json::to_vec_pretty(&old)?,
        )?;
    }
    let transaction = tempfile::tempdir_in(root)?;
    let mut old_files = vec![];
    let mut old_keys = vec![];
    let result = (|| -> Result<()> {
        for name in manifest
            .files
            .keys()
            .filter(|n| n.as_str() != "credentials.json")
        {
            let target = if name == "config.toml" {
                config.to_path_buf()
            } else {
                root.join(name)
            };
            let mut cursor = target.parent();
            while let Some(path) = cursor {
                if path.exists() {
                    ensure!(
                        !path.symlink_metadata()?.file_type().is_symlink(),
                        "restore destination contains a symbolic link"
                    );
                }
                cursor = path.parent();
            }
            if target.exists() {
                ensure!(
                    !target.symlink_metadata()?.file_type().is_symlink(),
                    "restore target is a symbolic link"
                );
            }
            let previous = if target.exists() {
                let path = transaction.path().join(old_files.len().to_string());
                std::fs::copy(&target, &path)?;
                Some(path)
            } else {
                None
            };
            old_files.push((target.clone(), previous));
            let mut bytes = std::fs::read(source.join(name))?;
            if name == "config.toml" && !include_credentials {
                let mut value: toml::Value = toml::from_str(std::str::from_utf8(&bytes)?)?;
                credentials(&mut value, false, &mut vec![], false);
                bytes = toml::to_string_pretty(&value)?.into_bytes();
            }
            if name == "queue.json" {
                let mut rows: Vec<serde_json::Value> = serde_json::from_slice(&bytes)?;
                for row in &mut rows {
                    for key in ["source", "thumbnail"] {
                        row[key] = row[key]
                            .as_str()
                            .filter(|_| manifest.options.cache)
                            .and_then(|s| restored_cache_path(s, &manifest.root, root))
                            .map(|p| serde_json::Value::String(p.to_string_lossy().into_owned()))
                            .unwrap_or(serde_json::Value::Null);
                    }
                    if row["status"] == "Running" {
                        row["status"] = "Paused".into();
                    }
                }
                bytes = serde_json::to_vec(&rows)?;
            }
            if name == "catalog.sqlite3" {
                let staged = transaction.path().join("restored-catalog.sqlite3");
                write(&staged, &bytes)?;
                {
                    let db = rusqlite::Connection::open(&staged)?;
                    db.execute(
                        "UPDATE settings SET value=? WHERE key='sync-device'",
                        [uuid::Uuid::new_v4().to_string()],
                    )?;
                    db.execute("DELETE FROM settings WHERE key='sync-applying'", [])?;
                }
                bytes = std::fs::read(staged)?;
            }
            write(&target, &bytes)?;
        }
        if include_credentials && manifest.files.contains_key("credentials.json") {
            let values: BTreeMap<String, Vec<u8>> =
                serde_json::from_slice(&std::fs::read(source.join("credentials.json"))?)?;
            for (key, value) in values {
                ensure!(
                    key.starts_with("IMG_DESKTOP_")
                        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "invalid credential key"
                );
                old_keys.push((key.clone(), crate::credentials::get(&key).ok()));
                crate::credentials::set(&key, &value)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = 0;
        for (target, previous) in old_files.into_iter().rev() {
            if let Some(previous) = previous {
                if std::fs::read(previous)
                    .and_then(|bytes| write(&target, &bytes).map_err(std::io::Error::other))
                    .is_err()
                {
                    failures += 1;
                }
            } else if target.exists() && std::fs::remove_file(target).is_err() {
                failures += 1;
            }
        }
        for (key, value) in old_keys {
            if let Some(value) = value {
                if crate::credentials::set(&key, &value).is_err() {
                    failures += 1;
                }
            } else {
                crate::credentials::remove(&key);
            }
        }
        return Err(error.context(format!(
            "restore failed ({failures} rollback errors); recovery copy retained at {}",
            rollback.display()
        )));
    }
    Ok(rollback)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_snapshot_restore_preserves_metadata_without_cache() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("data");
        let config = t.path().join("config.toml");
        let c = crate::catalog::Catalog::open(&root).unwrap();
        c.set_setting("acceptance", "before").unwrap();
        let cache = crate::cache::Cache::open(&root).unwrap();
        cache.put(b"private pixels").unwrap();
        drop(cache);
        let destination = t.path().join("backup");
        let manifest = export(
            &root,
            &config,
            &destination,
            Options {
                config: false,
                records: true,
                cache: false,
                credentials: false,
            },
        )
        .unwrap();
        assert!(manifest.files.contains_key("catalog.sqlite3"));
        assert!(!manifest.files.keys().any(|name| name.starts_with("cache/")));
        c.set_setting("acceptance", "after").unwrap();
        assert!(restore(&destination, &root, &config, false).is_err());
        drop(c);
        restore(&destination, &root, &config, false).unwrap();
        let c = crate::catalog::Catalog::open(&root).unwrap();
        assert_eq!(c.setting("acceptance").unwrap().as_deref(), Some("before"));
        assert_eq!(
            c.db.query_row("SELECT count(*) FROM cache", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn cache_paths_restore_across_operating_systems_without_path_escape() {
        let root = Path::new("/new");
        assert_eq!(
            restored_cache_path(r"C:\old\images\id\a.png", Path::new(r"C:\old"), root),
            Some(root.join("images/id/a.png"))
        );
        assert!(
            restored_cache_path(r"C:\old\images\..\secret", Path::new(r"C:\old"), root).is_none()
        );
        assert!(restored_cache_path("/original/photo.png", Path::new("/old"), root).is_none());
    }
    #[test]
    fn record_only_backup_excludes_images_and_mixed_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        write(&root.join("upload-inbox/id/record.json"), b"{}").unwrap();
        write(&root.join("upload-inbox/id/image"), b"private image").unwrap();
        let config = temp.path().join("config.toml");
        std::fs::write(&config,"version=1\n[providers.x]\ntoken='private-prefix-\x24{KEY}'\n[providers.x.headers]\nAuthorization='Bearer \x24{KEY}'").unwrap();
        let destination = temp.path().join("backup");
        let manifest = export(
            &root,
            &config,
            &destination,
            Options {
                config: true,
                records: true,
                cache: false,
                credentials: false,
            },
        )
        .unwrap();
        assert!(manifest.files.contains_key("upload-inbox/id/record.json"));
        assert!(!manifest.files.contains_key("upload-inbox/id/image"));
        let text = std::fs::read_to_string(destination.join("config.toml")).unwrap();
        assert!(!text.contains("private-prefix"));
        assert!(text.contains("Bearer"));
    }
    #[test]
    fn apply_failure_rolls_back_already_replaced_config() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("source");
        write(&source_root.join("queue.json"), b"invalid queue").unwrap();
        let config = temp.path().join("config.toml");
        std::fs::write(&config, "version=1\n").unwrap();
        let backup = temp.path().join("backup");
        export(
            &source_root,
            &config,
            &backup,
            Options {
                config: true,
                records: true,
                cache: false,
                credentials: false,
            },
        )
        .unwrap();
        let target = temp.path().join("target");
        write(&target.join("queue.json"), b"[]").unwrap();
        let before = b"version=1\n[providers.old]\ntoken='preserve-exactly'";
        std::fs::write(&config, before).unwrap();
        assert!(restore(&backup, &target, &config, false).is_err());
        assert_eq!(std::fs::read(config).unwrap(), before);
        assert_eq!(std::fs::read(target.join("queue.json")).unwrap(), b"[]");
    }
    #[test]
    fn roundtrip_rebases_paths_and_tampering_never_changes_target() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("old");
        std::fs::create_dir_all(&root).unwrap();
        let config = temp.path().join("config.toml");
        std::fs::write(&config, "version=1").unwrap();
        write(&root.join("images/id/image.png"), b"original bytes").unwrap();
        write(
            &root.join("queue.json"),
            serde_json::to_string(
                &serde_json::json!([{"status":"Done","source":root.join("images/id/image.png")}]),
            )
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
        let backup = temp.path().join("backup");
        export(
            &root,
            &config,
            &backup,
            Options {
                config: true,
                records: true,
                cache: true,
                credentials: false,
            },
        )
        .unwrap();
        let target = temp.path().join("new");
        restore(
            &backup,
            &target,
            &temp.path().join("new-config.toml"),
            false,
        )
        .unwrap();
        let rows: serde_json::Value =
            serde_json::from_slice(&std::fs::read(target.join("queue.json")).unwrap()).unwrap();
        assert_eq!(
            rows[0]["source"],
            target
                .canonicalize()
                .unwrap()
                .join("images/id/image.png")
                .to_string_lossy()
                .as_ref()
        );
        write(&backup.join("queue.json"), b"tampered").unwrap();
        assert!(restore(&backup, &target, &config, false).is_err());
        assert_eq!(
            std::fs::read(root.join("images/id/image.png")).unwrap(),
            b"original bytes"
        );
    }
    #[test]
    fn secrets_are_opt_in_and_live_session_blocks_restore() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        let config = temp.path().join("config.toml");
        std::fs::write(&config, "version=1\n[providers.x]\ntoken='do-not-export'").unwrap();
        let backup = temp.path().join("backup");
        export(
            &root,
            &config,
            &backup,
            Options {
                config: true,
                records: false,
                cache: false,
                credentials: false,
            },
        )
        .unwrap();
        assert!(
            !std::fs::read_to_string(backup.join("config.toml"))
                .unwrap()
                .contains("do-not-export")
        );
        let lock = std::fs::File::create(root.join("session.lock")).unwrap();
        lock.lock().unwrap();
        assert!(restore(&backup, &root, &config, false).is_err());
        assert!(
            std::fs::read_to_string(config)
                .unwrap()
                .contains("do-not-export")
        );
    }
}
