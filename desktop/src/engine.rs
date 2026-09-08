use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

pub const PAUSE: u8 = 1;
pub const CANCEL: u8 = 2;

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct TransferProgress {
    pub stage: String,
    pub sent: u64,
    pub total: i64,
    pub attempt: u32,
}

impl TransferProgress {
    pub fn percent(&self) -> Option<u8> {
        (self.total > 0).then(|| (self.sent.saturating_mul(100) / self.total as u64).min(100) as u8)
    }
    pub fn label(&self) -> String {
        match self.stage.as_str() {
            "sending" => self
                .percent()
                .map(|p| format!("{p}%"))
                .unwrap_or("传输中".into()),
            "waiting" => "等待确认".into(),
            "retrying" => format!("第 {} 次重试", self.attempt.saturating_sub(1)),
            _ => "准备中".into(),
        }
    }
}

#[derive(Clone, Default)]
pub struct Control {
    stop: Arc<AtomicU8>,
    pub finished: Arc<AtomicBool>,
    progress: Arc<Mutex<TransferProgress>>,
}
pub struct Completion(Arc<AtomicBool>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
impl Control {
    pub fn completion(&self) -> Completion {
        Completion(self.finished.clone())
    }
    pub fn child(&self) -> Self {
        Self {
            stop: self.stop.clone(),
            finished: Default::default(),
            progress: self.progress.clone(),
        }
    }

    pub fn stop(&self, reason: u8) {
        self.stop.store(reason, Ordering::SeqCst);
    }
    pub fn progress(&self) -> TransferProgress {
        self.progress.lock().unwrap().clone()
    }
}

pub struct ProcessOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stopped: u8,
}

// Own and reap the child even when its UI task is dropped. Never pipe credentials
// to the terminal; stderr is consumed only as bounded structured progress data.
pub fn run(mut command: Command, control: &Control) -> Result<ProcessOutput> {
    run_bounded(&mut command, control, 2 << 20)
}
/// Structured batch reports may include thousands of results; keep a separate explicit bound.
pub fn run_json(mut command: Command, control: &Control) -> Result<ProcessOutput> {
    run_bounded(&mut command, control, 64 << 20)
}
fn run_bounded(command: &mut Command, control: &Control, limit: u64) -> Result<ProcessOutput> {
    struct Finish(Arc<AtomicBool>);
    impl Drop for Finish {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let _finish = Finish(control.finished.clone());
    let stopped = control.stop.load(Ordering::SeqCst);
    if stopped != 0 {
        return Ok(ProcessOutput {
            success: false,
            stdout: vec![],
            stopped,
        });
    }
    if !command
        .get_envs()
        .any(|(key, _)| key == "IMG_DESKTOP_UPLOAD")
    {
        command.env("IMG_DESKTOP_UPLOAD", "1");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("上传引擎无法启动，请重新安装应用")?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = vec![];
        let _ = (&mut stdout).take(limit + 1).read_to_end(&mut bytes);
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        bytes
    });
    let progress = control.progress.clone();
    let events = std::thread::spawn(move || {
        for line in BufReader::new((&mut stderr).take(8 * 1024 * 1024))
            .lines()
            .map_while(Result::ok)
        {
            if let Ok(event) = serde_json::from_str::<TransferProgress>(&line) {
                *progress.lock().unwrap() = event;
            }
        }
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    });
    let mut stopped = 0;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
        let reason = control.stop.load(Ordering::SeqCst);
        if reason != 0 {
            if child.kill().is_ok() {
                stopped = reason;
            }
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = reader.join().unwrap_or_default();
    anyhow::ensure!(
        stdout.len() as u64 <= limit,
        "任务报告超过显示限制，请使用 CLI 导出完整报告"
    );
    let _ = events.join();
    Ok(ProcessOutput {
        success: status.success(),
        stdout,
        stopped,
    })
}

/// Complete the durable queue write and reap children before asking the framework to quit.
pub async fn wait_for_shutdown<F, Fut>(
    saved: crate::queue_store::Pending<()>,
    controls: &[Control],
    mut pause: F,
) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let result = crate::queue_store::acknowledged(saved).await;
    while controls
        .iter()
        .any(|control| !control.finished.load(Ordering::SeqCst))
    {
        pause().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interruption_reaps_process_and_reports_reason() {
        let control = Control::default();
        let other = control.clone();
        let task = std::thread::spawn(move || {
            #[cfg(unix)]
            let command = {
                let mut command = Command::new("/bin/sleep");
                command.arg("20");
                command
            };
            #[cfg(windows)]
            let command = {
                let mut command = Command::new("powershell");
                command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 20"]);
                command
            };
            run(command, &other).unwrap()
        });
        std::thread::sleep(Duration::from_millis(60));
        control.stop(PAUSE);
        assert_eq!(task.join().unwrap().stopped, PAUSE);
        assert!(control.finished.load(Ordering::SeqCst));
    }
    #[test]
    fn shutdown_waits_for_save_acknowledgement_and_child_reaping() {
        let (sender, saved) = futures_channel::oneshot::channel();
        let control = Control::default();
        let worker = control.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(230));
            sender.send(Ok(())).unwrap();
            std::thread::sleep(Duration::from_millis(40));
            worker.finished.store(true, Ordering::SeqCst);
        });
        let start = std::time::Instant::now();
        futures_lite::future::block_on(wait_for_shutdown(saved, &[control], || async {
            std::thread::sleep(Duration::from_millis(5));
        }))
        .unwrap();
        assert!(start.elapsed() >= Duration::from_millis(260));
        thread.join().unwrap();
    }
}
