use anyhow::{Context, Result, ensure};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = "https://github.com/liyown/img";
pub const RELEASES_PAGE: &str = "https://github.com/liyown/img/releases";
const RELEASES_FEED: &str = "https://github.com/liyown/img/releases.atom";
const RELEASES_API: &str = "https://api.github.com/repos/liyown/img/releases?per_page=30";
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

#[derive(Clone)]
pub struct Update {
    pub version: String,
    pub page: String,
    pub asset: String,
    pub size: u64,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}
fn architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

fn select_release(bytes: &[u8], current: &str, arch: &str) -> Result<Option<Update>> {
    let releases: Vec<Release> =
        serde_json::from_slice(bytes).context("更新服务返回了无法读取的信息")?;
    let current = Version::parse(current)?;
    let mut available = vec![];
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        let Some(version) = release
            .tag_name
            .strip_prefix("desktop-v")
            .and_then(|s| Version::parse(s).ok())
        else {
            continue;
        };
        if version <= current || !version.pre.is_empty() {
            continue;
        }
        let name = format!("img-desktop_{version}_macos_{arch}.dmg");
        let base = format!("{REPOSITORY}/releases/download/desktop-v{version}");
        if let Some(asset) = release.assets.iter().find(|a| {
            a.name == name
                && a.size > 0
                && a.size <= MAX_DOWNLOAD
                && a.browser_download_url == format!("{base}/{name}")
        }) {
            if !release.assets.iter().any(|a| {
                a.name == format!("{name}.sha256")
                    && a.size <= 1024
                    && a.browser_download_url == format!("{base}/{name}.sha256")
            }) {
                continue;
            }
            available.push((
                version.clone(),
                Update {
                    version: version.to_string(),
                    page: format!("{REPOSITORY}/releases/tag/desktop-v{version}"),
                    asset: name,
                    size: asset.size,
                },
            ));
        }
    }
    available.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(available.pop().map(|(_, update)| update))
}

fn curl(url: &str, timeout: &str) -> Command {
    let mut command = Command::new("/usr/bin/curl");
    command.args([
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "15",
        "--max-time",
        timeout,
        "--user-agent",
        concat!("img-desktop/", env!("CARGO_PKG_VERSION")),
        url,
    ]);
    command
}

fn select_feed(bytes: &[u8], current: &str) -> Result<Option<Update>> {
    let current = Version::parse(current)?;
    let prefix = format!("{REPOSITORY}/releases/tag/desktop-v");
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut versions = vec![];
    loop {
        match reader.read_event() {
            Ok(
                quick_xml::events::Event::Empty(element) | quick_xml::events::Event::Start(element),
            ) if element.local_name().as_ref() == b"link" => {
                for attribute in element.attributes().flatten() {
                    if attribute.key.as_ref() != b"href" {
                        continue;
                    }
                    let value = attribute.decoded_and_normalized_value(
                        quick_xml::XmlVersion::Implicit1_0,
                        reader.decoder(),
                    )?;
                    if let Some(version) = value
                        .strip_prefix(&prefix)
                        .and_then(|v| Version::parse(v).ok())
                    {
                        if version > current && version.pre.is_empty() {
                            versions.push(version);
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => anyhow::bail!("无法读取版本页面，请稍后重试"),
            _ => {}
        }
    }
    versions.sort();
    Ok(versions.pop().map(|version| Update {
        version: version.to_string(),
        page: format!("{prefix}{version}"),
        asset: String::new(),
        size: 0,
    }))
}

pub fn check() -> Result<Option<Update>> {
    let mut command = curl(RELEASES_API, "30");
    command.args(["--max-filesize", "2097152"]);
    let result = crate::engine::run(command, &crate::engine::Control::default())?;
    if result.success {
        return select_release(&result.stdout, CURRENT_VERSION, architecture());
    }
    // Anonymous API quota can be shared with other apps on the same network.
    // The public release feed provides a read-only fallback without credentials.
    // Without verified asset metadata, offer the release page instead of a download.
    let mut command = curl(RELEASES_FEED, "30");
    command.args(["--max-filesize", "2097152"]);
    let result = crate::engine::run(command, &crate::engine::Control::default())?;
    ensure!(result.success, "暂时无法检查更新，请稍后重试或查看版本页面");
    select_feed(&result.stdout, CURRENT_VERSION)
}

fn verify_checksum(path: &Path, text: &str, name: &str) -> Result<()> {
    let fields: Vec<_> = text.split_whitespace().collect();
    ensure!(
        fields.len() == 2
            && fields[1].trim_start_matches('*') == name
            && fields[0].len() == 64
            && fields[0].bytes().all(|b| b.is_ascii_hexdigit()),
        "更新校验信息无效"
    );
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    ensure!(
        format!("{:x}", hash.finalize()).eq_ignore_ascii_case(fields[0]),
        "安装包校验失败，下载文件已丢弃"
    );
    Ok(())
}

pub fn download(update: &Update, root: &Path) -> Result<PathBuf> {
    ensure!(
        update.size > 0 && update.size <= MAX_DOWNLOAD && !update.asset.is_empty(),
        "请从版本页面下载安装包"
    );
    let directory = root.join("updates");
    std::fs::create_dir_all(&directory)?;
    let file = tempfile::NamedTempFile::new_in(&directory)?;
    let base = format!("{REPOSITORY}/releases/download/desktop-v{}", update.version);
    let mut checksum_command = curl(&format!("{base}/{}.sha256", update.asset), "30");
    checksum_command.args(["--max-filesize", "1024"]);
    let checksum = crate::engine::run(checksum_command, &crate::engine::Control::default())?;
    ensure!(checksum.success, "无法获取安装包校验信息");
    let mut command = curl(&format!("{base}/{}", update.asset), "600");
    command
        .arg("--max-filesize")
        .arg(update.size.to_string())
        .arg("--output")
        .arg(file.path());
    let result = crate::engine::run(command, &crate::engine::Control::default())?;
    ensure!(
        result.success && file.as_file().metadata()?.len() == update.size,
        "安装包下载未完成，请重试"
    );
    verify_checksum(
        file.path(),
        std::str::from_utf8(&checksum.stdout)?,
        &update.asset,
    )?;
    // Release builds pin the publishing team. A matching checksum alone is not
    // an authorization to replace or execute application code.
    if let Some(team) = option_env!("IMG_SIGNING_TEAM") {
        ensure!(
            team.len() == 10 && team.bytes().all(|b| b.is_ascii_alphanumeric()),
            "更新签名配置无效"
        );
        let requirement =
            format!("anchor apple generic and certificate leaf[subject.OU] = \"{team}\"");
        let status = Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", "-R", &requirement])
            .arg(file.path())
            .output()?;
        ensure!(status.status.success(), "安装包签名不属于此应用的发布者");
    }
    let destination = directory.join(&update.asset);
    file.persist(&destination).map_err(|e| e.error)?;
    Ok(destination)
}

pub fn save_check_time(root: &Path) {
    if let Ok(mut file) = tempfile::NamedTempFile::new_in(root) {
        let _ = write!(file, "{}", crate::model::now());
        let _ = file.persist(root.join("last-update-check"));
    }
}
pub fn due(root: &Path) -> bool {
    let last = std::fs::read_to_string(root.join("last-update-check"))
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    crate::model::now().saturating_sub(last) >= 86400
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selects_only_newer_stable_desktop_assets_for_this_architecture() {
        let release = |version: &str| {
            serde_json::json!({"tag_name": format!("desktop-v{version}"), "draft": false, "prerelease": false, "assets": [
                {"name": format!("img-desktop_{version}_macos_arm64.dmg"), "size": 100, "browser_download_url": format!("{REPOSITORY}/releases/download/desktop-v{version}/img-desktop_{version}_macos_arm64.dmg")},
                {"name": format!("img-desktop_{version}_macos_arm64.dmg.sha256"), "size": 100, "browser_download_url": format!("{REPOSITORY}/releases/download/desktop-v{version}/img-desktop_{version}_macos_arm64.dmg.sha256")}
            ]})
        };
        let bytes = serde_json::to_vec(&vec![
            release("0.3.0"),
            release("0.2.0"),
            release("0.4.0-beta.1"),
        ])
        .unwrap();
        assert_eq!(
            select_release(&bytes, "0.2.0", "arm64")
                .unwrap()
                .unwrap()
                .version,
            "0.3.0"
        );
        assert!(select_release(&bytes, "0.3.0", "arm64").unwrap().is_none());
        assert!(select_release(&bytes, "0.2.0", "x86_64").unwrap().is_none());
        let mut malicious = release("0.3.0");
        malicious["assets"][0]["browser_download_url"] = "https://evil.test/app.dmg".into();
        assert!(
            select_release(
                &serde_json::to_vec(&vec![malicious]).unwrap(),
                "0.2.0",
                "arm64"
            )
            .unwrap()
            .is_none()
        );
    }
    #[test]
    fn public_feed_fallback_only_offers_matching_stable_desktop_release_pages() {
        let bytes = br#"<feed><entry><link href="https://github.com/liyown/img/releases/tag/desktop-v0.3.0"/></entry><entry><link href="https://github.com/liyown/img/releases/tag/v9.0.0"/></entry><entry><link href="https://evil.test/releases/tag/desktop-v8.0.0"/></entry><entry><link href="https://github.com/liyown/img/releases/tag/desktop-v1.0.0-beta.1"/></entry></feed>"#;
        let update = select_feed(bytes, "0.2.0").unwrap().unwrap();
        assert_eq!(update.version, "0.3.0");
        assert_eq!(update.size, 0);
        assert!(select_feed(bytes, "0.3.0").unwrap().is_none());
    }
    #[test]
    fn rejects_modified_or_mislabeled_downloads() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"installer").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"installer"));
        assert!(verify_checksum(file.path(), &format!("{digest}  app.dmg"), "app.dmg").is_ok());
        assert!(verify_checksum(file.path(), &format!("{digest}  other.dmg"), "app.dmg").is_err());
        file.write_all(b"modified").unwrap();
        assert!(verify_checksum(file.path(), &format!("{digest}  app.dmg"), "app.dmg").is_err());
    }
}
