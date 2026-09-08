//! Conditional metadata writes bypass image processing and never overwrite an existing batch.
use super::*;
impl Provider {
    pub fn sync_storage_supported(&self) -> Result<()> {
        ensure!(
            matches!(self.cfg.kind.as_str(), "s3" | "webdav"),
            "sync requires WebDAV or S3 storage"
        );
        let url = url::Url::parse(&self.cfg.endpoint)?;
        let loopback = url
            .host_str()
            .is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h == "[::1]");
        ensure!(
            url.scheme() == "https" || (loopback && self.cfg.allow_insecure),
            "sync storage requires HTTPS"
        );
        Ok(())
    }
    /// false means a pre-existing object rejected this write. Read-back must still verify its bytes.
    pub fn create_sync_object(
        &self,
        path: &str,
        bytes: Arc<[u8]>,
        control: &Control,
    ) -> Result<bool> {
        self.sync_storage_supported()?;
        pathgen::validate(path)?;
        (|| {
            control.check()?;
            let response = if self.cfg.kind == "s3" {
                self.s3_request(
                    Method::PUT,
                    &self.s3_url(Some(path))?,
                    Some(bytes),
                    "application/json",
                    true,
                    control,
                )?
            } else {
                let parts = path.split('/').collect::<Vec<_>>();
                for i in 1..parts.len() {
                    control.check()?;
                    let response = self
                        .client
                        .request(
                            Method::from_bytes(b"MKCOL")?,
                            format!(
                                "{}/{}/",
                                self.cfg.endpoint.trim_end_matches('/'),
                                pathgen::escape(&parts[..i].join("/"))
                            ),
                        )
                        .headers(self.headers()?)
                        .send()?;
                    if response.status() != StatusCode::METHOD_NOT_ALLOWED {
                        self.response(response, "sync create directory")?;
                    }
                }
                self.client
                    .put(format!(
                        "{}/{}",
                        self.cfg.endpoint.trim_end_matches('/'),
                        pathgen::escape(path)
                    ))
                    .headers(self.headers()?)
                    .header("If-None-Match", "*")
                    .header("Content-Type", "application/json")
                    .body(Body::sized(
                        ProgressReader::new(bytes.clone(), control),
                        bytes.len() as u64,
                    ))
                    .send()?
            };
            if response.status() == StatusCode::PRECONDITION_FAILED {
                return Ok(false);
            }
            if self.cfg.kind == "s3"
                && self.is_aliyun_oss()
                && response.status() == StatusCode::CONFLICT
            {
                let body = network::bounded(response, 64 * 1024)?;
                if oss_existing_object(&body) {
                    return Ok(false);
                }
                return Err(UploadError {
                    message: "OSS rejected the sync write with a storage conflict".into(),
                    retryable: false,
                    failure: crate::failure::Failure::http(409),
                }
                .into());
            }
            self.response(response, "sync conditional write")?;
            Ok(true)
        })()
        .map_err(|e| self.safe_error(e))
    }
}

fn oss_existing_object(body: &[u8]) -> bool {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        #[serde(rename = "Code")]
        code: String,
    }
    quick_xml::de::from_reader::<_, ErrorBody>(body)
        .is_ok_and(|error| error.code == "FileAlreadyExists")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oss_overwrite_rejection_is_distinct_from_other_conflicts() {
        assert!(oss_existing_object(
            b"<Error><Code>FileAlreadyExists</Code><Message>Object exists</Message></Error>"
        ));
        assert!(!oss_existing_object(
            b"<Error><Code>FileImmutable</Code></Error>"
        ));
        assert!(!oss_existing_object(
            b"<Error><Message>FileAlreadyExists</Message></Error>"
        ));
        assert!(!oss_existing_object(b"not XML"));
    }
}
