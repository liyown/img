use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Request {
    source: PathBuf,
    config: PathBuf,
    credentials: bool,
    manifest: Vec<u8>,
}
pub fn schedule(root: &Path, source: &Path, credentials: bool, manifest: Vec<u8>) -> Result<()> {
    let request = Request {
        source: source.to_owned(),
        config: crate::storage::config_path()?,
        credentials,
        manifest,
    };
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(&serde_json::to_vec(&request)?)?;
    file.as_file().sync_all()?;
    file.persist(root.join("restore-request.json"))
        .map_err(|e| e.error)?;
    Ok(())
}
pub fn pending(root: &Path) -> bool {
    root.join("restore-ready.json").is_file()
}
pub fn commit(root: &Path) -> Result<()> {
    std::fs::rename(
        root.join("restore-request.json"),
        root.join("restore-ready.json"),
    )?;
    Ok(())
}
pub fn restart(root: &Path) -> Result<()> {
    let executable = std::env::current_exe()?;
    commit(root)?;
    let launched = std::process::Command::new(executable)
        .args(std::env::args_os().skip(1))
        .spawn();
    if let Err(error) = launched {
        // Do not leave an armed request when relaunch could not start.
        std::fs::rename(
            root.join("restore-ready.json"),
            root.join("restore-request.json"),
        )?;
        return Err(error.into());
    }
    Ok(())
}
pub fn apply_pending(root: &Path) -> Result<()> {
    if !pending(root) {
        return Ok(());
    }
    let request: Request =
        serde_json::from_slice(&std::fs::read(root.join("restore-ready.json"))?)?;
    // A restart can reach this point before the previous process releases its lock.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("session.lock"))?;
        if lock.try_lock().is_ok() {
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "等待应用退出超时，恢复尚未执行"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let result = (|| -> Result<_> {
        ensure!(
            std::fs::read(request.source.join("manifest.json"))? == request.manifest,
            "备份在确认后发生变化，恢复已取消"
        );
        img_records::backup::restore(&request.source, root, &request.config, request.credentials)
    })();
    let (message, error) = match result {
        Ok(path) => (
            format!("恢复完成。恢复前的数据保留在 {}", path.display()),
            false,
        ),
        Err(e) => (format!("恢复未完成：{e:#}"), true),
    };
    std::fs::write(
        root.join("restore-result.json"),
        serde_json::to_vec(&(message, error))?,
    )?;
    std::fs::remove_file(root.join("restore-ready.json")).context("无法清除恢复请求")?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_backup_is_rejected_before_replacing_data() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        let source = temp.path().join("backup");
        std::fs::create_dir(&source).unwrap();
        let config = temp.path().join("config.toml");
        std::fs::write(&config, b"version=1").unwrap();
        std::fs::write(source.join("manifest.json"), b"changed").unwrap();
        let request = Request {
            source,
            config: config.clone(),
            credentials: false,
            manifest: b"confirmed".to_vec(),
        };
        std::fs::write(
            root.join("restore-request.json"),
            serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        assert!(!pending(&root));
        commit(&root).unwrap();
        apply_pending(&root).unwrap();
        assert_eq!(std::fs::read(config).unwrap(), b"version=1");
        assert!(!pending(&root));
        let (_, failed): (String, bool) =
            serde_json::from_slice(&std::fs::read(root.join("restore-result.json")).unwrap())
                .unwrap();
        assert!(failed);
    }
}
