use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub stage: String,
    pub sent: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub attempt: u32,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
type Reporter = Arc<dyn Fn(Progress) + Send + Sync>;
#[derive(Clone, Default)]
pub struct Control {
    cancelled: Arc<AtomicBool>,
    reporter: Option<Reporter>,
}
impl Control {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> Result<()> {
        ensure!(!self.is_cancelled(), "upload cancelled");
        Ok(())
    }
    pub fn with_reporter(mut self, report: impl Fn(Progress) + Send + Sync + 'static) -> Self {
        self.reporter = Some(Arc::new(report));
        self
    }
    pub fn report(&self, p: Progress) {
        if let Some(r) = &self.reporter {
            r(p);
        }
    }
    pub fn stage(&self, stage: &str, attempt: u32) {
        self.report(Progress {
            stage: stage.into(),
            attempt,
            ..Default::default()
        });
    }
    pub fn delay(&self, duration: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        while start.elapsed() < duration {
            self.check()?;
            std::thread::sleep(
                Duration::from_millis(25).min(duration.saturating_sub(start.elapsed())),
            );
        }
        self.check()
    }
}
pub struct ProgressReader {
    data: Cursor<Arc<[u8]>>,
    control: Control,
}
impl ProgressReader {
    pub fn new(data: Arc<[u8]>, control: &Control) -> Self {
        control.report(Progress {
            stage: "sending".into(),
            total: data.len() as u64,
            ..Default::default()
        });
        Self {
            data: Cursor::new(data),
            control: control.clone(),
        }
    }
}
impl Read for ProgressReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.control.is_cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "upload cancelled",
            ));
        }
        let n = self.data.read(out)?;
        let sent = self.data.position();
        let total = self.data.get_ref().len() as u64;
        self.control.report(Progress {
            stage: if sent == total { "waiting" } else { "sending" }.into(),
            sent,
            total,
            attempt: 0,
        });
        Ok(n)
    }
}
