use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryView {
    #[default]
    Grid,
    List,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyFormat {
    #[default]
    Url,
    MarkdownImage,
    MarkdownLink,
    Html,
    Bbcode,
}

impl CopyFormat {
    pub const ALL: [Self; 5] = [
        Self::Url,
        Self::MarkdownImage,
        Self::MarkdownLink,
        Self::Html,
        Self::Bbcode,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Url => "纯链接",
            Self::MarkdownImage => "Markdown 图片",
            Self::MarkdownLink => "Markdown 链接",
            Self::Html => "HTML 图片",
            Self::Bbcode => "BBCode",
        }
    }

    pub fn render(self, name: &str, url: &str) -> String {
        if self == Self::Url {
            return url.to_owned();
        }
        let name = name.replace(['\r', '\n'], " ");
        // Encode markup delimiters without changing already encoded URLs.
        let safe_url: String = url
            .chars()
            .map(|ch| match ch {
                '\\' | '(' | ')' | '<' | '>' | '[' | ']' | '"' | '\'' | ' ' => {
                    format!("%{:02X}", ch as u32)
                }
                ch if ch.is_ascii_control() => format!("%{:02X}", ch as u32),
                ch => ch.to_string(),
            })
            .collect();
        match self {
            Self::Url => unreachable!(),
            Self::MarkdownImage | Self::MarkdownLink => {
                let alt = name
                    .replace('\\', "\\\\")
                    .replace('[', "\\[")
                    .replace(']', "\\]");
                let prefix = if self == Self::MarkdownImage { "!" } else { "" };
                format!("{prefix}[{alt}]({safe_url})")
            }
            Self::Html => format!(
                "<img src=\"{}\" alt=\"{}\">",
                html_escape(&safe_url),
                html_escape(&name)
            ),
            Self::Bbcode => format!("[img]{safe_url}[/img]"),
        }
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub library_view: LibraryView,
    pub copy_format: CopyFormat,
    pub auto_copy: bool,
    pub sidebar_collapsed: bool,
    pub check_updates: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            library_view: LibraryView::Grid,
            copy_format: CopyFormat::Url,
            auto_copy: true,
            sidebar_collapsed: false,
            check_updates: false,
        }
    }
}

impl Preferences {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("preferences.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn save(self, root: &Path) -> Result<()> {
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&serde_json::to_vec_pretty(&self)?)?;
        file.as_file().sync_all()?;
        file.persist(root.join("preferences.json"))
            .map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_formats_preserve_url_and_escape_markup() {
        let url = "https://cdn.example.com/a(1).png?x=1&y=2";
        assert_eq!(CopyFormat::Url.render("a.png", url), url);
        assert_eq!(
            CopyFormat::MarkdownImage.render("a[1].png", url),
            "![a\\[1\\].png](https://cdn.example.com/a%281%29.png?x=1&y=2)"
        );
        assert_eq!(
            CopyFormat::MarkdownLink.render("a.png", "https://x.test/a.png"),
            "[a.png](https://x.test/a.png)"
        );
        assert_eq!(
            CopyFormat::Html.render("a\"<&.png", url),
            "<img src=\"https://cdn.example.com/a%281%29.png?x=1&amp;y=2\" alt=\"a&quot;&lt;&amp;.png\">"
        );
        assert_eq!(
            CopyFormat::Bbcode.render("a.png", "https://x.test/a[/img]\n.png"),
            "[img]https://x.test/a%5B/img%5D%0A.png[/img]"
        );
    }

    #[test]
    fn preferences_survive_restart_without_touching_queue() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("queue.json"), "[]").unwrap();
        let preferences = Preferences {
            library_view: LibraryView::List,
            copy_format: CopyFormat::MarkdownImage,
            auto_copy: false,
            sidebar_collapsed: true,
            check_updates: true,
        };
        preferences.save(temp.path()).unwrap();
        assert_eq!(Preferences::load(temp.path()).unwrap(), preferences);
        assert_eq!(
            std::fs::read(temp.path().join("queue.json")).unwrap(),
            b"[]"
        );
    }

    #[test]
    fn older_preferences_enable_automatic_link_copy() {
        let preferences: Preferences =
            serde_json::from_str(r#"{"library_view":"List","copy_format":"MarkdownImage"}"#)
                .unwrap();
        assert!(preferences.auto_copy);
        assert!(!preferences.sidebar_collapsed);
        assert_eq!(preferences.copy_format, CopyFormat::MarkdownImage);
    }
}
