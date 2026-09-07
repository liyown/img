use crate::{
    config::Upload,
    control::Control,
    media, network, pathgen,
    provider::{Provider, Request, UploadError},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct FileResult {
    pub local_path: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    #[serde(default, skip_serializing_if = "zero")]
    pub size: u64,
    #[serde(default, skip_serializing_if = "zero")]
    pub original_size: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<crate::failure::ErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}
impl FileResult {
    pub fn failure(source: &str, failure: crate::failure::Failure) -> Self {
        Self {
            local_path: source.into(),
            error: failure.message().into(),
            error_code: Some(failure.code),
            http_status: failure.http_status,
            retryable: Some(failure.retryable),
            ..Default::default()
        }
    }
}
fn zero(n: &u64) -> bool {
    *n == 0
}
#[derive(Clone, Default)]
pub struct Options {
    pub path: String,
    pub name: String,
    pub overwrite: bool,
    pub optimize: bool,
    pub strip_exif: bool,
    pub max_width: u32,
    pub allow_insecure: bool,
}
pub fn run(
    p: &Provider,
    c: &Upload,
    files: &[String],
    o: &Options,
    control: &Control,
) -> Vec<FileResult> {
    let result = Mutex::new(vec![FileResult::default(); files.len()]);
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..c.concurrency.max(1).min(files.len()) {
            let result = &result;
            let next = &next;
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= files.len() {
                        break;
                    }
                    let r = one(p, c, &files[i], o, control).unwrap_or_else(|e| {
                        FileResult::failure(
                            &files[i],
                            crate::failure::Failure::from_error(
                                &e,
                                crate::failure::ErrorCode::Unknown,
                            ),
                        )
                    });
                    result.lock().unwrap()[i] = r;
                }
            });
        }
    });
    result.into_inner().unwrap()
}
fn one(
    p: &Provider,
    c: &Upload,
    source: &str,
    o: &Options,
    control: &Control,
) -> Result<FileResult> {
    control.check()?;
    control.stage("preparing", 0);
    let (data, mut name) = if network::is_url(source) {
        let f = network::fetch(source, c.max_size, o.allow_insecure)?;
        (f.data, f.name)
    } else {
        (
            media::read_image(Path::new(source), c.max_size)?,
            Path::new(source)
                .file_name()
                .context("image has no filename")?
                .to_string_lossy()
                .into_owned(),
        )
    };
    let typ = media::inspect(&data, c.max_size)?;
    let processed = media::process(
        data,
        typ,
        o.strip_exif || c.strip_exif,
        if o.max_width == 0 {
            c.max_width
        } else {
            o.max_width
        },
        o.optimize,
    )?;
    control.check()?;
    if processed.content_type != typ {
        let extension = match processed.content_type.as_str() {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        name = Path::new(&name)
            .with_extension(extension)
            .to_string_lossy()
            .into_owned();
    }
    let template = if o.name.is_empty() {
        &c.path_template
    } else {
        ensure!(
            !o.name.contains(['/', '\\']) && o.name != "." && o.name != "..",
            "--name must be a filename without directories"
        );
        &o.name
    };
    let remote = pathgen::generate(
        &name,
        &processed.data,
        template,
        if o.path.is_empty() { &c.path } else { &o.path },
        &c.rename,
        chrono::Local::now(),
    )?;
    let data: Arc<[u8]> = processed.data.into();
    let overwrite = o.overwrite || c.overwrite || c.conflict == "overwrite";
    let mut attempt = 0;
    let url = loop {
        let result = p.upload(
            Request {
                name: &name,
                remote_path: &remote,
                content_type: &processed.content_type,
                data: data.clone(),
                overwrite,
            },
            control,
        );
        match result {
            Ok(url) => break url,
            Err(e) => {
                if attempt >= c.retry_count
                    || !e.downcast_ref::<UploadError>().is_some_and(|e| e.retryable)
                {
                    return Err(e);
                }
                attempt += 1;
                control.stage("retrying", attempt + 1);
                control.delay(Duration::from_secs(1u64 << ((attempt - 1).min(7))))?;
            }
        }
    };
    Ok(FileResult {
        local_path: source.into(),
        success: true,
        remote_path: remote,
        url,
        provider: p.name.clone(),
        size: data.len() as u64,
        original_size: processed.original_size,
        content_type: processed.content_type,
        error: String::new(),
        ..Default::default()
    })
}
pub fn exit_code(results: &[FileResult]) -> i32 {
    let n = results.iter().filter(|r| r.success).count();
    if n == results.len() && n > 0 {
        0
    } else if n > 0 {
        3
    } else {
        1
    }
}
