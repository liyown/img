use crate::upload::FileResult;
use anyhow::Result;
pub fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}
pub fn format_url(raw: &str, format: &str) -> String {
    match format {
        "markdown" => {
            let alt = url::Url::parse(raw)
                .ok()
                .and_then(|u| {
                    u.path_segments()
                        .and_then(|mut p| p.next_back())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let alt = crate::network::percent_decode(&alt).replace(['[', ']', '\r', '\n'], "");
            let safe = raw
                .replace('\\', "\\\\")
                .replace(')', "\\)")
                .replace(['\r', '\n'], "");
            format!("![{alt}]({safe})")
        }
        "html" => format!(
            "<img src=\"{}\" alt=\"\">",
            raw.replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\'', "&#39;")
        ),
        _ => raw.into(),
    }
}
pub fn render(format: &str, results: &[FileResult], clipboard: bool) -> Result<String> {
    if format == "json" {
        return Ok(serde_json::to_string_pretty(
            &serde_json::json!({"success":!results.is_empty()&&results.iter().all(|r|r.success),"files":results}),
        )?);
    }
    Ok(results
        .iter()
        .filter_map(|r| {
            if r.success {
                Some(format_url(&r.url, format))
            } else if !clipboard {
                Some(format!(
                    "Error: {}: {}",
                    clean(&r.local_path),
                    clean(&r.error)
                ))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_escapes_and_keeps_partial_results() {
        assert_eq!(
            format_url("https://e.test/a.png?x=1&y=2", "html"),
            "<img src=\"https://e.test/a.png?x=1&amp;y=2\" alt=\"\">"
        );
        let r = vec![
            FileResult {
                success: true,
                url: "https://e.test/a.png".into(),
                ..Default::default()
            },
            FileResult {
                local_path: "bad.png".into(),
                error: "failed".into(),
                ..Default::default()
            },
        ];
        let doc: serde_json::Value =
            serde_json::from_str(&render("json", &r, false).unwrap()).unwrap();
        assert_eq!(doc["success"], false);
        assert_eq!(doc["files"].as_array().unwrap().len(), 2);
        assert_eq!(render("url", &r, true).unwrap(), "https://e.test/a.png");
    }
}
