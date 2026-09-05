use anyhow::{Result, ensure};
use chrono::{DateTime, Local};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn validate(p: &str) -> Result<String> {
    ensure!(
        !p.is_empty() && !p.starts_with('/'),
        "remote path must be relative and non-empty"
    );
    ensure!(
        !p.chars().any(char::is_control),
        "remote path contains a control character"
    );
    ensure!(
        !p.split('/').any(|x| x.is_empty() || x == "." || x == ".."),
        "remote path contains an invalid segment"
    );
    Ok(p.into())
}
pub fn generate(
    local: &str,
    data: &[u8],
    template: &str,
    prefix: &str,
    rename: &str,
    now: DateTime<Local>,
) -> Result<String> {
    let local = local.replace('\\', "/");
    let base = local.rsplit('/').next().unwrap_or(&local);
    let ext = Path::new(base)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = Path::new(base)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(base);
    let hash = format!("{:x}", Sha256::digest(data))[..32].to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let suffix = if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    };
    let name = match rename {
        "original" | "" => base.to_string(),
        "hash" => format!("{hash}{suffix}"),
        "uuid" => format!("{id}{suffix}"),
        "timestamp" => format!("{}{suffix}", now.timestamp()),
        _ => anyhow::bail!("unknown rename strategy"),
    };
    let pairs = [
        ("year", now.format("%Y").to_string()),
        ("month", now.format("%m").to_string()),
        ("day", now.format("%d").to_string()),
        ("timestamp", now.format("%Y%m%d-%H%M%S").to_string()),
        ("unix", now.timestamp().to_string()),
        ("filename", name),
        ("stem", stem.to_string()),
        ("ext", ext),
        ("hash", hash),
        ("uuid", id),
    ];
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if let Some(j) = rest.find('}') {
            let key = &rest[1..j];
            out.push_str(
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or(&rest[..=j]),
            );
            rest = &rest[j + 1..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    let out = out.replace('\\', "/");
    validate(&out)?;
    if prefix.is_empty() {
        Ok(out)
    } else {
        let prefix = prefix.replace('\\', "/");
        validate(&prefix)?;
        validate(&format!("{prefix}/{out}"))
    }
}
pub fn escape(p: &str) -> String {
    let mut out = String::new();
    for c in p.bytes() {
        if c.is_ascii_alphanumeric() || b"-._~/".contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_safe_and_substituted_once() {
        let now = Local::now();
        assert!(generate("x.png", b"x", "../{filename}", "", "original", now).is_err());
        assert!(generate("x.png", b"x", "{filename}", "a/../b", "original", now).is_err());
        assert_eq!(
            generate("{year}.png", b"x", "{filename}", "", "original", now).unwrap(),
            "{year}.png"
        );
        assert_eq!(
            escape("目录/a b#%.png"),
            "%E7%9B%AE%E5%BD%95/a%20b%23%25.png"
        );
    }
}
