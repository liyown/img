use anyhow::{Context, Result, ensure};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_BATCH: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Status {
    Ready,
    Running,
    Done,
    Failed,
    Paused,
    Cancelled,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub target: String,
    pub source: Option<PathBuf>,
    pub thumbnail: Option<PathBuf>,
    pub asset: String,
    pub status: Status,
    pub progress: Option<u8>,
    pub url: Option<String>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    pub simulated: bool,
    pub added_at: u64,
    #[serde(default)]
    pub uploaded_size: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_record_id: Option<String>,
}

impl Item {
    pub fn provider_label(&self) -> String {
        if self.origin.is_empty() {
            self.target.clone()
        } else {
            format!("{} · {}", self.target, self.origin)
        }
    }
    pub fn fixture(name: &str, size: u64, target: &str, asset: &str, progress: u8) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            size,
            target: target.into(),
            source: None,
            thumbnail: None,
            asset: asset.into(),
            status: if progress == 100 {
                Status::Done
            } else {
                Status::Running
            },
            progress: Some(progress),
            url: (progress == 100).then(|| format!("https://example.com/{name}")),
            error: None,
            error_code: None,
            http_status: None,
            retryable: None,
            simulated: true,
            added_at: now(),
            uploaded_size: None,
            origin: String::new(),
            imported_record_id: None,
        }
    }
    pub fn reference_items() -> Vec<Self> {
        vec![
            Self::fixture(
                "banner_spring.webp",
                3_711_959,
                "SM.MS",
                "queue-coast.png",
                87,
            ),
            Self::fixture("avatar_v2.png", 430_080, "Imgur", "queue-bottle.png", 100),
        ]
    }
    pub fn size_label(&self) -> String {
        match self.uploaded_size {
            Some(size) if size < self.size => {
                format!("{} → {}", size_label(self.size), size_label(size))
            }
            _ => size_label(self.size),
        }
    }
    #[cfg(any(test, feature = "perf"))]
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || format!(
                "{} {} {}",
                self.name,
                self.target,
                self.url.as_deref().unwrap_or("")
            )
            .to_lowercase()
            .contains(&query.to_lowercase())
    }
}

pub fn data_dir() -> Result<PathBuf> {
    img_records::data_dir()
}

pub fn load(root: &Path) -> Result<Vec<Item>> {
    let path = root.join("queue.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut items: Vec<Item> =
        serde_json::from_slice(&std::fs::read(path)?).context("本地队列无法读取，请保留原文件")?;
    for item in &mut items {
        if item.status == Status::Running {
            item.status = Status::Paused;
            item.progress = None;
            item.error = Some("上次上传被中断，请检查远端结果后决定是否重试。".into());
        }
    }
    Ok(items)
}

#[cfg(test)]
pub fn save(root: &Path, items: &[Item]) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let real: Vec<_> = items.iter().filter(|i| !i.simulated).collect();
    let bytes = serde_json::to_vec_pretty(&real)?;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(root.join("queue.json")).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
pub fn prepare_file(path: &Path, root: &Path, target: &str) -> Result<Item> {
    prepare_file_with_limit(path, root, target, MAX_BYTES)
}
pub fn prepare_file_with_limit(
    path: &Path,
    root: &Path,
    target: &str,
    max_bytes: u64,
) -> Result<Item> {
    let metadata = path.metadata().context("无法读取图片")?;
    ensure!(metadata.is_file(), "请拖入图片文件，暂不支持文件夹");
    ensure!(
        metadata.len() <= max_bytes,
        "图片超过 {} MB 的大小上限",
        max_bytes / 1024 / 1024
    );
    let mut bytes = vec![];
    File::open(path)?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)?;
    prepare_bytes_with_limit(
        bytes,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
        root,
        target,
        max_bytes,
    )
}

#[cfg(test)]
pub fn prepare_bytes(bytes: Vec<u8>, name: &str, root: &Path, target: &str) -> Result<Item> {
    prepare_bytes_with_limit(bytes, name, root, target, MAX_BYTES)
}

pub fn prepare_bytes_with_limit(
    bytes: Vec<u8>,
    name: &str,
    root: &Path,
    target: &str,
    max_bytes: u64,
) -> Result<Item> {
    ensure!(
        bytes.len() as u64 <= max_bytes,
        "图片超过 {} MB 的大小上限",
        max_bytes / 1024 / 1024
    );
    let format = image::guess_format(&bytes).ok();
    let mut preview = None;
    let mut svg_preview = None;
    let ext;
    if let Some(format) = format {
        ensure!(
            matches!(
                format,
                ImageFormat::Png
                    | ImageFormat::Jpeg
                    | ImageFormat::Gif
                    | ImageFormat::WebP
                    | ImageFormat::Avif
            ),
            "暂不支持该图片格式"
        );
        ext = format.extensions_str()[0];
        if format != ImageFormat::Avif {
            let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
            let mut limits = Limits::default();
            limits.max_alloc = Some(96 * 1024 * 1024);
            limits.max_image_width = Some(16384);
            limits.max_image_height = Some(16384);
            reader.limits(limits);
            let mut decoder = reader.into_decoder().context("图片损坏或尺寸过大")?;
            let (width, height) = decoder.dimensions();
            ensure!(
                u64::from(width) * u64::from(height) <= 24_000_000,
                "图片超过 2400 万像素的预览限制"
            );
            let orientation = decoder
                .orientation()
                .unwrap_or(image::metadata::Orientation::NoTransforms);
            let mut image = image::DynamicImage::from_decoder(decoder)?;
            image.apply_orientation(orientation);
            preview = Some(image.thumbnail(1200, 900));
        }
    } else {
        let mut options = resvg::usvg::Options::default();
        // SVG previews cannot read local files or fetch external images.
        options.image_href_resolver.resolve_string = Box::new(|_, _| None);
        options.image_href_resolver.resolve_data = Box::new(|_, _, _| None);
        let tree = resvg::usvg::Tree::from_data(&bytes, &options)
            .context("请选择 PNG、JPEG、GIF、WebP、SVG 或 AVIF 图片")?;
        let size = tree.size();
        let scale = (1200. / size.width()).min(900. / size.height()).min(1.);
        let mut pixmap = resvg::tiny_skia::Pixmap::new(
            (size.width() * scale).ceil().max(1.) as u32,
            (size.height() * scale).ceil().max(1.) as u32,
        )
        .context("SVG 尺寸无效")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        svg_preview = Some(pixmap.encode_png()?);
        ext = "svg";
    }
    let id = uuid::Uuid::new_v4().to_string();
    let directory = root.join("images").join(&id);
    std::fs::create_dir_all(directory.join("original"))?;
    let name = if name.is_empty() {
        format!("clipboard-{}.{}", now(), ext)
    } else {
        Path::new(name)
            .file_name()
            .context("图片名称无效")?
            .to_string_lossy()
            .into_owned()
    };
    let source = directory.join("original").join(&name);
    let thumbnail = directory.join("preview.png");
    let write = (|| -> Result<()> {
        std::fs::write(&source, &bytes)?;
        if let Some(image) = preview {
            image.save_with_format(&thumbnail, ImageFormat::Png)?;
        }
        if let Some(png) = svg_preview {
            std::fs::write(&thumbnail, png)?;
        }
        #[cfg(target_os = "macos")]
        if ext == "avif" {
            let mut command = Command::new("/usr/bin/sips");
            command
                .args(["-s", "format", "png", "-Z", "1200"])
                .arg(&source)
                .arg("--out")
                .arg(&thumbnail);
            let _ = crate::engine::run(command, &crate::engine::Control::default());
        }
        if thumbnail.exists() {
            let _ = crate::thumbnails::prepare(root, &thumbnail);
        }
        Ok(())
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_dir_all(directory);
        return Err(e);
    }
    Ok(Item {
        id,
        name,
        size: bytes.len() as u64,
        target: target.into(),
        source: Some(source),
        thumbnail: thumbnail.exists().then_some(thumbnail),
        asset: String::new(),
        status: Status::Ready,
        progress: None,
        url: None,
        error: None,
        error_code: None,
        http_status: None,
        retryable: None,
        simulated: false,
        added_at: now(),
        uploaded_size: None,
        origin: String::new(),
        imported_record_id: None,
    })
}

pub fn prepare_url_controlled(
    url: &str,
    root: &Path,
    target: &str,
    binary: &Path,
    options: &crate::upload_options::UploadOptions,
    control: &crate::engine::Control,
) -> Result<Item> {
    let directory = tempfile::tempdir_in(root)?;
    let dest = directory.path().join("download");
    let mut command = Command::new(binary);
    command
        .current_dir(directory.path())
        .args(["fetch", "--output"])
        .arg(&dest)
        .arg("--max-size")
        .arg(options.max_bytes().to_string());
    if options.allow_http_sources {
        command.arg("--allow-insecure");
    }
    command.arg(url);
    let result = crate::engine::run(command, control)?;
    ensure!(
        result.success,
        "链接图片下载失败，请检查地址、大小限制与 HTTP 设置"
    );
    let meta: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    prepare_bytes_with_limit(
        std::fs::read(dest)?,
        meta["name"].as_str().unwrap_or("download.png"),
        root,
        target,
        options.max_bytes(),
    )
}

#[cfg(test)]
pub fn remove_records(root: &Path, items: &mut Vec<Item>, ids: &[String]) -> Result<usize> {
    let removed: Vec<_> = items
        .iter()
        .filter(|i| ids.contains(&i.id) && i.status != Status::Running)
        .cloned()
        .collect();
    let remaining: Vec<_> = items
        .iter()
        .filter(|i| !removed.iter().any(|r| r.id == i.id))
        .cloned()
        .collect();
    save(root, &remaining)?;
    *items = remaining;
    remove_cache(root, &removed);
    Ok(removed.len())
}

pub fn remove_cache(root: &Path, removed: &[Item]) {
    for item in removed {
        // Only delete this app's UUID cache directory, never source paths from the record.
        if uuid::Uuid::parse_str(&item.id).is_ok() && !item.simulated {
            let path = root.join("images").join(&item.id);
            if path
                .symlink_metadata()
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

// This snapshot lives only in memory and is never included in queue JSON or diagnostics.
#[derive(Clone)]
pub struct UploadConfiguration {
    pub(crate) config: String,
    pub(crate) environment: std::collections::BTreeMap<String, String>,
}
impl UploadConfiguration {
    pub fn capture(target: &str) -> Result<Self> {
        let (config, environment) = crate::storage::engine_config(
            &crate::storage::config_path()?,
            target,
            &crate::storage::SystemCredentials,
        )?;
        Ok(Self {
            config,
            environment,
        })
    }
}
pub struct UploadResult {
    pub url: String,
    pub size: Option<u64>,
}
pub enum UploadOutcome {
    Done(UploadResult),
    Paused,
    Cancelled,
}
pub fn upload(
    item: &Item,
    root: &Path,
    binary: &Path,
    options: &crate::upload_options::UploadOptions,
    control: &crate::engine::Control,
    configuration: Option<&UploadConfiguration>,
) -> Result<UploadOutcome> {
    let source = item.source.as_ref().context("待上传原图不可用")?;
    ensure!(!item.target.is_empty(), "请先选择存储源");
    let captured;
    let configuration = if let Some(configuration) = configuration {
        configuration
    } else {
        captured = UploadConfiguration::capture(&item.target)?;
        &captured
    };
    let config = options.engine_config(&configuration.config)?;
    let working_directory = tempfile::tempdir_in(root)?;
    let mut config_file = tempfile::NamedTempFile::new_in(working_directory.path())?;
    config_file.write_all(config.as_bytes())?;
    config_file.as_file().sync_all()?;
    let mut command = Command::new(binary);
    command
        .current_dir(working_directory.path())
        .env("IMG_DATA_DIR", root)
        .env_remove("IMG_PROVIDER")
        .env_remove("IMG_DEFAULT_PROVIDER")
        .env_remove("IMG_OUTPUT_FORMAT")
        .env_remove("IMG_OUTPUT_COPY")
        .env_remove("IMG_UPLOAD_CONCURRENCY")
        .envs(&configuration.environment)
        .arg("--config")
        .arg(config_file.path())
        .args([
            "upload",
            "--provider",
            &item.target,
            "--format",
            "json",
            "--no-copy",
            "--progress",
        ]);
    if options.optimize {
        command.arg("--optimize");
    }
    command.arg(source);
    let result = crate::engine::run(command, control)?;
    match result.stopped {
        crate::engine::PAUSE => return Ok(UploadOutcome::Paused),
        crate::engine::CANCEL => return Ok(UploadOutcome::Cancelled),
        _ => {}
    }
    let url = parse_upload_response(result.success, &result.stdout)?;
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    Ok(UploadOutcome::Done(UploadResult {
        url,
        size: payload["files"][0]["size"].as_u64(),
    }))
}

fn parse_upload_response(process_succeeded: bool, bytes: &[u8]) -> Result<String> {
    let payload: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("上传未完成，请检查存储配置、环境变量和网络连接"))?;
    let file = payload
        .get("files")
        .and_then(|v| v.get(0))
        .context("上传未返回文件结果")?;
    if !(process_succeeded && file.get("success").and_then(|v| v.as_bool()) == Some(true)) {
        return Err(crate::diagnostics::Failure::from_json(file).into());
    }
    let url = file
        .get("url")
        .and_then(|v| v.as_str())
        .context("上传完成但未返回链接，请检查公开地址配置")?;
    ensure!(
        url.starts_with("https://") || url.starts_with("http://"),
        "上传返回了无效的公开地址"
    );
    Ok(url.into())
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn size_label(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MB", size as f64 / 1_048_576.)
    } else {
        format!("{} KB", size / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_preserves_original_bytes_and_recovers_interruption() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("preview.png");
        image::RgbImage::new(32, 32).save(&source).unwrap();
        let mut item = prepare_file(&source, temp.path(), "storage").unwrap();
        assert_eq!(
            std::fs::read(item.source.as_ref().unwrap()).unwrap(),
            std::fs::read(source).unwrap()
        );
        item.status = Status::Running;
        let mut queue = vec![item];
        queue.extend(Item::reference_items());
        save(temp.path(), &queue).unwrap();
        let restored = load(temp.path()).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].status, Status::Paused);
        assert!(
            prepare_bytes(
                vec![0; MAX_BYTES as usize + 1],
                "oversized.png",
                temp.path(),
                "x"
            )
            .is_err()
        );
        assert!(prepare_bytes(b"invalid".to_vec(), "bad.png", temp.path(), "x").is_err());
    }
    #[test]
    fn removing_records_keeps_user_originals_and_running_tasks() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("user.png");
        image::RgbImage::new(12, 12).save(&source).unwrap();
        let done = prepare_file(&source, root.path(), "test").unwrap();
        let mut running = prepare_file(&source, root.path(), "test").unwrap();
        running.status = Status::Running;
        let ids = vec![done.id.clone(), running.id.clone()];
        let mut queue = vec![done.clone(), running];
        assert_eq!(remove_records(root.path(), &mut queue, &ids).unwrap(), 1);
        assert!(source.exists());
        assert!(!root.path().join("images").join(done.id).exists());
        assert_eq!(queue.len(), 1);
        assert_eq!(load(root.path()).unwrap().len(), 1);
    }
    #[test]
    fn svg_import_renders_locally_and_preserves_original() {
        let root = tempfile::tempdir().unwrap();
        let bytes = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect width="200" height="100" fill="#ffaa33"/></svg>"##.to_vec();
        let item = prepare_bytes(bytes.clone(), "vector.svg", root.path(), "test").unwrap();
        assert_eq!(std::fs::read(item.source.unwrap()).unwrap(), bytes);
        assert!(item.thumbnail.unwrap().exists());
    }
    #[test]
    fn filtering_includes_filename_provider_and_url() {
        let row = Item::reference_items().remove(1);
        assert!(row.matches("AVATAR"));
        assert!(row.matches("imgur"));
        assert!(row.matches("example.com"));
        assert!(!row.matches("not-present"));
    }

    #[test]
    fn upload_requires_successful_process_file_and_public_url() {
        let success = br#"{"files":[{"success":true,"url":"https://cdn.example.com/photo.png"}]}"#;
        assert_eq!(
            parse_upload_response(true, success).unwrap(),
            "https://cdn.example.com/photo.png"
        );
        assert!(parse_upload_response(false, success).is_err());
        for response in [
            br#"{"files":[{"success":false,"url":"https://cdn.example.com/photo.png"}]}"#
                .as_slice(),
            br#"{"files":[{"success":true}]}"#,
            br#"{"files":[{"success":true,"url":"file:///tmp/photo.png"}]}"#,
            b"",
            b"not json",
        ] {
            assert!(parse_upload_response(true, response).is_err());
        }
    }
}
