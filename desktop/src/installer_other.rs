use anyhow::{Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub struct PreparedInstall {
    package: PathBuf,
}
pub fn can_install_current() -> bool {
    false
}
pub fn installed() -> bool {
    true
}
pub fn prepare_current(_: &Path) -> Result<PreparedInstall> {
    anyhow::bail!("请使用本系统的安装包安装应用")
}
pub fn installation_notice(_: &Path) -> Option<(String, bool)> {
    None
}
pub fn install_label() -> &'static str {
    if cfg!(windows) {
        "退出并运行更新安装器"
    } else {
        "退出并打开系统安装器"
    }
}
pub fn prepare_update(package: &Path, version: &str, _: &Path) -> Result<PreparedInstall> {
    ensure!(
        semver::Version::parse(version)? > semver::Version::parse(crate::updates::CURRENT_VERSION)?,
        "更新版本必须高于当前版本"
    );
    let extension = if cfg!(windows) { "exe" } else { "deb" };
    ensure!(
        package.is_file() && package.extension().is_some_and(|ext| ext == extension),
        "更新包格式不匹配"
    );
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("dpkg-deb")
            .arg("-f")
            .arg(package)
            .args(["Package", "Version", "Architecture"])
            .output()?;
        ensure!(output.status.success(), "无法读取 DEB 安装包");
        let metadata = String::from_utf8(output.stdout)?;
        ensure!(
            metadata.lines().any(|line| line == "Package: img-desktop")
                && metadata
                    .lines()
                    .any(|line| line == format!("Version: {version}"))
                && metadata.lines().any(|line| line == "Architecture: amd64"),
            "DEB 应用、版本或架构不匹配"
        );
    }
    Ok(PreparedInstall {
        package: package.to_owned(),
    })
}
impl PreparedInstall {
    pub fn launch(self) -> Result<()> {
        #[cfg(windows)]
        {
            Command::new(&self.package)
                .args(["/CLOSEAPPLICATIONS", "/RESTARTAPPLICATIONS"])
                .stdin(Stdio::null())
                .spawn()?;
        }
        #[cfg(target_os = "linux")]
        {
            Command::new("/bin/sh")
                .args([
                    "-c",
                    r#"pkexec /usr/bin/apt-get install --yes "$1" && gtk-launch img"#,
                    "img-update",
                ])
                .arg(&self.package)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        }
        Ok(())
    }
}
