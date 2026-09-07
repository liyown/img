//! Stable, deliberately small error vocabulary shared by machine consumers.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidConfig,
    Authentication,
    Permission,
    Network,
    Timeout,
    RateLimited,
    Conflict,
    TooLarge,
    InvalidImage,
    FileNotFound,
    Io,
    Server,
    InvalidResponse,
    Cancelled,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub code: ErrorCode,
    pub http_status: Option<u16>,
    pub retryable: bool,
}
impl Failure {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            http_status: None,
            retryable: matches!(
                code,
                ErrorCode::Network
                    | ErrorCode::Timeout
                    | ErrorCode::RateLimited
                    | ErrorCode::Server
            ),
        }
    }
    pub fn http(status: u16) -> Self {
        use ErrorCode::*;
        let code = match status {
            401 => Authentication,
            403 => Permission,
            408 | 504 => Timeout,
            409 | 412 => Conflict,
            413 => TooLarge,
            429 => RateLimited,
            500..=599 => Server,
            _ => InvalidResponse,
        };
        Self {
            http_status: Some(status),
            ..Self::new(code)
        }
    }
    pub fn from_error(error: &anyhow::Error, fallback: ErrorCode) -> Self {
        if let Some(e) = error.downcast_ref::<crate::provider::UploadError>() {
            return e.failure.clone();
        }
        if let Some(e) = error.downcast_ref::<reqwest::Error>() {
            return if let Some(status) = e.status() {
                Self::http(status.as_u16())
            } else {
                Self::new(if e.is_timeout() {
                    ErrorCode::Timeout
                } else {
                    ErrorCode::Network
                })
            };
        }
        if let Some(e) = error.downcast_ref::<std::io::Error>() {
            return Self::new(match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::FileNotFound,
                std::io::ErrorKind::PermissionDenied => ErrorCode::Permission,
                std::io::ErrorKind::TimedOut => ErrorCode::Timeout,
                _ => ErrorCode::Io,
            });
        }
        if error.downcast_ref::<image::ImageError>().is_some() {
            return Self::new(ErrorCode::InvalidImage);
        }
        // Only classify trusted local messages; never return their contents or a server body.
        let text = error.to_string().to_lowercase();
        let code = if text.contains("already exists") {
            ErrorCode::Conflict
        } else if text.contains("cancelled") {
            ErrorCode::Cancelled
        } else if text.contains("exceeds") || text.contains("maximum size") {
            ErrorCode::TooLarge
        } else if text.contains("supported image") || text.contains("image is empty") {
            ErrorCode::InvalidImage
        } else {
            fallback
        };
        Self::new(code)
    }
    pub fn message(&self) -> &'static str {
        use ErrorCode::*;
        match self.code {
            InvalidConfig => "Invalid configuration; check the selected storage provider",
            Authentication => "Authentication failed; check your credentials",
            Permission => "Access denied; check storage and file permissions",
            Network => "Network connection failed",
            Timeout => "Request timed out",
            RateLimited => "Service rate limit reached; try again later",
            Conflict => "Remote file already exists; choose another name or enable overwrite",
            TooLarge => "Image or response exceeds the size limit",
            InvalidImage => "Image is empty, damaged or unsupported",
            FileNotFound => "Source image was not found; select the original again",
            Io => "Could not read or write a local file",
            Server => "Storage service is temporarily unavailable",
            InvalidResponse => "Storage returned an invalid response or public URL",
            Cancelled => "Upload cancelled",
            Unknown => "Upload failed; inspect the diagnostic details",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_classification_is_stable_and_never_copies_secrets() {
        for (status, code, retry) in [
            (401, ErrorCode::Authentication, false),
            (403, ErrorCode::Permission, false),
            (429, ErrorCode::RateLimited, true),
            (503, ErrorCode::Server, true),
            (408, ErrorCode::Timeout, true),
            (412, ErrorCode::Conflict, false),
        ] {
            let f = Failure::http(status);
            assert_eq!(
                (f.code, f.retryable, f.http_status),
                (code, retry, Some(status))
            );
        }
        let e =
            anyhow::anyhow!("bad config token=private-secret https://host/?secret=private-secret");
        let f = Failure::from_error(&e, ErrorCode::InvalidConfig);
        assert!(!f.message().contains("private-secret"));
    }
}
