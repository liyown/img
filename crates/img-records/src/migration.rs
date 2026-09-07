//! PicGo/PicList built-in configuration import. No plugins or scripts execute.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct Candidate {
    pub name: String,
    pub kind: String,
    pub values: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}
pub struct Plan {
    pub candidates: Vec<Candidate>,
    pub skipped: Vec<String>,
}

pub fn parse(bytes: &[u8], existing: &[String]) -> Result<Plan> {
    ensure!(bytes.len() <= 16 << 20, "configuration exceeds 16 MiB");
    let doc: Value = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("invalid configuration JSON; source preserved"))?;
    let beds = doc
        .get("picBed")
        .and_then(Value::as_object)
        .context("missing picBed configuration")?;
    let mut names: BTreeSet<_> = existing.iter().cloned().collect();
    let mut plan = Plan {
        candidates: vec![],
        skipped: vec![],
    };
    for (kind, data) in beds {
        if matches!(kind.as_str(), "current" | "uploader" | "list") {
            continue;
        }
        let rows: Vec<&Value> = if let Some(rows) = data.as_array() {
            rows.iter().collect()
        } else if let Some(rows) = data.get("configList").and_then(Value::as_array) {
            rows.iter().collect()
        } else {
            vec![data]
        };
        for row in rows {
            if !matches!(kind.as_str(), "github" | "aliyun") {
                plan.skipped
                    .push(format!("{kind}: unsupported provider; source preserved"));
                continue;
            }
            let get = |key: &str| {
                row.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };
            let label = get("_configName");
            let base = if label.is_empty() {
                format!("imported-{kind}")
            } else {
                label
            };
            ensure!(
                base.len() <= 80 && !base.chars().any(char::is_control),
                "invalid imported provider name"
            );
            let mut name = base.clone();
            let mut n = 2;
            while names.contains(&name) {
                name = format!("{base}-{n}");
                n += 1;
            }
            names.insert(name.clone());
            let mut values = BTreeMap::new();
            let target = if kind == "github" {
                let repo = get("repo");
                let Some((owner, repo)) = repo.split_once('/') else {
                    plan.skipped
                        .push(format!("{name}: missing owner/repository"));
                    continue;
                };
                for (key, value) in [
                    ("owner", owner.into()),
                    ("repo", repo.into()),
                    ("token", get("token")),
                    (
                        "branch",
                        if get("branch").is_empty() {
                            "main".into()
                        } else {
                            get("branch")
                        },
                    ),
                    ("public_url", get("customUrl")),
                ] {
                    values.insert(key.into(), value);
                }
                "github"
            } else {
                let area = get("area");
                let area = area.trim_start_matches("oss-");
                let bucket = get("bucket");
                let domain = get("customUrl");
                for (key, value) in [
                    ("endpoint", format!("https://oss-{area}.aliyuncs.com")),
                    ("region", area.to_owned()),
                    ("bucket", bucket.clone()),
                    ("access_key", get("accessKeyId")),
                    ("secret_key", get("accessKeySecret")),
                    (
                        "public_url",
                        if domain.is_empty() {
                            format!("https://{bucket}.oss-{area}.aliyuncs.com")
                        } else {
                            domain
                        },
                    ),
                ] {
                    values.insert(key.into(), value);
                }
                "oss"
            };
            values.insert("path_prefix".into(), get("path").trim_matches('/').into());
            let mut warnings = vec![];
            if name != base {
                warnings.push(format!("Name already exists; imported as {name}"));
            }
            if values
                .get("public_url")
                .is_some_and(|v| v.starts_with("http://"))
            {
                warnings.push("Public URL uses HTTP; review before saving".into());
            }
            if !get("options").is_empty() || !get("slim").is_empty() {
                warnings.push("Image processing URL suffix was not imported".into());
            }
            plan.candidates.push(Candidate {
                name,
                kind: target.into(),
                values,
                warnings,
            });
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_preserves_source_renames_collisions_and_reports_unsupported() {
        let bytes = br#"{"picBed":{"github":[{"_configName":"photos","repo":"me/img","token":"test-secret","path":"posts/"}],"qiniu":{}}}"#;
        let plan = parse(bytes, &["photos".into()]).unwrap();
        assert_eq!(plan.candidates[0].name, "photos-2");
        assert_eq!(plan.candidates[0].values["path_prefix"], "posts");
        assert_eq!(plan.skipped.len(), 1);
        assert!(
            parse(b"{secret invalid", &[])
                .err()
                .unwrap()
                .to_string()
                .find("secret")
                .is_none()
        );
    }
}
