//! A single owner for queue writes. Enqueue synchronously, await acknowledgements off the UI thread.
use crate::model::Item;
use anyhow::{Context, Result, ensure};
use futures_channel::oneshot;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

type Reply<T> = oneshot::Sender<Result<T>>;
pub type Pending<T> = oneshot::Receiver<Result<T>>;
enum Request {
    Save(u64, Vec<Item>, Reply<()>),
    Barrier(Reply<()>),
    Recover(bool, Reply<Vec<Item>>),
}
struct Sender {
    next: u64,
    tx: mpsc::Sender<Request>,
}
#[derive(Clone)]
pub struct QueueStore(Arc<Mutex<Sender>>);
impl QueueStore {
    pub fn new(root: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut last = 0;
            let primary = root.join("queue.json");
            let mut blocked = primary.exists() && valid(&primary).is_err();
            for request in rx {
                match request {
                    Request::Save(revision, items, reply) => {
                        let result = (|| {
                            ensure!(!blocked, "队列保存已停止，请先恢复或重试保存");
                            ensure!(revision > last, "拒绝过期队列快照");
                            save(&root, &items)?;
                            last = revision;
                            Ok(())
                        })();
                        blocked |= result.is_err();
                        let _ = reply.send(result);
                    }
                    Request::Barrier(reply) => {
                        let _ = reply.send(if blocked {
                            Err(anyhow::anyhow!("队列未安全保存，上传已暂停"))
                        } else {
                            Ok(())
                        });
                    }
                    Request::Recover(empty, reply) => {
                        let result = recover(&root, empty);
                        if result.is_ok() {
                            blocked = false;
                        }
                        let _ = reply.send(result);
                    }
                }
            }
        });
        Self(Arc::new(Mutex::new(Sender { next: 0, tx })))
    }
    pub fn save(&self, items: Vec<Item>) -> Pending<()> {
        let (tx, rx) = oneshot::channel();
        let mut sender = self.0.lock().unwrap();
        sender.next += 1;
        let revision = sender.next;
        let _ = sender.tx.send(Request::Save(revision, items, tx));
        rx
    }
    pub fn barrier(&self) -> Pending<()> {
        let (tx, rx) = oneshot::channel();
        let _ = self.0.lock().unwrap().tx.send(Request::Barrier(tx));
        rx
    }
    pub fn recover(&self, empty: bool) -> Pending<Vec<Item>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.0.lock().unwrap().tx.send(Request::Recover(empty, tx));
        rx
    }
}
pub async fn acknowledged<T>(pending: Pending<T>) -> Result<T> {
    pending.await.context("队列保存线程已停止")?
}
fn valid(path: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice::<Vec<Item>>(&bytes).context("队列文件损坏")?;
    Ok(bytes)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let root = path.parent().context("队列目录不可用")?;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(root)?.sync_all()?;
    Ok(())
}
fn save(root: &Path, items: &[Item]) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let path = root.join("queue.json");
    if path.exists() {
        let previous = valid(&path)?;
        if let Ok(older) = valid(&root.join("queue.backup-1.json")) {
            atomic(&root.join("queue.backup-2.json"), &older)?;
        }
        atomic(&root.join("queue.backup-1.json"), &previous)?;
    }
    let items: Vec<_> = items.iter().filter(|i| !i.simulated).collect();
    atomic(&path, &serde_json::to_vec(&items)?)
}
fn recover(root: &Path, empty: bool) -> Result<Vec<Item>> {
    std::fs::create_dir_all(root)?;
    let mut items = if empty {
        vec![]
    } else {
        let bytes = ["queue.backup-1.json", "queue.backup-2.json"]
            .iter()
            .find_map(|name| valid(&root.join(name)).ok())
            .context("没有有效备份；可以保留损坏文件并重建空队列")?;
        serde_json::from_slice::<Vec<Item>>(&bytes)?
    };
    for item in &mut items {
        if item.status == crate::model::Status::Running {
            item.status = crate::model::Status::Paused;
            item.progress = None;
        }
    }
    let path = root.join("queue.json");
    if path.exists() {
        // Preserve the exact damaged bytes before replacing the primary, even for an empty reset.
        let old = std::fs::read(&path)?;
        atomic(
            &root.join(format!("queue-preserved-{}.json", uuid::Uuid::new_v4())),
            &old,
        )?;
    }
    atomic(&path, &serde_json::to_vec(&items)?)?;
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait<T>(p: Pending<T>) -> Result<T> {
        futures_lite::future::block_on(acknowledged(p))
    }
    fn item(name: &str) -> Item {
        let mut item = Item::reference_items().remove(0);
        item.name = name.into();
        item.simulated = false;
        item
    }
    #[test]
    fn serial_writes_barrier_and_two_valid_backups() {
        let root = tempfile::tempdir().unwrap();
        let store = QueueStore::new(root.path().into());
        let first = store.save(vec![item("first")]);
        let second = store.save(vec![item("second")]);
        let third = store.save(vec![item("third")]);
        wait(store.barrier()).unwrap();
        for p in [first, second, third] {
            wait(p).unwrap();
        }
        assert_eq!(crate::model::load(root.path()).unwrap()[0].name, "third");
        assert!(
            String::from_utf8(valid(&root.path().join("queue.backup-2.json")).unwrap())
                .unwrap()
                .contains("first")
        );
    }
    #[test]
    fn damaged_primary_blocks_upload_until_explicit_recovery_and_preserves_bytes() {
        let root = tempfile::tempdir().unwrap();
        let store = QueueStore::new(root.path().into());
        wait(store.save(vec![item("recover me")])).unwrap();
        wait(store.save(vec![item("new")])).unwrap();
        std::fs::write(root.path().join("queue.json"), b"damaged-private-data").unwrap();
        assert!(wait(store.save(vec![])).is_err());
        assert!(wait(store.barrier()).is_err());
        let recovered = wait(store.recover(false)).unwrap();
        assert_eq!(recovered[0].name, "recover me");
        assert_eq!(recovered[0].status, crate::model::Status::Paused);
        wait(store.barrier()).unwrap();
        assert!(std::fs::read_dir(root.path()).unwrap().flatten().any(|p| {
            p.file_name()
                .to_string_lossy()
                .starts_with("queue-preserved-")
                && std::fs::read(p.path()).unwrap() == b"damaged-private-data"
        }));
    }
    #[test]
    fn reset_requires_preservation_and_write_failure_blocks_barrier() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("queue.json"), b"bad").unwrap();
        let store = QueueStore::new(root.path().into());
        assert!(wait(store.recover(false)).is_err());
        assert!(wait(store.recover(true)).unwrap().is_empty());
        std::fs::create_dir(root.path().join("queue.backup-1.json")).unwrap();
        assert!(wait(store.save(vec![item("not saved")])).is_err());
        assert!(wait(store.barrier()).is_err());
    }
}
