//! Local Markdown maintenance. Plans, source checks, backups and reports never sync.
mod parser;
use crate::{control::Control, media, network};
use anyhow::{Context, Result, ensure};
use img_records::catalog::{Catalog, digest};
pub use parser::{Reference, scan as image_references};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
const FILE_LIMIT: u64 = 16 << 20;
const SCAN_LIMIT: u64 = 128 << 20;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub old: String,
    pub new: String,
    pub content_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub replacement: String,
    pub old_url: String,
    pub new_url: String,
    pub line: usize,
    pub occurrences: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub relative: PathBuf,
    pub original_hash: String,
    pub output_hash: String,
    pub references: usize,
    pub unmatched: Vec<String>,
    pub changes: Vec<Change>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReferencePlan {
    pub version: u32,
    pub task_id: String,
    pub root: PathBuf,
    pub mappings: Vec<Mapping>,
    pub files: Vec<Document>,
    pub outside_scope: String,
    pub restore_of: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileResult {
    pub relative: PathBuf,
    pub success: bool,
    pub status: String,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub backup: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub plan: ReferencePlan,
    pub files: Vec<FileResult>,
    pub complete: bool,
    pub record_warning: Option<String>,
}
fn url(value: &str) -> Result<String> {
    let parsed = url::Url::parse(value)?;
    ensure!(
        parsed.scheme() == "https"
            && parsed.host_str().is_some()
            && parsed.username().is_empty()
            && parsed.password().is_none(),
        "article mappings require HTTPS image URLs without credentials"
    );
    Ok(parsed.to_string())
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let metadata = path.symlink_metadata()?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "document must be a regular file"
    );
    ensure!(metadata.len() <= FILE_LIMIT, "document exceeds 16 MiB");
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= FILE_LIMIT, "document exceeds 16 MiB");
    Ok(bytes)
}
fn inside(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "document path must stay inside selected directory"
    );
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        ensure!(
            !path.symlink_metadata()?.file_type().is_symlink(),
            "document path contains a symlink"
        );
    }
    ensure!(
        path.canonicalize()?.starts_with(root),
        "document moved outside selected directory"
    );
    Ok(path)
}
fn collect(root: &Path, control: &Control) -> Result<Vec<PathBuf>> {
    let mut queue = vec![root.to_path_buf()];
    let mut files = vec![];
    let mut visited = 0;
    while let Some(directory) = queue.pop() {
        control.check()?;
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            visited += 1;
            ensure!(
                visited <= 100_000,
                "directory exceeds 100000 entries; select a smaller scope"
            );
            let path = entry.path();
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | ".img" | "node_modules")
                ) {
                    queue.push(path)
                }
            } else if kind.is_file()
                && path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
                    })
            {
                files.push(path.strip_prefix(root)?.to_path_buf());
                ensure!(
                    files.len() <= 10000,
                    "select a scope with at most 10000 Markdown files"
                );
            }
        }
    }
    files.sort();
    Ok(files)
}
/// External mapping files are verified by fetching the destination image once per URL.
pub fn verify_mappings(pairs: &[(String, String)], control: &Control) -> Result<Vec<Mapping>> {
    let mut verified = BTreeMap::new();
    let mut mappings = vec![];
    for (old, new) in pairs {
        control.check()?;
        let old = url(old)?;
        let new = url(new)?;
        let hash = if let Some(hash) = verified.get(&new) {
            String::clone(hash)
        } else {
            let bytes = network::fetch(&new, 128 << 20, false)?.data;
            media::detect(&bytes)?;
            let hash = digest(&bytes);
            verified.insert(new.clone(), hash.clone());
            hash
        };
        mappings.push(Mapping {
            old,
            new,
            content_hash: hash,
        });
    }
    Ok(mappings)
}
pub fn migration_mappings(report: &crate::migrate::Report) -> Result<Vec<Mapping>> {
    report
        .files
        .iter()
        .filter(|file| file.success)
        .map(|file| {
            Ok(Mapping {
                old: url(&file.source_url)?,
                new: url(file
                    .url
                    .as_deref()
                    .context("verified migration URL missing")?)?,
                content_hash: file
                    .content_hash
                    .clone()
                    .context("verified migration hash missing")?,
            })
        })
        .collect()
}
impl ReferencePlan {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && uuid::Uuid::parse_str(&self.task_id).is_ok(),
            "unsupported reference plan"
        );
        ensure!(
            self.root.is_absolute() && self.root.canonicalize()? == self.root,
            "selected directory has moved"
        );
        ensure!(
            self.files.len() <= 10000 && self.mappings.len() <= 100000,
            "reference plan exceeds scope limit"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= 32 << 20,
            "reference preview exceeds 32 MiB; select a smaller directory"
        );
        let mut paths = HashSet::new();
        for file in &self.files {
            ensure!(
                paths.insert(&file.relative)
                    && file
                        .relative
                        .components()
                        .all(|part| matches!(part, Component::Normal(_))),
                "invalid or duplicate document path"
            );
            let mut end = 0;
            for change in &file.changes {
                ensure!(
                    change.start >= end
                        && change.end >= change.start
                        && change.end <= FILE_LIMIT as usize,
                    "invalid change range"
                );
                end = change.end;
            }
        }
        Ok(())
    }
}
fn rewrite(original: &str, changes: &[Change]) -> Result<String> {
    let mut result = String::new();
    let mut start = 0;
    for change in changes {
        ensure!(
            change.start >= start
                && original.get(change.start..change.end) == Some(change.original.as_str()),
            "document no longer matches preview"
        );
        result.push_str(
            original
                .get(start..change.start)
                .context("invalid text range")?,
        );
        result.push_str(&change.replacement);
        start = change.end;
        ensure!(
            result.len() <= FILE_LIMIT as usize,
            "modified document exceeds 16 MiB"
        );
    }
    result.push_str(original.get(start..).context("invalid text range")?);
    ensure!(
        result.len() <= FILE_LIMIT as usize,
        "modified document exceeds 16 MiB"
    );
    Ok(result)
}
pub fn scan(root: &Path, mappings: Vec<Mapping>, control: &Control) -> Result<ReferencePlan> {
    let root = root.canonicalize()?;
    ensure!(root.is_dir(), "choose a Markdown directory");
    let mut mapped = BTreeMap::new();
    for mapping in &mappings {
        let old = url(&mapping.old)?;
        let new = url(&mapping.new)?;
        ensure!(
            mapping.content_hash.len() == 64
                && mapping.content_hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "mapping destination has not been verified"
        );
        if let Some(previous) = mapped.insert(old, new.clone()) {
            ensure!(previous == new, "one old link has conflicting destinations");
        }
    }
    let mut files = vec![];
    let mut total = 0u64;
    let mut references_total = 0usize;
    for relative in collect(&root, control)? {
        control.check()?;
        let result = (|| -> Result<Document> {
            let path = inside(&root, &relative)?;
            let bytes = read(&path)?;
            total += bytes.len() as u64;
            ensure!(
                total <= SCAN_LIMIT,
                "scan exceeds 128 MiB; choose a smaller directory"
            );
            let original = std::str::from_utf8(&bytes).context("document is not UTF-8")?;
            let refs = image_references(original);
            references_total += refs.len();
            ensure!(
                references_total <= 100000,
                "scope exceeds 100000 image references; select a smaller directory"
            );
            let mut unmatched = Vec::new();
            let mut changes = Vec::new();
            let references = refs.iter().map(|r| r.occurrences).sum();
            for reference in refs {
                let key = url(&reference.source).unwrap_or_else(|_| reference.source.clone());
                if let Some(new) = mapped.get(&key) {
                    if key != *new {
                        changes.push(Change {
                            start: reference.start,
                            end: reference.end,
                            original: original[reference.start..reference.end].into(),
                            replacement: reference.replacement(new),
                            old_url: reference.source,
                            new_url: new.clone(),
                            line: reference.line,
                            occurrences: reference.occurrences,
                        });
                    }
                } else if !unmatched.contains(&reference.source) {
                    unmatched.push(reference.source);
                }
            }
            let output = rewrite(original, &changes)?;
            Ok(Document {
                relative: relative.clone(),
                original_hash: digest(&bytes),
                output_hash: digest(output.as_bytes()),
                references,
                unmatched,
                changes,
                error: None,
            })
        })();
        ensure!(
            total <= SCAN_LIMIT,
            "scan exceeds 128 MiB; choose a smaller directory"
        );
        ensure!(
            references_total <= 100000,
            "scope exceeds 100000 image references; select a smaller directory"
        );
        files.push(result.unwrap_or_else(|error| Document {
            relative,
            original_hash: String::new(),
            output_hash: String::new(),
            references: 0,
            unmatched: vec![],
            changes: vec![],
            error: Some(error.to_string()),
        }));
    }
    Ok(ReferencePlan {
        version: 1,
        task_id: uuid::Uuid::new_v4().to_string(),
        root,
        mappings,
        files,
        outside_scope: "unknown".into(),
        restore_of: None,
    })
}
fn backup_path(path: &Path, task_id: &str) -> Result<PathBuf> {
    Ok(path.with_file_name(format!(
        "{}.img-backup-{task_id}",
        path.file_name()
            .context("document filename missing")?
            .to_string_lossy()
    )))
}
fn backup(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        ensure!(
            read(path)? == bytes,
            "existing backup differs from original"
        );
        return Ok(());
    }
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().context("backup folder missing")?)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}
/// Save the backup, sync the replacement, then recheck source immediately before atomic replacement.
fn replace(path: &Path, expected: &[u8], changed: &[u8], backup_file: &Path) -> Result<()> {
    ensure!(
        read(path)? == expected,
        "document changed after preview; rescan before applying"
    );
    let permissions = path.metadata()?.permissions();
    backup(backup_file, expected)?;
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().context("document folder missing")?)?;
    temp.write_all(changed)?;
    temp.as_file().set_permissions(permissions)?;
    temp.as_file().sync_all()?;
    ensure!(
        read(path)? == expected,
        "document changed while preparing backup; original kept"
    );
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}
fn store(catalog: &Catalog, report: &Report) -> Result<()> {
    catalog.save_task(
        &format!("references:{}", report.plan.task_id),
        "references",
        &serde_json::to_string(report)?,
    )
}
pub fn save_plan(catalog: &Catalog, plan: &ReferencePlan) -> Result<()> {
    plan.validate()?;
    ensure!(
        catalog
            .task_optional(&format!("references:{}", plan.task_id))?
            .is_none(),
        "reference plan already exists"
    );
    store(
        catalog,
        &Report {
            plan: plan.clone(),
            files: vec![],
            complete: false,
            record_warning: None,
        },
    )
}
pub fn load(catalog: &Catalog, id: &str) -> Result<Report> {
    ensure!(
        uuid::Uuid::parse_str(id).is_ok(),
        "invalid reference task ID"
    );
    let report: Report = serde_json::from_str(&catalog.task(&format!("references:{id}"))?)?;
    ensure!(
        report.plan.task_id == id,
        "reference task identity mismatch"
    );
    report.plan.validate()?;
    Ok(report)
}
pub fn apply(catalog: &Catalog, plan: &ReferencePlan, control: &Control) -> Result<Report> {
    apply_verified(catalog, plan, control, |mapping| {
        let bytes = network::fetch(&url(&mapping.new)?, 128 << 20, false)?.data;
        media::detect(&bytes)?;
        ensure!(
            digest(&bytes) == mapping.content_hash,
            "destination image has changed since migration; create a new preview"
        );
        Ok(())
    })
}
fn apply_verified(
    catalog: &Catalog,
    plan: &ReferencePlan,
    control: &Control,
    mut verify: impl FnMut(&Mapping) -> Result<()>,
) -> Result<Report> {
    plan.validate()?;
    let _lease =
        img_records::remote_lock::acquire(&catalog.root, "references-task", &plan.task_id, true)?;
    let mut report = match catalog.task_optional(&format!("references:{}", plan.task_id))? {
        Some(body) => {
            let saved: Report = serde_json::from_str(&body)?;
            ensure!(saved.plan == *plan, "saved reference plan differs");
            saved
        }
        None => Report {
            plan: plan.clone(),
            files: vec![],
            complete: false,
            record_warning: None,
        },
    };
    report.record_warning = None;
    store(catalog, &report)?;
    let mut verified = BTreeMap::<String, Result<(), String>>::new();
    for document in &plan.files {
        control.check()?;
        if document.changes.is_empty() {
            continue;
        }
        let result = (|| -> Result<PathBuf> {
            ensure!(document.error.is_none(), "document scan failed");
            let path = inside(&plan.root, &document.relative)?;
            let _lease = img_records::remote_lock::acquire(
                &catalog.root,
                "reference-file",
                path.to_str().context("document path is not UTF-8")?,
                true,
            )?;
            let bytes = read(&path)?;
            let backup_file = backup_path(&path, &plan.task_id)?;
            if digest(&bytes) == document.output_hash {
                ensure!(
                    digest(&read(&backup_file)?) == document.original_hash,
                    "modified document lacks a valid backup"
                );
                return Ok(backup_file);
            }
            ensure!(
                digest(&bytes) == document.original_hash,
                "document changed after preview; rescan before applying"
            );
            if plan.restore_of.is_none() {
                for change in &document.changes {
                    let mapping = plan
                        .mappings
                        .iter()
                        .find(|mapping| {
                            mapping.new == change.new_url
                                && mapping.old
                                    == url(&change.old_url)
                                        .unwrap_or_else(|_| change.old_url.clone())
                        })
                        .context("change is not backed by a verified mapping")?;
                    let validation = verified
                        .entry(mapping.new.clone())
                        .or_insert_with(|| verify(mapping).map_err(|error| error.to_string()));
                    if let Err(error) = validation {
                        anyhow::bail!(
                            "new link could not be verified; original references kept: {error}"
                        );
                    }
                    control.check()?;
                }
            }
            let output = rewrite(std::str::from_utf8(&bytes)?, &document.changes)?;
            ensure!(
                digest(output.as_bytes()) == document.output_hash,
                "preview content hash mismatch"
            );
            control.check()?;
            replace(&path, &bytes, output.as_bytes(), &backup_file)?;
            Ok(backup_file)
        })();
        let success = result.is_ok();
        let (backup, error) = match result {
            Ok(path) => (Some(path), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let item = FileResult {
            relative: document.relative.clone(),
            success,
            status: if success { "complete" } else { "failed" }.into(),
            error,
            error_code: (!success).then(|| "document_write_failed".into()),
            backup,
        };
        if let Some(index) = report
            .files
            .iter()
            .position(|file| file.relative == document.relative)
        {
            report.files[index] = item
        } else {
            report.files.push(item)
        }
        if let Err(error) = store(catalog, &report) {
            report.record_warning = Some(format!(
                "document results could not be saved: {error}; backups remain beside their documents"
            ));
            return Ok(report);
        }
    }
    report.complete = plan.files.iter().all(|file| {
        file.error.is_none()
            && (file.changes.is_empty()
                || report
                    .files
                    .iter()
                    .any(|r| r.relative == file.relative && r.success))
    });
    store(catalog, &report)?;
    Ok(report)
}
pub fn restore_plan(report: &Report, control: &Control) -> Result<ReferencePlan> {
    report.plan.validate()?;
    let mut files = vec![];
    for original in &report.plan.files {
        control.check()?;
        if original.changes.is_empty() {
            continue;
        }
        let path = inside(&report.plan.root, &original.relative)?;
        let backup_file = backup_path(&path, &report.plan.task_id)?;
        if !backup_file.exists() {
            continue;
        }
        let backup = read(&backup_file)?;
        ensure!(
            digest(&backup) == original.original_hash,
            "backup was modified; refusing restoration"
        );
        let current = read(&path)?;
        ensure!(
            digest(&current) == original.output_hash,
            "document changed since repair; rescan before restoring"
        );
        files.push(Document {
            relative: original.relative.clone(),
            original_hash: original.output_hash.clone(),
            output_hash: original.original_hash.clone(),
            references: 0,
            unmatched: vec![],
            changes: vec![Change {
                start: 0,
                end: current.len(),
                original: String::from_utf8(current)?,
                replacement: String::from_utf8(backup)?,
                old_url: String::new(),
                new_url: String::new(),
                line: 1,
                occurrences: 1,
            }],
            error: None,
        });
    }
    ensure!(
        !files.is_empty(),
        "no applied document backups available to restore"
    );
    Ok(ReferencePlan {
        version: 1,
        task_id: uuid::Uuid::new_v4().to_string(),
        root: report.plan.root.clone(),
        mappings: vec![],
        files,
        outside_scope: "unknown".into(),
        restore_of: Some(report.plan.task_id.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Catalog, Vec<Mapping>) {
        let root = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(&root.path().join("data")).unwrap();
        std::fs::create_dir(root.path().join("articles")).unwrap();
        let mappings = vec![Mapping {
            old: "https://old.test/a.png".into(),
            new: "https://new.test/a.png".into(),
            content_hash: digest(b"verified bytes"),
        }];
        (root, catalog, mappings)
    }
    #[test]
    fn preview_apply_retry_and_restore_preserve_documents_and_each_backup() {
        let (root, catalog, mappings) = fixture();
        let articles = root.path().join("articles");
        let path = articles.join("article.md");
        let original = "![a][pic]\n\n[pic]: https://old.test/a.png 'title'\n\n![](https://unmatched.test/b.png)\n";
        std::fs::write(&path, original).unwrap();
        let plan = scan(&articles, mappings, &Control::default()).unwrap();
        assert_eq!(plan.outside_scope, "unknown");
        assert_eq!(plan.files[0].references, 2);
        assert_eq!(plan.files[0].unmatched.len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        save_plan(&catalog, &plan).unwrap();
        let mut checks = 0;
        let first = apply_verified(&catalog, &plan, &Control::default(), |_| {
            checks += 1;
            Ok(())
        })
        .unwrap();
        assert!(first.complete);
        assert_eq!(checks, 1);
        let backup = first.files[0].backup.as_ref().unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
        let changed = std::fs::read_to_string(&path).unwrap();
        assert!(changed.contains("https://new.test/a.png"));
        assert!(changed.contains("https://unmatched.test/b.png"));
        let again = apply_verified(&catalog, &plan, &Control::default(), |_| {
            panic!("complete file should be reused")
        })
        .unwrap();
        assert!(again.complete);
        assert_eq!(std::fs::read_dir(&articles).unwrap().count(), 2);
        let restore = restore_plan(&again, &Control::default()).unwrap();
        let restored = apply_verified(&catalog, &restore, &Control::default(), |_| {
            panic!("restore is local")
        })
        .unwrap();
        assert!(restored.complete);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(restored.files[0].backup.as_ref().unwrap()).unwrap(),
            changed
        );
        assert!(catalog.sync_events(None).unwrap().is_empty());
    }
    #[test]
    fn changed_document_and_backup_failure_keep_original_with_partial_success() {
        let (root, catalog, mappings) = fixture();
        let articles = root.path().join("articles");
        let original = "![](https://old.test/a.png)";
        for name in ["a.md", "b.markdown", "c.md"] {
            std::fs::write(articles.join(name), original).unwrap();
        }
        let plan = scan(&articles, mappings, &Control::default()).unwrap();
        std::fs::write(articles.join("a.md"), "edited after preview").unwrap();
        std::fs::create_dir(backup_path(&articles.join("b.markdown"), &plan.task_id).unwrap())
            .unwrap();
        let report = apply_verified(&catalog, &plan, &Control::default(), |_| Ok(())).unwrap();
        assert!(!report.complete);
        assert_eq!(report.files.iter().filter(|f| f.success).count(), 1);
        assert_eq!(
            std::fs::read_to_string(articles.join("a.md")).unwrap(),
            "edited after preview"
        );
        assert_eq!(
            std::fs::read_to_string(articles.join("b.markdown")).unwrap(),
            original
        );
        std::fs::write(articles.join("c.md"), "edited after apply").unwrap();
        assert!(restore_plan(&report, &Control::default()).is_err());
    }
    #[test]
    fn cancellation_persistence_failure_and_unverified_destination_never_write_articles() {
        let (root, catalog, mappings) = fixture();
        let articles = root.path().join("articles");
        let path = articles.join("a.md");
        let original = "![](https://old.test/a.png)";
        std::fs::write(&path, original).unwrap();
        let plan = scan(&articles, mappings, &Control::default()).unwrap();
        let cancelled = Control::default();
        cancelled.cancel();
        assert!(
            apply_verified(&catalog, &plan, &cancelled, |_| panic!(
                "no verification after cancel"
            ))
            .is_err()
        );
        let report = apply_verified(&catalog, &plan, &Control::default(), |_| {
            anyhow::bail!("TLS or hash validation failed")
        })
        .unwrap();
        assert!(!report.complete);
        assert_eq!(std::fs::read_dir(&articles).unwrap().count(), 1);
        let db = rusqlite::Connection::open(catalog.root.join("catalog.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER fail_reference_save BEFORE INSERT ON tasks BEGIN SELECT RAISE(ABORT, 'injected persistence failure'); END;").unwrap();
        assert!(
            apply_verified(&catalog, &plan, &Control::default(), |_| panic!(
                "save must precede verification"
            ))
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }
    #[cfg(unix)]
    #[test]
    fn symlink_scope_and_replaced_document_are_never_followed() {
        let (root, catalog, mappings) = fixture();
        let articles = root.path().join("articles");
        let outside = root.path().join("outside.md");
        std::fs::write(&outside, "![](https://old.test/a.png)").unwrap();
        std::os::unix::fs::symlink(&outside, articles.join("symlink.md")).unwrap();
        assert!(
            scan(&articles, mappings.clone(), &Control::default())
                .unwrap()
                .files
                .is_empty()
        );
        let path = articles.join("real.md");
        std::fs::write(&path, "![](https://old.test/a.png)").unwrap();
        let plan = scan(&articles, mappings, &Control::default()).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        let report = apply_verified(&catalog, &plan, &Control::default(), |_| {
            panic!("symlink validation precedes network")
        })
        .unwrap();
        assert!(!report.complete);
        assert_eq!(
            std::fs::read_to_string(outside).unwrap(),
            "![](https://old.test/a.png)"
        );
    }
}
