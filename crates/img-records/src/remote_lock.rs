//! Cross-process protection for an object while it is being uploaded or removed.
use anyhow::Result;
use std::{fs::File, path::Path};
pub fn acquire(root: &Path, namespace: &str, path: &str, exclusive: bool) -> Result<File> {
    let directory = root.join("remote-locks");
    std::fs::create_dir_all(&directory)?;
    let id = crate::catalog::identity(&[namespace, path]);
    let file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(id))?;
    if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    }
    .map_err(|_| {
        anyhow::anyhow!("remote object is protected by an active upload or delete task")
    })?;
    Ok(file)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upload_protects_only_its_object() {
        let t = tempfile::tempdir().unwrap();
        let upload = acquire(t.path(), "source", "a.png", false).unwrap();
        assert!(acquire(t.path(), "source", "a.png", true).is_err());
        assert!(acquire(t.path(), "source", "b.png", true).is_ok());
        drop(upload);
        assert!(acquire(t.path(), "source", "a.png", true).is_ok());
    }
}
