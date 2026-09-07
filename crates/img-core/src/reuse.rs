//! Content reuse is explicitly enabled and scoped to a resolved destination.
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    time::Duration,
};

pub struct Entry {
    _lock: File,
    path: PathBuf,
}
impl Entry {
    pub fn acquire(scope: &[u8], image: &[u8], control: &crate::control::Control) -> Result<Self> {
        let mut hash = Sha256::new();
        hash.update(scope);
        hash.update([0]);
        hash.update(Sha256::digest(image));
        let key = format!("{:x}", hash.finalize());
        let root = img_records::data_dir()?.join("reuse");
        std::fs::create_dir_all(&root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(format!("{key}.lock")))?;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    control.delay(Duration::from_millis(50))?
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
        Ok(Self {
            _lock: lock,
            path: root.join(format!("{key}.json")),
        })
    }
    pub fn load(&self) -> Option<crate::upload::FileResult> {
        let record: crate::upload::FileResult =
            serde_json::from_slice(&std::fs::read(&self.path).ok()?).ok()?;
        (record.success && !record.url.is_empty()).then_some(record)
    }
    pub fn save(&self, record: &crate::upload::FileResult) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
        file.write_all(&serde_json::to_vec(record)?)?;
        file.as_file().sync_all()?;
        file.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }
}
