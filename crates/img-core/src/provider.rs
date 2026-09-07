use crate::{
    config::{ProviderConfig, is_sensitive},
    control::{Control, ProgressReader},
    network, pathgen,
};
use anyhow::{Context, Result, bail, ensure};
use aws_credential_types::{
    Credentials,
    provider::{ProvideCredentials, SharedCredentialsProvider},
};
use aws_sigv4::{
    http_request::{
        PayloadChecksumKind, PercentEncodingMode, SignableBody, SignableRequest, SigningSettings,
        UriPathNormalizationMode, sign,
    },
    sign::v4,
};
use base64::Engine;
use reqwest::{
    Method, StatusCode,
    blocking::{Body, Client, Response, multipart},
    header::{HeaderMap, HeaderName, HeaderValue},
};
use std::{
    io::Read,
    sync::Arc,
    time::{Duration, SystemTime},
};

#[derive(Debug)]
pub struct UploadError {
    pub message: String,
    pub retryable: bool,
    pub failure: crate::failure::Failure,
}
impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for UploadError {}
#[derive(Clone)]
enum S3Credentials {
    Explicit(Credentials),
    Default {
        runtime: Arc<tokio::runtime::Runtime>,
        provider: SharedCredentialsProvider,
    },
}
#[derive(Clone)]
pub struct Provider {
    pub name: String,
    cfg: ProviderConfig,
    client: Client,
    github_api: String,
    credentials: Option<S3Credentials>,
}
pub struct Request<'a> {
    pub name: &'a str,
    pub remote_path: &'a str,
    pub content_type: &'a str,
    pub data: Arc<[u8]>,
    pub overwrite: bool,
}
impl Provider {
    pub fn path_prefix(&self) -> &str {
        &self.cfg.path_prefix
    }
    pub fn reuse_scope(&self) -> Result<Option<Vec<u8>>> {
        // Default-chain credentials can change accounts without changing config.
        if self.cfg.kind == "s3" && self.cfg.access_key.is_empty() {
            return Ok(None);
        }
        Ok(Some(serde_json::to_vec(&self.cfg)?))
    }
    pub fn new(name: &str, config: &ProviderConfig) -> Result<Self> {
        let cfg = config.resolved()?;
        cfg.validate()?;
        let credentials = if cfg.kind == "s3" {
            if !cfg.access_key.is_empty() {
                Some(S3Credentials::Explicit(Credentials::new(
                    &cfg.access_key,
                    &cfg.secret_key,
                    if cfg.session_token.is_empty() {
                        None
                    } else {
                        Some(cfg.session_token.clone())
                    },
                    None,
                    "img-config",
                )))
            } else {
                let runtime = Arc::new(tokio::runtime::Runtime::new()?);
                let sdk = runtime.block_on(
                    aws_config::defaults(aws_config::BehaviorVersion::latest())
                        .region(aws_config::Region::new(
                            if cfg.region.is_empty() {
                                "auto"
                            } else {
                                &cfg.region
                            }
                            .to_string(),
                        ))
                        .load(),
                );
                Some(S3Credentials::Default {
                    runtime,
                    provider: sdk
                        .credentials_provider()
                        .context("AWS credentials provider unavailable")?,
                })
            }
        } else {
            None
        };
        Ok(Self {
            name: name.into(),
            cfg,
            client: network::client()?,
            github_api: "https://api.github.com".into(),
            credentials,
        })
    }
    fn headers(&self) -> Result<HeaderMap> {
        let mut out = HeaderMap::new();
        for (name, value) in &self.cfg.headers {
            if [
                "content-type",
                "content-length",
                "transfer-encoding",
                "host",
            ]
            .iter()
            .any(|h| name.eq_ignore_ascii_case(h))
            {
                continue;
            }
            let key =
                HeaderName::from_bytes(name.as_bytes()).context("invalid HTTP header name")?;
            let mut value = HeaderValue::from_str(value).context("invalid HTTP header value")?;
            value.set_sensitive(is_sensitive(name));
            out.insert(key, value);
        }
        Ok(out)
    }
    fn response(&self, r: Response, action: &str) -> Result<Vec<u8>> {
        let status = r.status();
        let bytes = network::bounded(r, 1 << 20)?;
        if !status.is_success() {
            return Err(UploadError {
                message: format!(
                    "{action} failed with HTTP {}: {}",
                    status.as_u16(),
                    self.cfg.sanitize(&String::from_utf8_lossy(&bytes))
                ),
                retryable: status.is_server_error()
                    || status == StatusCode::TOO_MANY_REQUESTS
                    || status == StatusCode::REQUEST_TIMEOUT,
                failure: crate::failure::Failure::http(status.as_u16()),
            }
            .into());
        }
        Ok(bytes)
    }
    fn safe_error(&self, e: anyhow::Error) -> anyhow::Error {
        if e.downcast_ref::<UploadError>().is_some() {
            return e;
        }
        let retryable = e
            .downcast_ref::<reqwest::Error>()
            .is_some_and(|e| e.is_connect() || e.is_timeout() || e.is_body());
        UploadError {
            message: self.cfg.sanitize(&format!("{e:#}")),
            retryable,
            failure: crate::failure::Failure::from_error(
                &e,
                crate::failure::ErrorCode::InvalidResponse,
            ),
        }
        .into()
    }
    pub fn upload(&self, r: Request<'_>, control: &Control) -> Result<String> {
        control.check()?;
        let result = match self.cfg.kind.as_str() {
            "http" => self.http_upload(&r, control),
            "github" => self.github_upload(&r, control),
            "s3" => self.s3_upload(&r, control),
            _ => bail!("unsupported provider"),
        };
        result.map_err(|e| self.safe_error(e))
    }
    pub fn test(&self) -> Result<()> {
        let result = (|| -> Result<()> {
            match self.cfg.kind.as_str() {
                "http" => {
                    let r = self
                        .client
                        .head(&self.cfg.url)
                        .headers(self.headers()?)
                        .send()?;
                    if r.status() != StatusCode::METHOD_NOT_ALLOWED {
                        self.response(r, "HTTP connection test")?;
                    }
                }
                "github" => {
                    let r = self
                        .github_request(Method::GET, &self.repository_endpoint())
                        .send()?;
                    self.response(r, "GitHub connection test")?;
                }
                "s3" => {
                    let r = self.s3_request(
                        Method::HEAD,
                        &self.s3_url(None)?,
                        None,
                        "",
                        false,
                        &Control::default(),
                    )?;
                    self.response(r, "S3 connection test")?;
                }
                _ => bail!("unsupported provider"),
            }
            Ok(())
        })();
        result.map_err(|e| self.safe_error(e))
    }
    fn http_upload(&self, r: &Request<'_>, control: &Control) -> Result<String> {
        let field = if self.cfg.file_field.is_empty() {
            "file"
        } else {
            &self.cfg.file_field
        };
        let mut form = multipart::Form::new().part(
            field.to_string(),
            multipart::Part::bytes(r.data.to_vec())
                .file_name(r.name.to_string())
                .mime_str(r.content_type)?,
        );
        for (k, v) in &self.cfg.fields {
            form = form.text(k.clone(), v.clone());
        }
        let boundary = form.boundary().to_string();
        let mut bytes = Vec::new();
        form.into_reader().read_to_end(&mut bytes)?;
        let bytes: Arc<[u8]> = bytes.into();
        let len = bytes.len() as u64;
        let body = Body::sized(ProgressReader::new(bytes, control), len);
        let method = if self.cfg.method.is_empty() {
            Method::POST
        } else {
            Method::from_bytes(self.cfg.method.to_ascii_uppercase().as_bytes())?
        };
        let response = self
            .client
            .request(method, &self.cfg.url)
            .headers(self.headers()?)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()?;
        let doc: serde_json::Value =
            serde_json::from_slice(&self.response(response, "HTTP upload")?)
                .context("invalid HTTP JSON response")?;
        let mut value = &doc;
        for key in self.cfg.url_json_path.split('.') {
            value = value.get(key).context("URL JSON path not found")?;
        }
        let url = value
            .as_str()
            .filter(|s| !s.is_empty())
            .context("URL JSON path is not a non-empty string")?;
        network::secure_url(url, self.cfg.allow_insecure)?;
        Ok(url.into())
    }
    fn repository_endpoint(&self) -> String {
        format!(
            "{}/repos/{}/{}",
            self.github_api.trim_end_matches('/'),
            pathgen::escape(&self.cfg.owner).replace('/', "%2F"),
            pathgen::escape(&self.cfg.repo).replace('/', "%2F")
        )
    }
    fn github_request(&self, method: Method, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .request(method, url)
            .bearer_auth(&self.cfg.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }
    fn github_upload(&self, r: &Request<'_>, control: &Control) -> Result<String> {
        let branch = if self.cfg.branch.is_empty() {
            "main"
        } else {
            &self.cfg.branch
        };
        let endpoint = format!(
            "{}/contents/{}",
            self.repository_endpoint(),
            pathgen::escape(r.remote_path)
        );
        let lookup = self
            .github_request(Method::GET, &endpoint)
            .query(&[("ref", branch)])
            .send()?;
        let sha = if lookup.status() == StatusCode::NOT_FOUND {
            None
        } else {
            let doc: serde_json::Value =
                serde_json::from_slice(&self.response(lookup, "GitHub lookup")?)?;
            ensure!(r.overwrite, "GitHub file already exists; use --overwrite");
            Some(
                doc["sha"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .context("GitHub file response is missing sha")?
                    .to_string(),
            )
        };
        control.check()?;
        let template = if self.cfg.commit_message.is_empty() {
            "upload: {path}"
        } else {
            &self.cfg.commit_message
        };
        let mut payload = serde_json::json!({"branch":branch,"message":template.replace("{path}",r.remote_path),"content":base64::engine::general_purpose::STANDARD.encode(&r.data)});
        if let Some(sha) = sha {
            payload["sha"] = sha.into();
        }
        let data: Arc<[u8]> = serde_json::to_vec(&payload)?.into();
        let len = data.len() as u64;
        let response = self
            .github_request(Method::PUT, &endpoint)
            .header("Content-Type", "application/json")
            .body(Body::sized(ProgressReader::new(data, control), len))
            .send()?;
        self.response(response, "GitHub upload")?;
        let public = if self.cfg.public_url.is_empty() {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}",
                pathgen::escape(&self.cfg.owner),
                pathgen::escape(&self.cfg.repo),
                pathgen::escape(branch)
            )
        } else {
            self.cfg.public_url.clone()
        };
        Ok(format!(
            "{}/{}",
            public.trim_end_matches('/'),
            pathgen::escape(r.remote_path)
        ))
    }
    fn s3_url(&self, key: Option<&str>) -> Result<String> {
        let region = if self.cfg.region.is_empty() {
            "auto"
        } else {
            &self.cfg.region
        };
        let endpoint = if self.cfg.endpoint.is_empty() {
            format!("https://s3.{region}.amazonaws.com")
        } else {
            self.cfg.endpoint.clone()
        };
        let mut url = network::secure_url(&endpoint, self.cfg.allow_insecure)?;
        ensure!(
            url.query().is_none(),
            "S3 endpoint must not contain a query"
        );
        let base_path = url.path().trim_end_matches('/').to_string();
        let path = if self.cfg.path_style {
            format!("{base_path}/{}", pathgen::escape(&self.cfg.bucket))
        } else {
            let host = format!(
                "{}.{}",
                self.cfg.bucket,
                url.host_str().context("missing S3 host")?
            );
            url.set_host(Some(&host))
                .context("invalid S3 bucket hostname")?;
            base_path
        };
        let path = match key {
            Some(k) => format!("{path}/{}", pathgen::escape(k)),
            None => {
                if path.is_empty() {
                    "/".into()
                } else {
                    path
                }
            }
        };
        url.set_path(&path);
        Ok(url.into())
    }
    fn credentials(&self) -> Result<Credentials> {
        match self
            .credentials
            .as_ref()
            .context("S3 credentials not initialized")?
        {
            S3Credentials::Explicit(c) => Ok(c.clone()),
            S3Credentials::Default { runtime, provider } => runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(20), provider.provide_credentials())
                    .await
                    .context("AWS credential lookup timed out")?
                    .context("AWS credential lookup failed")
            }),
        }
    }
    fn s3_request(
        &self,
        method: Method,
        url: &str,
        data: Option<Arc<[u8]>>,
        content_type: &str,
        no_overwrite: bool,
        control: &Control,
    ) -> Result<Response> {
        let identity = self.credentials()?.into();
        let region = if self.cfg.region.is_empty() {
            "auto"
        } else {
            &self.cfg.region
        };
        let mut settings = SigningSettings::default();
        settings.percent_encoding_mode = PercentEncodingMode::Single;
        settings.uri_path_normalization_mode = UriPathNormalizationMode::Disabled;
        settings.payload_checksum_kind = PayloadChecksumKind::XAmzSha256;
        let params = v4::SigningParams::builder()
            .identity(&identity)
            .region(region)
            .name("s3")
            .time(SystemTime::now())
            .settings(settings)
            .build()?
            .into();
        let bytes = data.as_deref().unwrap_or(&[]);
        let mut headers = vec![];
        if !content_type.is_empty() {
            headers.push(("content-type", content_type));
        }
        if no_overwrite {
            headers.push(("if-none-match", "*"));
        }
        let signable = SignableRequest::new(
            method.as_str(),
            url,
            headers.iter().copied(),
            SignableBody::Bytes(bytes),
        )?;
        let (instructions, _) = sign(signable, &params)?.into_parts();
        let mut request = self.client.request(method, url);
        for (k, v) in headers {
            request = request.header(k, v);
        }
        for (k, v) in instructions.headers() {
            let mut value = HeaderValue::from_str(v)?;
            value.set_sensitive(
                k.eq_ignore_ascii_case("authorization")
                    || k.eq_ignore_ascii_case("x-amz-security-token"),
            );
            request = request.header(k, value);
        }
        if let Some(data) = data {
            let len = data.len() as u64;
            request = request.body(Body::sized(ProgressReader::new(data, control), len));
        }
        Ok(request.send()?)
    }
    fn s3_upload(&self, r: &Request<'_>, control: &Control) -> Result<String> {
        let url = self.s3_url(Some(r.remote_path))?;
        if !r.overwrite {
            let head = self.s3_request(Method::HEAD, &url, None, "", false, control)?;
            ensure!(
                !head.status().is_success(),
                "remote object already exists; use --overwrite"
            );
            if head.status() != StatusCode::NOT_FOUND {
                self.response(head, "S3 object lookup")?;
            }
        }
        control.check()?;
        let response = self.s3_request(
            Method::PUT,
            &url,
            Some(r.data.clone()),
            r.content_type,
            !r.overwrite,
            control,
        )?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            bail!("remote object already exists; use --overwrite");
        }
        self.response(response, "S3 upload")?;
        Ok(format!(
            "{}/{}",
            self.cfg.public_url.trim_end_matches('/'),
            pathgen::escape(r.remote_path)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Debug)]
    struct Seen {
        method: String,
        url: String,
        headers: std::collections::BTreeMap<String, String>,
        body: Vec<u8>,
    }
    fn server(
        replies: Vec<(u16, &'static str)>,
    ) -> (String, Arc<Mutex<Vec<Seen>>>, std::thread::JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        let seen = Arc::new(Mutex::new(vec![]));
        let copy = seen.clone();
        let thread = std::thread::spawn(move || {
            for (status, body) in replies {
                let mut r = server
                    .recv_timeout(Duration::from_secs(4))
                    .unwrap()
                    .expect("provider did not send expected request");
                let mut data = vec![];
                r.as_reader().read_to_end(&mut data).unwrap();
                copy.lock().unwrap().push(Seen {
                    method: r.method().as_str().into(),
                    url: r.url().into(),
                    headers: r
                        .headers()
                        .iter()
                        .map(|h| {
                            (
                                h.field.as_str().as_str().to_ascii_lowercase(),
                                h.value.as_str().into(),
                            )
                        })
                        .collect(),
                    body: data,
                });
                r.respond(tiny_http::Response::from_string(body).with_status_code(status))
                    .unwrap();
            }
        });
        (address, seen, thread)
    }
    #[test]
    fn http_multipart_progress_counts_body_not_hashing_and_protects_headers() {
        let (url, seen, thread) =
            server(vec![(200, r#"{"data":{"url":"https://cdn.test/a.png"}}"#)]);
        let mut cfg = ProviderConfig {
            kind: "http".into(),
            url,
            url_json_path: "data.url".into(),
            allow_insecure: true,
            method: "PATCH".into(),
            ..Default::default()
        };
        cfg.fields.insert("folder".into(), "中文".into());
        cfg.headers.insert("Content-Type".into(), "broken".into());
        let p = Provider::new("local", &cfg).unwrap();
        let events = Arc::new(Mutex::new(vec![]));
        let copy = events.clone();
        let control = Control::default().with_reporter(move |e| copy.lock().unwrap().push(e));
        assert_eq!(
            p.upload(
                Request {
                    name: "图.png",
                    remote_path: "a.png",
                    content_type: "image/png",
                    data: Arc::from(b"raw image".as_slice()),
                    overwrite: false
                },
                &control
            )
            .unwrap(),
            "https://cdn.test/a.png"
        );
        thread.join().unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].method, "PATCH");
        assert!(seen[0].headers["content-type"].starts_with("multipart/form-data; boundary="));
        assert!(String::from_utf8_lossy(&seen[0].body).contains("中文"));
        let n = seen[0].body.len() as u64;
        assert_eq!(seen[0].headers["content-length"], n.to_string());
        let events = events.lock().unwrap();
        assert_eq!(events.last().unwrap().sent, n);
        assert_eq!(events.last().unwrap().total, n);
        assert_eq!(events.last().unwrap().stage, "waiting");
    }
    #[test]
    fn s3_signs_exact_escaped_path_payload_and_session_token() {
        let (url, seen, thread) = server(vec![(404, ""), (200, "")]);
        let cfg = ProviderConfig {
            kind: "s3".into(),
            endpoint: url,
            region: "auto".into(),
            bucket: "images".into(),
            public_url: "https://cdn.test".into(),
            path_style: true,
            access_key: "FAKEACCESS".into(),
            secret_key: "fake-test-secret".into(),
            session_token: "fake-session".into(),
            allow_insecure: true,
            ..Default::default()
        };
        let p = Provider::new("s3", &cfg).unwrap();
        let data: Arc<[u8]> = Arc::from(b"payload".as_slice());
        let result = p
            .upload(
                Request {
                    name: "a.png",
                    remote_path: "目录/a %.png",
                    content_type: "image/png",
                    data: data.clone(),
                    overwrite: false,
                },
                &Control::default(),
            )
            .unwrap();
        thread.join().unwrap();
        assert_eq!(result, "https://cdn.test/%E7%9B%AE%E5%BD%95/a%20%25.png");
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].method, "HEAD");
        assert_eq!(seen[1].method, "PUT");
        assert_eq!(seen[1].url, "/images/%E7%9B%AE%E5%BD%95/a%20%25.png");
        assert_eq!(seen[1].body, data.as_ref());
        assert_eq!(seen[1].headers["if-none-match"], "*");
        assert!(
            seen[1].headers["authorization"].starts_with("AWS4-HMAC-SHA256 Credential=FAKEACCESS/")
        );
        assert_eq!(seen[1].headers["x-amz-security-token"], "fake-session");
        use sha2::Digest;
        assert_eq!(
            seen[1].headers["x-amz-content-sha256"],
            format!("{:x}", sha2::Sha256::digest(&data))
        );
    }
    #[test]
    fn github_lookup_overwrite_and_branch_remain_compatible() {
        let (api, seen, thread) = server(vec![(200, r#"{"sha":"existing-sha"}"#), (200, "{}")]);
        let cfg = ProviderConfig {
            kind: "github".into(),
            owner: "owner".into(),
            repo: "repo".into(),
            branch: "feature/photos".into(),
            token: "fake-token".into(),
            ..Default::default()
        };
        let mut p = Provider::new("gh", &cfg).unwrap();
        p.github_api = api;
        let url = p
            .upload(
                Request {
                    name: "a.png",
                    remote_path: "a b.png",
                    content_type: "image/png",
                    data: Arc::from(b"image".as_slice()),
                    overwrite: true,
                },
                &Control::default(),
            )
            .unwrap();
        assert!(url.ends_with("/feature/photos/a%20b.png"));
        thread.join().unwrap();
        let seen = seen.lock().unwrap();
        assert!(seen[0].url.contains("ref=feature%2Fphotos"));
        let body: serde_json::Value = serde_json::from_slice(&seen[1].body).unwrap();
        assert_eq!(body["sha"], "existing-sha");
        assert_eq!(body["content"], "aW1hZ2U=");
        assert_eq!(seen[1].headers["authorization"], "Bearer fake-token");
    }
    #[test]
    fn permanent_errors_and_auth_values_are_not_retried_or_leaked() {
        let (url, _, thread) = server(vec![(403, "Bearer super-secret")]);
        let mut cfg = ProviderConfig {
            kind: "http".into(),
            url,
            url_json_path: "url".into(),
            allow_insecure: true,
            ..Default::default()
        };
        cfg.headers
            .insert("Authorization".into(), "Bearer super-secret".into());
        let p = Provider::new("local", &cfg).unwrap();
        let error = p
            .upload(
                Request {
                    name: "a.png",
                    remote_path: "a.png",
                    content_type: "image/png",
                    data: Arc::from(b"image".as_slice()),
                    overwrite: false,
                },
                &Control::default(),
            )
            .unwrap_err();
        thread.join().unwrap();
        assert!(!error.to_string().contains("super-secret"));
        assert!(!error.downcast_ref::<UploadError>().unwrap().retryable);
    }
    #[test]
    fn stalled_local_response_is_a_retryable_timeout_without_query_secrets() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/upload?token=private", server.server_addr());
        let thread = std::thread::spawn(move || {
            let mut request = server
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .unwrap();
            let mut body = vec![];
            request.as_reader().read_to_end(&mut body).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            let _ = request.respond(tiny_http::Response::from_string("private-body"));
        });
        let config = ProviderConfig {
            kind: "http".into(),
            url,
            url_json_path: "url".into(),
            allow_insecure: true,
            ..Default::default()
        };
        let mut provider = Provider::new("local", &config).unwrap();
        provider.client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(75))
            .build()
            .unwrap();
        let error = provider
            .upload(
                Request {
                    name: "a.png",
                    remote_path: "a.png",
                    content_type: "image/png",
                    data: Arc::from(&b"test-image"[..]),
                    overwrite: false,
                },
                &Control::default(),
            )
            .unwrap_err();
        let failure =
            crate::failure::Failure::from_error(&error, crate::failure::ErrorCode::Unknown);
        assert_eq!(failure.code, crate::failure::ErrorCode::Timeout);
        assert!(failure.retryable);
        assert_eq!(failure.http_status, None);
        let result = crate::upload::FileResult::failure("a.png", failure);
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("private") && !json.contains("token="));
        thread.join().unwrap();
    }
}
