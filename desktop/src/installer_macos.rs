//! Stage a verified app on the destination filesystem; hand off only after the queue is saved.
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub struct PreparedInstall {
    stage: tempfile::TempDir,
    destination: PathBuf,
    receipt: PathBuf,
    version: String,
}
fn plist(app: &Path, key: &str) -> Result<String> {
    let result = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", &format!("Print :{key}")])
        .arg(app.join("Contents/Info.plist"))
        .output()?;
    ensure!(result.status.success(), "安装包缺少应用信息");
    Ok(String::from_utf8(result.stdout)?.trim().into())
}
pub fn running_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_owned())
}
pub fn installed() -> bool {
    running_app().is_some_and(|app| {
        app.parent() == Some(Path::new("/Applications"))
            || std::env::var_os("HOME").is_some_and(|home| {
                app.parent() == Some(Path::new(&home).join("Applications").as_path())
            })
    })
}
/// Development and isolated QA bundles must not offer installation as the public app.
/// Cache metadata once; this is queried while rendering settings.
pub fn can_install_current() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        running_app().is_some_and(|app| {
            plist(&app, "CFBundleIdentifier").is_ok_and(|id| id == "dev.img.desktop")
        })
    })
}
fn destination() -> Result<PathBuf> {
    if installed() {
        return running_app().context("找不到当前应用");
    }
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("找不到用户目录")?)
            .join("Applications/Img.app"),
    )
}
fn validate(app: &Path, version: &str) -> Result<()> {
    ensure!(
        plist(app, "CFBundleIdentifier")? == "dev.img.desktop",
        "安装包不是 img 桌面应用"
    );
    ensure!(
        plist(app, "CFBundleShortVersionString")? == version,
        "安装包版本不匹配"
    );
    ensure!(
        app.join("Contents/MacOS/img").is_file(),
        "安装包缺少内置 CLI"
    );
    let mut command = Command::new("/usr/bin/codesign");
    command.args(["--verify", "--deep", "--strict"]);
    if let Some(team) = option_env!("IMG_SIGNING_TEAM") {
        ensure!(
            team.len() == 10 && team.bytes().all(|b| b.is_ascii_alphanumeric()),
            "发布团队无效"
        );
        command.args([
            "-R",
            &format!("anchor apple generic and certificate leaf[subject.OU] = \"{team}\""),
        ]);
    }
    ensure!(
        command.arg(app).output()?.status.success(),
        "应用签名校验失败"
    );
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    for binary in ["img", "img-desktop"] {
        ensure!(
            Command::new("/usr/bin/lipo")
                .arg(app.join("Contents/MacOS").join(binary))
                .args(["-verify_arch", arch])
                .output()?
                .status
                .success(),
            "安装包不支持当前芯片"
        );
    }
    Ok(())
}
fn prepare_app(
    app: &Path,
    version: &str,
    root: &Path,
    destination: PathBuf,
) -> Result<PreparedInstall> {
    validate(app, version)?;
    ensure!(!destination.is_symlink(), "安装目标不能是符号链接");
    if destination.exists() {
        ensure!(
            plist(&destination, "CFBundleIdentifier")? == "dev.img.desktop",
            "目标位置已有其他应用，不能覆盖"
        );
    }
    let parent = destination.parent().context("安装位置无效")?;
    std::fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".img-update-")
        .tempdir_in(parent)
        .context("应用目录不可写，请在 Finder 中手动安装")?;
    let next = stage.path().join("Next.app");
    ensure!(
        Command::new("/usr/bin/ditto")
            .arg(app)
            .arg(&next)
            .output()?
            .status
            .success(),
        "无法准备新应用，旧版未改变"
    );
    validate(&next, version)?;
    std::fs::write(
        stage.path().join("install.sh"),
        include_str!("../install-update.sh"),
    )?;
    Ok(PreparedInstall {
        stage,
        destination,
        receipt: root.join("install-result"),
        version: version.into(),
    })
}
pub fn prepare_current(root: &Path) -> Result<PreparedInstall> {
    prepare_app(
        &running_app().context("请从 DMG 或 ZIP 内打开 Img.app")?,
        crate::updates::CURRENT_VERSION,
        root,
        destination()?,
    )
}
pub fn prepare_update(dmg: &Path, version: &str, root: &Path) -> Result<PreparedInstall> {
    let mount = tempfile::tempdir()?;
    let output = Command::new("/usr/bin/hdiutil")
        .args(["attach", "-readonly", "-nobrowse", "-mountpoint"])
        .arg(mount.path())
        .arg(dmg)
        .output()?;
    ensure!(output.status.success(), "无法打开更新安装包");
    let result = prepare_app(&mount.path().join("Img.app"), version, root, destination()?);
    let _ = Command::new("/usr/bin/hdiutil")
        .arg("detach")
        .arg(mount.path())
        .output();
    result
}
impl PreparedInstall {
    pub fn launch(self) -> Result<()> {
        let log = std::fs::File::create(self.receipt.with_extension("log"))?;
        Command::new("/bin/sh")
            .arg(self.stage.path().join("install.sh"))
            .arg(std::process::id().to_string())
            .arg(self.stage.path())
            .arg(&self.destination)
            .arg(&self.receipt)
            .arg(&self.version)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .context("无法启动安装程序，旧版未改变")?;
        let _ = self.stage.keep();
        Ok(())
    }
}
pub fn installation_notice(root: &Path) -> Option<(String, bool)> {
    let path = root.join("install-result");
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(path);
    let mut lines = text.lines();
    let success =
        lines.next() == Some("installed") && lines.next() == Some(crate::updates::CURRENT_VERSION);
    Some(if success {
        (
            format!(
                "已安装 img {}，原有配置与图库已保留",
                crate::updates::CURRENT_VERSION
            ),
            false,
        )
    } else {
        (
            "上次安装未完成，请重新下载安装包；原有数据未清理".into(),
            true,
        )
    })
}

pub fn install_label() -> &'static str {
    "退出并安装更新"
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_replaces_after_parent_exit_and_rolls_back_on_launch_failure() {
        for fails in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let stage = root.path().join("stage with spaces");
            let destination = root.path().join("Img.app");
            let receipt = root.path().join("receipt");
            std::fs::create_dir_all(stage.join("Next.app")).unwrap();
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(stage.join("Next.app/version"), "new").unwrap();
            std::fs::write(destination.join("version"), "old").unwrap();
            let mut parent = Command::new("/bin/sleep").arg("0.2").spawn().unwrap();
            // Replace only the launch command, so no test app or user session is opened.
            let script = include_str!("../install-update.sh").replace(
                "/usr/bin/open \"$@\" \"$destination\"",
                if fails {
                    "/usr/bin/false"
                } else {
                    "/usr/bin/true"
                },
            );
            let path = root.path().join("helper.sh");
            std::fs::write(&path, script).unwrap();
            let mut helper = Command::new("/bin/sh")
                .arg(path)
                .arg(parent.id().to_string())
                .arg(&stage)
                .arg(&destination)
                .arg(&receipt)
                .arg("0.3.0")
                .spawn()
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(destination.join("version")).unwrap(),
                "old"
            );
            parent.wait().unwrap();
            assert_eq!(helper.wait().unwrap().success(), !fails);
            assert_eq!(
                std::fs::read_to_string(destination.join("version")).unwrap(),
                if fails { "old" } else { "new" }
            );
            assert!(!stage.exists());
            assert!(
                std::fs::read_to_string(receipt)
                    .unwrap()
                    .starts_with(if fails { "failed" } else { "installed" })
            );
        }
    }
    #[test]
    fn invalid_application_is_rejected_before_destination_changes() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("Applications/Img.app");
        assert!(prepare_app(root.path(), "0.3.0", root.path(), dest.clone()).is_err());
        assert!(!dest.exists());
    }
    #[test]
    fn install_receipt_requires_running_version_to_match() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("install-result"), "installed\n999.0.0\n").unwrap();
        assert!(installation_notice(root.path()).unwrap().1);
        assert!(installation_notice(root.path()).is_none());
        std::fs::write(
            root.path().join("install-result"),
            format!("installed\n{}\n", crate::updates::CURRENT_VERSION),
        )
        .unwrap();
        assert!(!installation_notice(root.path()).unwrap().1);
    }
}
