use anyhow::{Context, Result, ensure};
use reqwest::{
    blocking::{Client, Response},
    redirect::Policy,
};
use std::{io::Read, time::Duration};
use url::Url;

pub fn secure_url(raw: &str, allow_http: bool) -> Result<Url> {
    ensure!(
        !raw.chars().any(char::is_control),
        "URL contains a control character"
    );
    let u = Url::parse(raw).context("URL must be absolute")?;
    ensure!(
        u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.fragment().is_none(),
        "URL must have a host and no credentials or fragment"
    );
    ensure!(
        u.scheme() == "https" || (allow_http && u.scheme() == "http"),
        "URL must use HTTPS (allow insecure HTTP only for trusted endpoints)"
    );
    Ok(u)
}
pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("img/", env!("CARGO_PKG_VERSION")))
        .redirect(Policy::custom(|a| {
            if a.previous().len() >= 5 {
                return a.error("too many redirects");
            }
            if let Some(first) = a.previous().first()
                && (first.origin() != a.url().origin()
                    || a.url().username() != ""
                    || a.url().password().is_some())
            {
                return a.error("refusing cross-origin redirect");
            }
            a.follow()
        }))
        .build()?)
}
pub fn bounded(mut r: Response, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        r.content_length().is_none_or(|n| n <= limit),
        "response exceeds maximum size of {limit} bytes"
    );
    let mut data = Vec::new();
    (&mut r).take(limit + 1).read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= limit,
        "response exceeds maximum size of {limit} bytes"
    );
    Ok(data)
}
pub fn is_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}
pub struct Fetched {
    pub data: Vec<u8>,
    pub name: String,
}
pub fn fetch(source: &str, max_size: u64, allow_http: bool) -> Result<Fetched> {
    ensure!(
        (1..=128 << 20).contains(&max_size),
        "invalid maximum download size"
    );
    let u = secure_url(source, allow_http)?;
    let r = client()?
        .get(u.clone())
        .send()
        .context("cannot download image")?;
    ensure!(
        r.status().is_success(),
        "image download failed with HTTP {}",
        r.status().as_u16()
    );
    let content_type = r
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("");
    let mut name = u
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("")
        .to_string();
    name = percent_decode(&name);
    name = name.replace(['/', '\\', '\r', '\n'], "_");
    if name.is_empty() || name == "." || name == ".." {
        name = "download".into();
    }
    if !name.contains('.') {
        name.push_str(match content_type {
            "image/png" => ".png",
            "image/jpeg" => ".jpg",
            "image/gif" => ".gif",
            "image/webp" => ".webp",
            "image/avif" => ".avif",
            "image/svg+xml" => ".svg",
            _ => "",
        });
    }
    let data = bounded(r, max_size)?;
    ensure!(!data.is_empty(), "image download is empty");
    Ok(Fetched { data, name })
}
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(a), Some(b)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push((a * 16 + b) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
