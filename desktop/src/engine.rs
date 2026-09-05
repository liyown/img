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
impl Control {
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
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("上传引擎无法启动，请重新安装应用")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = vec![];
        let _ = stdout.take(2 * 1024 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let progress = control.progress.clone();
    let events = std::thread::spawn(move || {
        for line in BufReader::new(stderr.take(8 * 1024 * 1024))
            .lines()
            .map_while(Result::ok)
        {
            if let Ok(event) = serde_json::from_str::<TransferProgress>(&line) {
                *progress.lock().unwrap() = event;
            }
        }
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
    let _ = events.join();
    Ok(ProcessOutput {
        success: status.success(),
        stdout,
        stopped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interruption_reaps_process_and_reports_reason() {
        let control = Control::default();
        let other = control.clone();
        let task = std::thread::spawn(move || {
            let mut command = Command::new("/bin/sleep");
            command.arg("20");
            run(command, &other).unwrap()
        });
        std::thread::sleep(Duration::from_millis(60));
        control.stop(PAUSE);
        assert_eq!(task.join().unwrap().stopped, PAUSE);
        assert!(control.finished.load(Ordering::SeqCst));
    }
}
