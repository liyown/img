use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UploadOptions {
    pub image_format: String,
    pub quality: u8,
    pub max_edge: u32,
    pub watermark: String,
    pub watermark_opacity: u8,
    pub reuse: bool,
    pub optimize: bool,
    pub strip_exif: bool,
    pub max_width: u32,
    pub concurrency: usize,
    pub retry_count: u32,
    pub max_size_mb: u64,
    pub path: String,
    pub path_template: String,
    pub rename: String,
    pub overwrite: bool,
    pub allow_http_sources: bool,
}
impl Default for UploadOptions {
    fn default() -> Self {
        Self {
            image_format: "original".into(),
            quality: 85,
            max_edge: 0,
            watermark: String::new(),
            watermark_opacity: 60,
            reuse: false,
            optimize: false,
            strip_exif: false,
            max_width: 0,
            concurrency: 3,
            retry_count: 2,
            max_size_mb: 8,
            path: String::new(),
            path_template: "{year}/{month}/{filename}".into(),
            rename: "original".into(),
            overwrite: false,
            allow_http_sources: false,
        }
    }
}
impl UploadOptions {
    pub fn max_bytes(&self) -> u64 {
        self.max_size_mb * 1024 * 1024
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(
                self.image_format.as_str(),
                "original" | "png" | "jpeg" | "webp"
            ),
            "图片输出格式无效"
        );
        ensure!(
            (1..=100).contains(&self.quality) && self.watermark_opacity <= 100,
            "JPEG 质量应为 1–100，水印透明度应为 0–100"
        );
        ensure!(self.max_edge <= 32768, "最长边应为 0–32768");
        ensure!((1..=6).contains(&self.concurrency), "同时上传数量应为 1–6");
        ensure!(self.retry_count <= 5, "自动重试次数应为 0–5");
        ensure!(
            (1..=128).contains(&self.max_size_mb),
            "单张大小上限应为 1–128 MB"
        );
        ensure!(
            self.max_width == 0 || (16..=16384).contains(&self.max_width),
            "最大宽度应为 16–16384 像素，或填写 0 保持原尺寸"
        );
        ensure!(
            ["original", "timestamp", "hash", "uuid"].contains(&self.rename.as_str()),
            "命名方式无效"
        );
        for (value, empty) in [(&self.path, true), (&self.path_template, false)] {
            if empty && value.is_empty() {
                continue;
            }
            ensure!(
                !value.is_empty()
                    && value.len() <= 1024
                    && !value.contains('\\')
                    && !value.chars().any(char::is_control)
                    && value
                        .split('/')
                        .all(|s| !s.is_empty() && s != "." && s != ".."),
                "上传路径需要是相对路径，不能包含空目录或 .."
            );
            let mut remainder = value.as_str();
            while let Some((_, after)) = remainder.split_once('{') {
                let Some((key, rest)) = after.split_once('}') else {
                    anyhow::bail!("路径模板缺少右花括号")
                };
                ensure!(
                    [
                        "year",
                        "month",
                        "day",
                        "timestamp",
                        "unix",
                        "filename",
                        "stem",
                        "ext",
                        "hash",
                        "uuid"
                    ]
                    .contains(&key),
                    "路径模板含有不支持的变量"
                );
                remainder = rest;
            }
        }
        Ok(())
    }
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("upload-options.json");
        let value: Self = if path.exists() {
            serde_json::from_slice(&std::fs::read(path)?)?
        } else {
            Self::default()
        };
        value.validate()?;
        Ok(value)
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.as_file().sync_all()?;
        file.persist(root.join("upload-options.json"))
            .map_err(|e| e.error)?;
        Ok(())
    }
    pub fn engine_config(&self, config: &str) -> Result<String> {
        self.validate()?;
        let mut table: toml::Table = toml::from_str(config)?;
        table.insert(
            "upload".into(),
            toml::Value::Table(
                [
                    (
                        "recipe".into(),
                        toml::Value::Table(
                            [
                                (
                                    "format".into(),
                                    toml::Value::String(self.image_format.clone()),
                                ),
                                ("quality".into(), toml::Value::Integer(self.quality.into())),
                                (
                                    "max_edge".into(),
                                    toml::Value::Integer(self.max_edge.into()),
                                ),
                                (
                                    "watermark".into(),
                                    toml::Value::String(self.watermark.clone()),
                                ),
                                (
                                    "opacity".into(),
                                    toml::Value::Integer(self.watermark_opacity.into()),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        ),
                    ),
                    ("reuse".into(), toml::Value::Boolean(self.reuse)),
                    ("path".into(), toml::Value::String(self.path.clone())),
                    (
                        "path_template".into(),
                        toml::Value::String(self.path_template.clone()),
                    ),
                    ("rename".into(), toml::Value::String(self.rename.clone())),
                    (
                        "conflict".into(),
                        toml::Value::String(
                            if self.overwrite { "overwrite" } else { "error" }.into(),
                        ),
                    ),
                    ("overwrite".into(), toml::Value::Boolean(self.overwrite)),
                    ("strip_exif".into(), toml::Value::Boolean(self.strip_exif)),
                    (
                        "max_width".into(),
                        toml::Value::Integer(self.max_width.into()),
                    ),
                    ("concurrency".into(), toml::Value::Integer(1)),
                    (
                        "retry_count".into(),
                        toml::Value::Integer(self.retry_count.into()),
                    ),
                    (
                        "max_size".into(),
                        toml::Value::Integer(self.max_bytes() as i64),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
        );
        Ok(toml::to_string(&table)?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_validate_and_override_engine_defaults() {
        let root = tempfile::tempdir().unwrap();
        let mut options = UploadOptions {
            strip_exif: true,
            max_width: 1200,
            ..Default::default()
        };
        options.save(root.path()).unwrap();
        assert_eq!(UploadOptions::load(root.path()).unwrap(), options);
        let config: toml::Table = toml::from_str(
            &options
                .engine_config("[upload]\noverwrite = true\nmax_width = 500")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(config["upload"]["overwrite"].as_bool(), Some(false));
        assert_eq!(config["upload"]["max_width"].as_integer(), Some(1200));
        options.path = "../outside".into();
        assert!(options.validate().is_err());
    }
}
