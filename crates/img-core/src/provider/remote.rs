use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct RemoteItem {
    pub path: String,
    pub url: String,
    pub size: u64,
    pub version: String,
    pub directory: bool,
}
#[derive(Serialize, Deserialize)]
pub struct RemotePage {
    pub items: Vec<RemoteItem>,
    pub next: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct S3Page {
    #[serde(default)]
    contents: Vec<S3Object>,
    #[serde(default)]
    common_prefixes: Vec<S3Prefix>,
    next_continuation_token: Option<String>,
    #[serde(default)]
    is_truncated: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct S3Object {
    key: String,
    size: u64,
    #[serde(rename = "ETag", default)]
    etag: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct S3Prefix {
    prefix: String,
}
#[derive(Deserialize)]
struct DavPage {
    #[serde(default, rename = "response")]
    responses: Vec<DavResponse>,
}
#[derive(Deserialize)]
struct DavResponse {
    href: String,
    #[serde(default)]
    propstat: Vec<DavPropstat>,
}
#[derive(Deserialize)]
struct DavPropstat {
    status: String,
    prop: DavProps,
}
#[derive(Default, Deserialize)]
struct DavProps {
    #[serde(default)]
    getcontentlength: u64,
    #[serde(default)]
    getetag: String,
    #[serde(default)]
    resourcetype: DavType,
}
#[derive(Default, Deserialize)]
struct DavType {
    collection: Option<serde::de::IgnoredAny>,
}
impl Provider {
    fn public_object(&self, path: &str) -> String {
        let base = if self.cfg.public_url.is_empty() && self.cfg.kind == "github" {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}",
                pathgen::escape(&self.cfg.owner),
                pathgen::escape(&self.cfg.repo),
                pathgen::escape(self.branch())
            )
        } else {
            self.cfg.public_url.clone()
        };
        format!("{}/{}", base.trim_end_matches('/'), pathgen::escape(path))
    }
    fn branch(&self) -> &str {
        if self.cfg.branch.is_empty() {
            "main"
        } else {
            &self.cfg.branch
        }
    }
    fn dav_url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.cfg.endpoint.trim_end_matches('/'),
            pathgen::escape(path)
        )
    }
    fn dav_request(&self, method: Method, path: &str) -> Result<reqwest::blocking::RequestBuilder> {
        Ok(self
            .client
            .request(method, self.dav_url(path))
            .headers(self.headers()?))
    }
    pub fn list_remote(&self, prefix: &str, cursor: Option<&str>) -> Result<RemotePage> {
        if !prefix.is_empty() {
            pathgen::validate(prefix.trim_end_matches('/'))?;
        }
        self.list_remote_inner(prefix, cursor)
            .map_err(|e| self.safe_error(e))
    }
    fn list_remote_inner(&self, prefix: &str, cursor: Option<&str>) -> Result<RemotePage> {
        match self.cfg.kind.as_str() {
            "s3" => {
                let mut url = url::Url::parse(&self.s3_url(None)?)?;
                {
                    let mut q = url.query_pairs_mut();
                    q.append_pair("list-type", "2")
                        .append_pair("max-keys", "200")
                        .append_pair("delimiter", "/")
                        .append_pair("prefix", prefix);
                    if let Some(cursor) = cursor {
                        q.append_pair("continuation-token", cursor);
                    }
                }
                let response = self.s3_request(
                    Method::GET,
                    url.as_str(),
                    None,
                    "",
                    false,
                    &Control::default(),
                )?;
                let bytes = self.response(response, "S3 list")?;
                let page: S3Page = quick_xml::de::from_reader(bytes.as_slice())?;
                ensure!(
                    !page.is_truncated || page.next_continuation_token.is_some(),
                    "S3 listing omitted its continuation token"
                );
                let mut items = page
                    .contents
                    .into_iter()
                    .map(|o| RemoteItem {
                        url: self.public_object(&o.key),
                        directory: o.key.ends_with('/'),
                        path: o.key,
                        size: o.size,
                        version: o.etag,
                    })
                    .collect::<Vec<_>>();
                items.extend(page.common_prefixes.into_iter().map(|p| RemoteItem {
                    path: p.prefix,
                    url: String::new(),
                    size: 0,
                    version: String::new(),
                    directory: true,
                }));
                Ok(RemotePage {
                    items,
                    next: page.next_continuation_token,
                })
            }
            "github" => {
                ensure!(
                    cursor.is_none(),
                    "GitHub contents listing does not support pagination"
                );
                let endpoint = format!(
                    "{}/contents/{}",
                    self.repository_endpoint(),
                    pathgen::escape(prefix)
                );
                let response = self
                    .github_request(Method::GET, &endpoint)
                    .query(&[("ref", self.branch())])
                    .send()?;
                let bytes = self.response(response, "GitHub list")?;
                let rows: Vec<serde_json::Value> = serde_json::from_slice(&bytes)?;
                // GitHub Contents API caps directories at 1000 entries; do not imply completeness.
                ensure!(
                    rows.len() < 1000,
                    "directory has at least 1000 entries; browse smaller subdirectories"
                );
                let items = rows
                    .into_iter()
                    .filter(|r| matches!(r["type"].as_str(), Some("file" | "dir")))
                    .map(|r| {
                        let path = r["path"]
                            .as_str()
                            .context("GitHub entry omitted path")?
                            .to_string();
                        Ok(RemoteItem {
                            url: self.public_object(&path),
                            path,
                            size: r["size"].as_u64().unwrap_or(0),
                            version: r["sha"].as_str().unwrap_or("").into(),
                            directory: r["type"] == "dir",
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(RemotePage { items, next: None })
            }
            "webdav" => {
                ensure!(cursor.is_none(), "WebDAV does not support listing cursors");
                let response=self.dav_request(Method::from_bytes(b"PROPFIND")?,prefix)?
                    .header("Depth","1").header("Content-Type","application/xml")
                    .body(r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getetag/></d:prop></d:propfind>"#).send()?;
                let bytes = self.response(response, "WebDAV list")?;
                let page: DavPage = quick_xml::de::from_reader(bytes.as_slice())?;
                let base = url::Url::parse(&self.dav_url(""))?;
                let requested = url::Url::parse(&self.dav_url(prefix))?;
                let mut items = vec![];
                for row in page.responses {
                    let href = requested.join(&row.href)?;
                    ensure!(
                        href.origin() == base.origin(),
                        "WebDAV response points outside the configured server"
                    );
                    let Some(relative) = href.path().strip_prefix(base.path()) else {
                        continue;
                    };
                    let path = percent_encoding::percent_decode_str(relative)
                        .decode_utf8()?
                        .into_owned();
                    if path.trim_end_matches('/') == prefix.trim_end_matches('/') {
                        continue;
                    }
                    pathgen::validate(path.trim_end_matches('/'))?;
                    if let Some(p) = row
                        .propstat
                        .into_iter()
                        .find(|p| p.status.split_whitespace().nth(1) == Some("200"))
                    {
                        items.push(RemoteItem {
                            url: self.public_object(&path),
                            path,
                            size: p.prop.getcontentlength,
                            version: p.prop.getetag,
                            directory: p.prop.resourcetype.collection.is_some(),
                        });
                    }
                }
                Ok(RemotePage { items, next: None })
            }
            _ => bail!("this provider does not support remote browsing"),
        }
    }
    /// Authenticated bounded reads; an expected version pins the selected remote object.
    pub fn read_remote(
        &self,
        path: &str,
        version: &str,
        limit: u64,
        control: &Control,
    ) -> Result<Vec<u8>> {
        pathgen::validate(path)?;
        ensure!(!path.ends_with('/'), "cannot download a directory");
        control.check()?;
        let result = (|| -> Result<Vec<u8>> {
            let response = match self.cfg.kind.as_str() {
                "s3" => self.s3_request_condition(
                    Method::GET,
                    &self.s3_url(Some(path))?,
                    None,
                    "",
                    false,
                    (!version.is_empty()).then_some(version),
                    control,
                )?,
                "webdav" => {
                    let mut request = self.dav_request(Method::GET, path)?;
                    if !version.is_empty() {
                        request = request.header("If-Match", version);
                    }
                    request.send()?
                }
                "github" => {
                    ensure!(
                        !version.is_empty() && version.bytes().all(|b| b.is_ascii_hexdigit()),
                        "GitHub download needs the listed blob SHA"
                    );
                    let response = self
                        .github_request(
                            Method::GET,
                            &format!("{}/git/blobs/{version}", self.repository_endpoint()),
                        )
                        .send()?;
                    if !response.status().is_success() {
                        self.response(response, "GitHub download")?;
                        unreachable!()
                    }
                    let body = network::bounded(response, limit.saturating_mul(2))?;
                    let value: serde_json::Value = serde_json::from_slice(&body)?;
                    ensure!(
                        value["sha"].as_str() == Some(version),
                        "remote version changed"
                    );
                    let content = value["content"]
                        .as_str()
                        .context("missing blob content")?
                        .replace(['\n', '\r'], "");
                    let bytes = base64::engine::general_purpose::STANDARD.decode(content)?;
                    ensure!(bytes.len() as u64 <= limit, "download exceeds size limit");
                    return Ok(bytes);
                }
                _ => bail!("provider does not support authenticated remote reads"),
            };
            if !response.status().is_success() {
                self.response(response, "remote download")?;
                unreachable!()
            }
            network::bounded(response, limit)
        })();
        result.map_err(|e| self.safe_error(e))
    }
    /// Delete a single explicitly selected file only if its version still matches.
    pub fn delete_remote(&self, path: &str, version: &str) -> Result<()> {
        pathgen::validate(path)?;
        ensure!(
            !path.ends_with('/') && !version.is_empty(),
            "select a file with a version identifier; directory deletion is disabled"
        );
        self.invalidate_reuse()?;
        let result = (|| -> Result<()> {
            let response=match self.cfg.kind.as_str() {
                "s3"=>self.s3_request_condition(Method::DELETE,&self.s3_url(Some(path))?,None,"",false,Some(version),&Control::default())?,
                "github"=>self.github_request(Method::DELETE,&format!("{}/contents/{}",self.repository_endpoint(),pathgen::escape(path)))
                    .json(&serde_json::json!({"message":format!("delete: {path}"),"sha":version,"branch":self.branch()})).send()?,
                "webdav"=>{
                    // Recheck the resource type: DELETE on a DAV collection is recursive.
                    let parent=path.rsplit_once('/').map(|(p,_)|format!("{p}/")).unwrap_or_default();
                    let item=self.list_remote(&parent,None)?.items.into_iter().find(|i|i.path==path).context("remote file no longer exists")?;
                    ensure!(!item.directory && item.version==version,"remote object changed; refresh before deleting");
                    self.dav_request(Method::DELETE,path)?.header("If-Match",version).send()?
                },
                _=>bail!("this provider does not support remote deletion"),
            };
            self.response(response, "remote delete")?;
            self.invalidate_reuse()
                .context("remote deletion succeeded but link reuse invalidation failed")?;
            Ok(())
        })();
        result.map_err(|e| self.safe_error(e))
    }
    pub(super) fn webdav_upload(&self, r: &Request<'_>, control: &Control) -> Result<String> {
        let parts = r.remote_path.split('/').collect::<Vec<_>>();
        for i in 1..parts.len() {
            control.check()?;
            let response = self
                .dav_request(
                    Method::from_bytes(b"MKCOL")?,
                    &format!("{}/", parts[..i].join("/")),
                )?
                .send()?;
            if response.status() != StatusCode::METHOD_NOT_ALLOWED {
                self.response(response, "WebDAV create directory")?;
            }
        }
        control.check()?;
        let mut request = self
            .dav_request(Method::PUT, r.remote_path)?
            .header("Content-Type", r.content_type);
        if !r.overwrite {
            request = request.header("If-None-Match", "*");
        }
        let response = request
            .body(Body::sized(
                ProgressReader::new(r.data.clone(), control),
                r.data.len() as u64,
            ))
            .send()?;
        self.response(response, "WebDAV upload")?;
        Ok(self.public_object(r.remote_path))
    }
}
