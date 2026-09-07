use crate::model::Item;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Failure {
    pub error_code: String,
    pub http_status: Option<u16>,
    pub retryable: Option<bool>,
}
impl Failure {
    pub fn new(code: &str) -> Self {
        Self {
            error_code: canonical(code).into(),
            http_status: None,
            retryable: None,
        }
    }
    pub fn from_json(value: &serde_json::Value) -> Self {
        Self {
            error_code: canonical(value["error_code"].as_str().unwrap_or("unknown")).into(),
            http_status: value["http_status"]
                .as_u64()
                .filter(|s| (100..600).contains(s))
                .map(|s| s as u16),
            retryable: value["retryable"].as_bool(),
        }
    }
    pub fn from_error(e: &anyhow::Error) -> Self {
        if let Some(f) = e.downcast_ref::<Self>() {
            return f.clone();
        }
        if let Some(e) = e.downcast_ref::<std::io::Error>() {
            return Self::new(match e.kind() {
                std::io::ErrorKind::NotFound => "file_not_found",
                std::io::ErrorKind::PermissionDenied => "permission",
                _ => "io",
            });
        }
        Self::new("unknown")
    }
    pub fn summary(&self) -> &'static str {
        match self.error_code.as_str() {
            "invalid_config" => "存储配置无效，请检查默认存储源与必填参数",
            "authentication" => "凭据无效或已过期，请更新存储凭据",
            "permission" => "没有访问权限，请检查存储与本地文件权限",
            "network" => "网络连接失败，请检查网络后重试",
            "timeout" => "请求超时，可以稍后重试",
            "rate_limited" => "服务请求过于频繁，请稍后重试",
            "conflict" => "远端已有同名文件，请调整命名或覆盖设置",
            "too_large" => "图片超过大小限制，请缩小图片或调整设置",
            "invalid_image" => "图片已损坏或格式不受支持，请重新选择文件",
            "file_not_found" => "原图副本不可用，请重新选择原文件",
            "io" => "文件读写失败，请检查磁盘空间和目录权限",
            "server" => "存储服务暂时不可用，可以稍后重试",
            "invalid_response" => "服务未返回有效图片链接，请检查公开地址配置",
            "cancelled" => "上传已取消",
            _ => "上传失败，请查看详情或检查存储配置",
        }
    }
    pub fn needs_file(&self) -> bool {
        matches!(self.error_code.as_str(), "file_not_found" | "invalid_image")
    }
    pub fn needs_settings(&self) -> bool {
        matches!(
            self.error_code.as_str(),
            "invalid_config"
                | "authentication"
                | "permission"
                | "conflict"
                | "too_large"
                | "invalid_response"
        )
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.summary())
    }
}
impl std::error::Error for Failure {}
fn canonical(value: &str) -> &str {
    match value {
        "invalid_config" | "authentication" | "permission" | "network" | "timeout"
        | "rate_limited" | "conflict" | "too_large" | "invalid_image" | "file_not_found" | "io"
        | "server" | "invalid_response" | "cancelled" => value,
        _ => "unknown",
    }
}
pub fn report(items: &[Item]) -> Vec<u8> {
    let failures: Vec<_> = items.iter().rev().filter(|i| i.status == crate::model::Status::Failed).take(100)
        .map(|i| serde_json::json!({"stage":"upload","error_code":canonical(i.error_code.as_deref().unwrap_or("unknown")),"http_status":i.http_status.filter(|s| (100..600).contains(s)),"retryable":i.retryable})).collect();
    serde_json::to_vec_pretty(&serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,"failures":failures})).unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_only_allowlisted_fields_and_accepts_old_errors() {
        let mut item = Item::reference_items().remove(0);
        item.status = crate::model::Status::Failed;
        item.error = Some("token=secret https://private/?key=secret".into());
        item.error_code = Some("secret".into());
        item.name = "private-name".into();
        let text = String::from_utf8(report(&[item])).unwrap();
        assert!(!text.contains("secret") && !text.contains("private-name"));
        assert!(text.contains("unknown"));
        assert_eq!(
            Failure::from_json(&serde_json::json!({"error":"secret"})).error_code,
            "unknown"
        );
    }
}
