use anyhow::{Result, ensure};
use img_core::{
    config::Upload,
    control::Control,
    network, output,
    provider::Provider,
    upload::{self, Options},
};
use regex::Regex;
use std::{collections::HashMap, path::Path, sync::LazyLock};
static MARKDOWN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\(\s*(<[^>]*>|[^)\s]+)").unwrap());
static HTML: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap());
static CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^\s*```[^\n]*\n.*?^\s*```[^\n]*(?:\n|$)|`[^`\n]*`").unwrap()
});
#[derive(Debug)]
struct Ref {
    start: usize,
    end: usize,
    source: String,
}
fn references(doc: &str) -> Vec<Ref> {
    let excluded = CODE
        .find_iter(doc)
        .map(|m| m.start()..m.end())
        .collect::<Vec<_>>();
    let mut refs = vec![];
    for cap in MARKDOWN.captures_iter(doc) {
        let m = cap.get(1).unwrap();
        let (start, end) = if m.as_str().starts_with('<') {
            (m.start() + 1, m.end() - 1)
        } else {
            (m.start(), m.end())
        };
        refs.push(Ref {
            start,
            end,
            source: doc[start..end].into(),
        });
    }
    for cap in HTML.captures_iter(doc) {
        let m = cap.get(1).or_else(|| cap.get(2)).unwrap();
        refs.push(Ref {
            start: m.start(),
            end: m.end(),
            source: m.as_str().into(),
        });
    }
    refs.retain(|r| {
        replaceable(&r.source) && !excluded.iter().any(|range| range.contains(&r.start))
    });
    refs.sort_by_key(|r| r.start);
    refs
}
fn replaceable(src: &str) -> bool {
    if src.is_empty() || src.starts_with('#') || src.starts_with("//") {
        return false;
    }
    if src.as_bytes().get(1) == Some(&b':')
        && src
            .as_bytes()
            .get(2)
            .is_some_and(|b| *b == b'\\' || *b == b'/')
    {
        return true;
    }
    if let Some((scheme, _)) = src.split_once(':')
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return scheme == "http" || scheme == "https";
    }
    true
}
fn apply(doc: &str, refs: &[Ref], mapping: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut pos = 0;
    for r in refs {
        if r.start < pos {
            continue;
        }
        out.push_str(&doc[pos..r.start]);
        out.push_str(
            mapping
                .get(&r.source)
                .filter(|s| !s.is_empty())
                .map(String::as_str)
                .unwrap_or(&doc[r.start..r.end]),
        );
        pos = r.end;
    }
    out.push_str(&doc[pos..]);
    out
}
pub fn rewrite(
    doc: &str,
    dir: &Path,
    p: &Provider,
    c: &Upload,
    o: &Options,
    control: &Control,
) -> Result<(String, usize, usize, Vec<upload::FileResult>)> {
    let refs = references(doc);
    let mut original = vec![];
    for r in &refs {
        if !original.contains(&r.source) {
            original.push(r.source.clone());
        }
    }
    let files = original
        .iter()
        .map(|s| {
            if network::is_url(s) || Path::new(s).is_absolute() {
                s.clone()
            } else {
                dir.join(s).to_string_lossy().into_owned()
            }
        })
        .collect::<Vec<_>>();
    let results = upload::run(p, c, &files, o, control);
    let mut mapping = HashMap::new();
    let mut ok = 0;
    let mut failed = 0;
    for (src, r) in original.into_iter().zip(&results) {
        if r.success {
            mapping.insert(src, r.url.clone());
            ok += 1;
        } else {
            eprintln!(
                "  {}: {} (kept original)",
                output::clean(&src),
                output::clean(&r.error)
            );
            failed += 1;
        }
    }
    Ok((apply(doc, &refs, &mapping), ok, failed, results))
}
pub fn preview(doc: &str, dir: &Path) -> Vec<serde_json::Value> {
    references(doc)
        .into_iter()
        .map(|r| {
            let remote = network::is_url(&r.source);
            let path = dir.join(&r.source);
            serde_json::json!({"source":r.source,"remote":remote,"exists":remote || path.is_file()})
        })
        .collect()
}
pub fn replace_with_backup(
    file: &Path,
    expected: &[u8],
    changed: &[u8],
) -> Result<std::path::PathBuf> {
    use std::io::Write;
    ensure!(
        std::fs::read(file)? == expected,
        "document changed during upload; refusing to overwrite edits"
    );
    let backup = file.with_file_name(format!(
        "{}.img-backup-{}",
        file.file_name().unwrap().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let mut saved = tempfile::NamedTempFile::new_in(
        file.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    saved.write_all(expected)?;
    saved.as_file().sync_all()?;
    saved.persist_noclobber(&backup).map_err(|e| e.error)?;
    let permissions = file.metadata()?.permissions();
    img_core::config::write_atomic(file, changed)?;
    std::fs::set_permissions(file, permissions)?;
    Ok(backup)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_preserves_original_and_rejects_concurrent_edits() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("article.md");
        std::fs::write(&file, b"original").unwrap();
        let backup = replace_with_backup(&file, b"original", b"rewritten").unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), b"original");
        assert_eq!(std::fs::read(&file).unwrap(), b"rewritten");
        assert!(replace_with_backup(&file, b"original", b"lost edit").is_err());
        let undo =
            replace_with_backup(&file, b"rewritten", &std::fs::read(backup).unwrap()).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"original");
        assert_eq!(std::fs::read(undo).unwrap(), b"rewritten");
    }
    #[test]
    fn preserves_prose_titles_alt_and_code() {
        let doc = "The file (a.png) stays. ![alt](a.png \"title\") <img src='b.jpg' alt='a.png'> `![](skip.png)`\n```md\n![](code.png)\n```\n![](<path with spaces.png>)";
        let refs = references(doc);
        assert_eq!(
            refs.iter().map(|r| r.source.as_str()).collect::<Vec<_>>(),
            ["a.png", "b.jpg", "path with spaces.png"]
        );
        let out = apply(
            doc,
            &refs,
            &HashMap::from([("a.png".into(), "https://cdn.test/a.png".into())]),
        );
        assert!(out.contains("(a.png) stays"));
        assert!(out.contains("![alt](https://cdn.test/a.png \"title\")"));
        assert!(out.contains("alt='a.png'"));
    }
    #[test]
    fn skips_non_http_schemes_and_data() {
        for s in [
            "",
            "#anchor",
            "data:image/png;base64,a",
            "//e/a.png",
            "ftp://e/a.png",
        ] {
            assert!(!replaceable(s));
        }
        assert!(replaceable("C:\\images\\a.png"));
    }
}
