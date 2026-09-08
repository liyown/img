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
            self.response(response, "sync conditional write")?;
            Ok(true)
        })()
        .map_err(|e| self.safe_error(e))
    }
}
