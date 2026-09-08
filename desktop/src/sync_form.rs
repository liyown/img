use std::collections::BTreeMap;

pub type Values = BTreeMap<&'static str, String>;
pub type Errors = BTreeMap<&'static str, &'static str>;

pub fn keys(s3: bool) -> &'static [&'static str] {
    if s3 {
        &[
            "endpoint",
            "bucket",
            "region",
            "access_key",
            "secret_key",
            "prefix",
        ]
    } else {
        &["endpoint", "username", "password", "prefix"]
    }
}
pub fn validate(s3: bool, values: &Values) -> Errors {
    let mut errors = Errors::new();
    for &(key, message) in if s3 {
        &[
            ("endpoint", "请输入 S3 服务地址"),
            ("bucket", "请输入存储桶名称"),
            ("region", "请输入区域；自动区域可填写 auto"),
            ("access_key", "请输入 Access Key ID"),
            ("secret_key", "请输入 Secret Access Key"),
        ][..]
    } else {
        &[
            ("endpoint", "请输入 WebDAV 服务地址"),
            ("username", "请输入用户名"),
            ("password", "请输入应用密码"),
        ][..]
    } {
        if values.get(key).is_none_or(|v| v.trim().is_empty()) {
            errors.insert(key, message);
        }
    }
    if !errors.contains_key("endpoint") {
        match url::Url::parse(
            values
                .get("endpoint")
                .map(String::as_str)
                .unwrap_or("")
                .trim(),
        ) {
            Ok(url) if url.scheme() != "https" => {
                errors.insert("endpoint", "请使用 https:// 开头的服务地址");
            }
            Ok(url)
                if url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none() => {}
            _ => {
                errors.insert("endpoint", "请输入完整服务地址，不包含账号、查询参数或片段");
            }
        }
    }
    let prefix = values
        .get("prefix")
        .map(String::as_str)
        .unwrap_or("")
        .trim();
    if prefix.is_empty() {
        errors.insert("prefix", "请输入专用同步目录，例如 .img-sync/");
    } else if prefix.starts_with('/')
        || prefix.contains('\\')
        || prefix.chars().any(char::is_control)
        || prefix
            .strip_suffix('/')
            .unwrap_or(prefix)
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        errors.insert(
            "prefix",
            "使用相对目录，例如 .img-sync/，不要包含 / 开头或 ..",
        );
    }
    if s3
        && values
            .get("bucket")
            .is_some_and(|v| v.trim().contains(['/', '\\', ' ']))
    {
        errors.insert("bucket", "这里只填写存储桶名称，不填写网址或目录");
    }
    errors
}
pub fn failure_message(code: &str, s3: bool) -> &'static str {
    match code {
        "cancelled" => "操作已取消，填写内容已保留。",
        "invalid_response" => "服务未接受请求，请确认填写的是 WebDAV 或 S3 接口地址。",
        "authentication" if s3 => "身份验证失败，请检查 Access Key ID 和 Secret Access Key。",
        "authentication" => "身份验证失败，请检查用户名和应用密码。",
        "permission" => "没有访问权限，请确认账号可以列出、读取和写入同步目录。",
        "tls_error" => "服务地址的 HTTPS 证书无效或已过期，请更新证书后重试。",
        "network" => "无法连接服务，请检查地址、网络或代理后重试。",
        "timeout" => "连接超时，填写内容已保留，请稍后重试。",
        "rate_limited" => "服务暂时限制了请求次数，请稍后重试。",
        "conditional_writes_unsupported" => {
            "该服务不支持安全同步所需的防覆盖写入，请更换同步存储。"
        }
        "credentials_missing" => "本机保存的连接凭据不可用，请重新编辑连接。",
        "sync_busy" => "另一个同步任务正在运行，请等待完成后重试。",
        "corrupt_remote_data" => "同步目录中有损坏的记录，本次没有应用这些数据。",
        "invalid_config" => "连接配置未被服务接受，请检查服务地址、存储桶和区域。",
        "server" => "存储服务暂时不可用，填写内容已保留，请稍后重试。",
        _ => "连接未完成，填写内容已保留。请检查服务地址与访问权限后重试。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_forms_identify_required_fields_before_connecting() {
        let values = Values::from([("prefix", ".img-sync/".into())]);
        let errors = validate(false, &values);
        assert_eq!(errors.len(), 3);
        assert!(
            errors.contains_key("endpoint")
                && errors.contains_key("username")
                && errors.contains_key("password")
        );
        assert_eq!(validate(true, &values).len(), 5);
    }
    #[test]
    fn address_and_directory_errors_do_not_require_network_requests() {
        let mut values = Values::from([
            ("endpoint", "https://dav.example.com/dav/".into()),
            ("username", "name".into()),
            ("password", "  secret  ".into()),
            ("prefix", ".img-sync".into()),
        ]);
        assert!(validate(false, &values).is_empty());
        assert_eq!(values["password"], "  secret  ");
        for address in [
            "example.com",
            "http://example.com",
            "https://name:secret@example.com",
            "https://example.com/?token=a",
        ] {
            values.insert("endpoint", address.into());
            assert!(validate(false, &values).contains_key("endpoint"));
        }
        for prefix in ["", "/", "/sync/", "a/../", "a//b/", "a\\b"] {
            values.insert("prefix", prefix.into());
            assert!(validate(false, &values).contains_key("prefix"));
        }
    }
    #[test]
    fn failure_messages_are_actionable_and_do_not_repeat_server_payloads() {
        assert!(failure_message("authentication", false).contains("应用密码"));
        assert!(failure_message("tls_error", true).contains("证书"));
        assert!(!failure_message("secret-response", false).contains("secret-response"));
    }
}
