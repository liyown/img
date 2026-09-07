use serde::Serialize;
use std::io::Read;

#[derive(Debug, Serialize)]
pub struct Check {
    pub accessible: bool,
    pub code: &'static str,
    pub http_status: Option<u16>,
    pub message: &'static str,
}
pub fn check(url: &str, allow_http: bool) -> Check {
    let result = (|| -> anyhow::Result<Check> {
        let url = crate::network::secure_url(url, allow_http)?;
        // No provider credentials are attached to a public-link check.
        let mut response = crate::network::client()?
            .get(url)
            .header("Range", "bytes=0-4095")
            .send()?;
        let status = response.status().as_u16();
        let (code, message) = match status {
            401 | 403 => (
                "access_denied",
                "Public access was denied. Check bucket policy, CDN access rules and hotlink protection.",
            ),
            404 | 410 => (
                "not_found",
                "Image was not found. Check the public domain and path, or upload again if the object was deleted.",
            ),
            429 => (
                "rate_limited",
                "The public endpoint is rate limiting requests. Try again later.",
            ),
            500..=599 => (
                "server_error",
                "The storage or CDN endpoint is unavailable. Check its status and retry.",
            ),
            200..=299 => {
                let mut bytes = vec![];
                (&mut response).take(4096).read_to_end(&mut bytes)?;
                let image = image::guess_format(&bytes).is_ok()
                    || (response
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|v| v.split(';').next() == Some("image/svg+xml"))
                        && String::from_utf8_lossy(&bytes).contains("<svg"));
                return Ok(Check {
                    accessible: image,
                    code: if image { "ok" } else { "not_image" },
                    http_status: Some(status),
                    message: if image {
                        "The public URL returns image data."
                    } else {
                        "The URL returned a response but no recognizable image. Check domain routing, login pages and CDN rules."
                    },
                });
            }
            _ => (
                "unexpected_status",
                "The public endpoint returned an unexpected status. Check redirects and domain configuration.",
            ),
        };
        Ok(Check {
            accessible: false,
            code,
            message,
            http_status: Some(status),
        })
    })();
    result.unwrap_or(Check { accessible: false, code: "connection_failed", http_status: None,
        message: "Cannot verify the public URL. Check its scheme, DNS, TLS certificate, proxy and redirect configuration." })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnoses_status_and_rejects_html_without_credentials() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let handle = std::thread::spawn(move || {
            for status in [403, 404, 200] {
                let request = server.recv().unwrap();
                assert!(
                    !request
                        .headers()
                        .iter()
                        .any(|h| h.field.equiv("authorization"))
                );
                request
                    .respond(
                        tiny_http::Response::from_string("<html>Login</html>")
                            .with_status_code(status),
                    )
                    .unwrap();
            }
        });
        for expected in ["access_denied", "not_found", "not_image"] {
            assert_eq!(check(&url, true).code, expected);
        }
        handle.join().unwrap();
    }
}
