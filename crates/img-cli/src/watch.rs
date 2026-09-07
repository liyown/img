use anyhow::{Result, ensure};
use img_core::{
    config::Upload,
    control::Control,
    provider::Provider,
    upload::{self, Options},
};
use std::{collections::HashMap, path::Path, time::Duration};

pub fn run(
    directory: &Path,
    provider: &Provider,
    config: &Upload,
    options: &Options,
    interval: u64,
    new_only: bool,
    control: &Control,
) -> Result<i32> {
    ensure!(directory.is_dir(), "watch requires an existing directory");
    let roots = [directory.to_path_buf()];
    let mut previous = HashMap::new();
    let mut uploaded = HashMap::new();
    if new_only {
        for path in img_records::files::collect(&roots, true, 10_000)? {
            uploaded.insert(path.clone(), img_records::files::stamp(&path)?);
        }
    }
    loop {
        control.check()?;
        let paths = img_records::files::collect(&roots, true, 10_000)?;
        let mut observed = HashMap::new();
        for path in paths {
            let Ok(stamp) = img_records::files::stamp(&path) else {
                continue;
            };
            if ready(previous.get(&path), uploaded.get(&path), &stamp) {
                let results = upload::run(
                    provider,
                    config,
                    &[path.to_string_lossy().into_owned()],
                    options,
                    control,
                );
                println!("{}", img_core::output::render("json", &results, false)?);
                if results.iter().all(|r| r.success) {
                    uploaded.insert(path.clone(), stamp);
                }
            }
            observed.insert(path, stamp);
        }
        uploaded.retain(|p, _| observed.contains_key(p));
        previous = observed;
        control.delay(Duration::from_secs(interval))?;
    }
}
fn ready(
    previous: Option<&img_records::files::Stamp>,
    uploaded: Option<&img_records::files::Stamp>,
    now: &img_records::files::Stamp,
) -> bool {
    now.size > 0 && previous == Some(now) && uploaded != Some(now)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_stable_unuploaded_versions_are_ready() {
        let first = img_records::files::Stamp {
            size: 1,
            modified: std::time::UNIX_EPOCH,
        };
        let second = img_records::files::Stamp { size: 2, ..first };
        assert!(!ready(None, None, &first));
        assert!(!ready(Some(&first), None, &second));
        assert!(ready(Some(&second), None, &second));
        assert!(!ready(Some(&second), Some(&second), &second));
    }
}
