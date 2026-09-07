use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub fn image_extension(path: &Path) -> bool {
    path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif"
        )
    })
}
pub fn collect(paths: &[PathBuf], recursive: bool, limit: usize) -> Result<Vec<PathBuf>> {
    let mut seen = BTreeSet::new();
    let mut files = vec![];
    let mut pending: Vec<_> = paths.iter().rev().cloned().collect();
    let mut visited = 0;
    while let Some(path) = pending.pop() {
        visited += 1;
        ensure!(visited <= 100_000, "directory scan exceeds 100,000 entries");
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            ensure!(recursive, "directory input requires --recursive");
            let mut children = std::fs::read_dir(path)?
                .map(|entry| entry.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            children.sort();
            pending.extend(children.into_iter().rev());
        } else if metadata.is_file() && (image_extension(&path) || paths.contains(&path)) {
            let path = path.canonicalize()?;
            if seen.insert(path.clone()) {
                files.push(path);
            }
            ensure!(files.len() <= limit, "image batch exceeds {limit} files");
        }
    }
    Ok(files)
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified: SystemTime,
}
pub fn stamp(path: &Path) -> Result<Stamp> {
    let m = path.metadata()?;
    Ok(Stamp {
        size: m.len(),
        modified: m.modified()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recursive_scan_is_bounded_deterministic_and_ignores_non_images() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        for name in ["b.png", "a.JPG", "sub/c.webp", "sub/note.txt"] {
            std::fs::write(root.path().join(name), b"x").unwrap();
        }
        let roots = [root.path().into()];
        assert!(collect(&roots, false, 10).is_err());
        assert!(collect(&roots, true, 2).is_err());
        let files = collect(&roots, true, 10).unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.windows(2).all(|w| w[0] < w[1]));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.path(), root.path().join("sub/loop")).unwrap();
            assert_eq!(collect(&roots, true, 10).unwrap().len(), 3);
        }
    }
}
