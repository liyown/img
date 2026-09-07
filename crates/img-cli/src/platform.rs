use anyhow::{Context, Result, bail, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn clipboard(text: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let candidates: Vec<(&str, Vec<&str>)> = vec![("/usr/bin/pbcopy", vec![])];
    #[cfg(target_os = "linux")]
    let candidates: Vec<(&str, Vec<&str>)> = vec![
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ];
    #[cfg(target_os = "windows")]
    let candidates: Vec<(&str, Vec<&str>)> = vec![(
        "powershell",
        vec![
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::InputEncoding = [Text.UTF8Encoding]::new(); Set-Clipboard -Value ([Console]::In.ReadToEnd())",
        ],
    )];
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let candidates: Vec<(&str, Vec<&str>)> = vec![];
    for (bin, args) in candidates {
        let mut child = match Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let write = child.stdin.take().unwrap().write_all(text.as_bytes());
        let status = child.wait()?;
        if write.is_ok() && status.success() {
            return Ok(());
        }
    }
    bail!("no working clipboard tool is available")
}
pub fn screenshot(region: bool, window: bool) -> Result<tempfile::NamedTempFile> {
    let file = tempfile::Builder::new()
        .prefix("img-screenshot-")
        .suffix(".png")
        .tempfile()?;
    capture(file.path(), region, window)?;
    ensure!(
        file.as_file().metadata()?.len() > 0,
        "screenshot cancelled or produced an empty file"
    );
    Ok(file)
}
#[cfg(target_os = "macos")]
fn capture(path: &Path, region: bool, window: bool) -> Result<()> {
    let mut cmd = Command::new("/usr/sbin/screencapture");
    cmd.arg("-x");
    if region {
        cmd.arg("-i");
    }
    if window {
        cmd.arg("-w");
    }
    ensure!(
        cmd.arg(path).status()?.success(),
        "screenshot failed or was cancelled"
    );
    Ok(())
}
#[cfg(target_os = "linux")]
fn capture(path: &Path, region: bool, window: bool) -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let uri = runtime.block_on(async {
            Ok::<_, anyhow::Error>(
                ashpd::desktop::screenshot::Screenshot::request()
                    .interactive(region || window)
                    .modal(true)
                    .send()
                    .await?
                    .response()?
                    .uri()
                    .to_string(),
            )
        })?;
        let source = url::Url::parse(&uri)?
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("portal returned a non-local screenshot"))?;
        std::fs::copy(source, path)?;
        return Ok(());
    }
    let mut candidates = vec![];
    if !window {
        candidates.push((
            "flameshot",
            if region {
                vec!["gui", "--path"]
            } else {
                vec!["full", "--path"]
            },
        ));
    }
    candidates.push((
        "scrot",
        if region {
            vec!["-s"]
        } else if window {
            vec!["-u"]
        } else {
            vec![]
        },
    ));
    candidates.push((
        "gnome-screenshot",
        if region {
            vec!["-a", "-f"]
        } else if window {
            vec!["-w", "-f"]
        } else {
            vec!["-f"]
        },
    ));
    candidates.push((
        "import",
        if region || window {
            vec![]
        } else {
            vec!["-window", "root"]
        },
    ));
    for (bin, args) in candidates {
        match Command::new(bin).args(args).arg(path).status() {
            Ok(s) => {
                ensure!(s.success(), "screenshot failed or was cancelled");
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("install flameshot, scrot, gnome-screenshot or ImageMagick for screenshots")
}
#[cfg(target_os = "windows")]
fn capture(path: &Path, region: bool, window: bool) -> Result<()> {
    let script = include_str!("capture-windows.ps1");
    ensure!(
        Command::new("powershell")
            .args(["-NoProfile", "-STA", "-NonInteractive", "-Command", script])
            .env("IMG_CAPTURE_PATH", path)
            .env(
                "IMG_CAPTURE_MODE",
                if region {
                    "region"
                } else if window {
                    "window"
                } else {
                    "screen"
                }
            )
            .status()?
            .success(),
        "screenshot failed"
    );
    Ok(())
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn capture(_: &Path, _: bool, _: bool) -> Result<()> {
    bail!("screenshots unsupported on this platform")
}

pub fn install_cli(dir: Option<&Path>) -> Result<(PathBuf, bool)> {
    let directory = match dir {
        Some(d) => d.to_path_buf(),
        None => {
            #[cfg(windows)]
            {
                PathBuf::from(
                    std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?,
                )
                .join("Programs/img")
            }
            #[cfg(not(windows))]
            {
                PathBuf::from(std::env::var_os("HOME").context("home directory is unavailable")?)
                    .join(".local/bin")
            }
        }
    };
    std::fs::create_dir_all(&directory)?;
    let directory = directory.canonicalize()?;
    let source = std::env::current_exe()?.canonicalize()?;
    let destination = directory.join(if cfg!(windows) { "img.exe" } else { "img" });
    let on_path = std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| d.canonicalize().ok().as_ref() == Some(&directory))
    });
    if destination.canonicalize().ok().as_ref() == Some(&source) {
        return Ok((destination, on_path));
    }
    ensure!(
        destination.symlink_metadata().is_err(),
        "{} already exists; keep the existing CLI or choose another --dir",
        destination.display()
    );
    #[cfg(unix)]
    {
        if source
            .ancestors()
            .any(|p| p.extension().is_some_and(|s| s == "app"))
        {
            std::os::unix::fs::symlink(&source, &destination)?;
            return Ok((destination, on_path));
        }
    }
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    std::io::copy(&mut std::fs::File::open(&source)?, &mut file)?;
    file.as_file()
        .set_permissions(source.metadata()?.permissions())?;
    file.as_file().sync_all()?;
    file.persist_noclobber(&destination).map_err(|e| e.error)?;
    Ok((destination, on_path))
}
