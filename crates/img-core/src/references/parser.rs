//! Source ranges come from CommonMark; replacement never reformats the document.
use pulldown_cmark::{Event, LinkType, Parser, Tag};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    ops::Range,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub source: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub occurrences: usize,
    pub syntax: Syntax,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Syntax {
    Markdown { angle: bool },
    Html,
    SharedReference { title: String },
}
impl Reference {
    pub fn replacement(&self, url: &str) -> String {
        match &self.syntax {
            Syntax::Html => html_escape::encode_double_quoted_attribute(url)
                .replace('\'', "&#39;")
                .replace(' ', "&#32;")
                .replace('>', "&gt;"),
            Syntax::Markdown { angle } => {
                let url = url.replace('&', "&amp;");
                if *angle {
                    url
                } else {
                    url.replace('(', "\\(").replace(')', "\\)")
                }
            }
            Syntax::SharedReference { title } => {
                let title = if title.is_empty() {
                    String::new()
                } else {
                    format!(
                        " \"{}\"",
                        title
                            .replace('&', "&amp;")
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"")
                    )
                };
                format!("(<{}>{title})", url.replace('&', "&amp;"))
            }
        }
    }
}
fn close_label(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => depth += 1,
            b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
fn destination(text: &str, mut at: usize) -> Option<(Range<usize>, bool)> {
    let bytes = text.as_bytes();
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if bytes.get(at) == Some(&b'<') {
        let start = at + 1;
        at = start;
        while at < bytes.len() {
            if bytes[at] == b'\\' {
                at += 2;
                continue;
            }
            if bytes[at] == b'>' {
                return Some((start..at, true));
            }
            at += 1;
        }
        return None;
    }
    let start = at;
    let mut depth = 0usize;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'\\' => {
                at += 2;
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1
            }
            _ if byte.is_ascii_whitespace() => break,
            _ => {}
        }
        at += 1;
    }
    (at > start && at <= bytes.len()).then_some((start..at, false))
}
fn line(text: &str, at: usize) -> usize {
    text[..at].bytes().filter(|&b| b == b'\n').count() + 1
}
pub fn scan(text: &str) -> Vec<Reference> {
    let mut parser = Parser::new(text).into_offset_iter();
    let events = parser.by_ref().collect::<Vec<_>>();
    let defs = parser.reference_definitions();
    let shared: HashSet<_> = events
        .iter()
        .filter_map(|(event, _)| match event {
            Event::Start(Tag::Link {
                link_type: LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut,
                id,
                ..
            }) => defs.get(id).map(|def| def.span.start),
            _ => None,
        })
        .collect();
    let mut refs = Vec::new();
    let mut html_blocks = Vec::new();
    for (event, span) in events {
        match event {
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                id,
                ..
            }) => {
                let raw = &text[span.clone()];
                let Some(close) = close_label(raw, 1) else {
                    continue;
                };
                let located = match link_type {
                    LinkType::Inline => {
                        let mut at = close + 1;
                        while raw.as_bytes().get(at).is_some_and(u8::is_ascii_whitespace) {
                            at += 1;
                        }
                        if raw.as_bytes().get(at) != Some(&b'(') {
                            continue;
                        }
                        destination(raw, at + 1).map(|(range, angle)| {
                            (
                                span.start + range.start..span.start + range.end,
                                Syntax::Markdown { angle },
                            )
                        })
                    }
                    LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut => {
                        defs.get(&id).and_then(|def| {
                            if shared.contains(&def.span.start) {
                                return Some((
                                    span.start + close + 1..span.end,
                                    Syntax::SharedReference {
                                        title: def.title.as_deref().unwrap_or("").into(),
                                    },
                                ));
                            }
                            let raw = &text[def.span.clone()];
                            let open = raw.find('[')?;
                            let close = close_label(raw, open)?;
                            let colon = raw[close + 1..].find(':')? + close + 1;
                            destination(raw, colon + 1).map(|(range, angle)| {
                                (
                                    def.span.start + range.start..def.span.start + range.end,
                                    Syntax::Markdown { angle },
                                )
                            })
                        })
                    }
                    _ => None,
                };
                if let Some((range, syntax)) = located {
                    refs.push(Reference {
                        source: dest_url.into_string(),
                        start: range.start,
                        end: range.end,
                        line: line(text, span.start),
                        occurrences: 1,
                        syntax,
                    });
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => html_blocks.push(span),
            _ => {}
        }
    }
    let mut excluded = Vec::new();
    for span in html_blocks {
        html(text, span, &mut excluded, &mut refs);
    }
    let mut unique = BTreeMap::<(usize, usize), Reference>::new();
    for reference in refs {
        unique
            .entry((reference.start, reference.end))
            .and_modify(|r| r.occurrences += 1)
            .or_insert(reference);
    }
    unique.into_values().collect()
}
fn html(text: &str, span: Range<usize>, excluded: &mut Vec<String>, refs: &mut Vec<Reference>) {
    let bytes = text.as_bytes();
    let mut at = span.start;
    while at < span.end {
        if bytes[at] != b'<' {
            at += 1;
            continue;
        }
        if text[at..].starts_with("<!--") {
            at = text[at + 4..span.end]
                .find("-->")
                .map(|end| at + 4 + end + 3)
                .unwrap_or(span.end);
            continue;
        }
        let mut end = at + 1;
        let mut quote = None;
        while end < span.end {
            let b = bytes[end];
            if quote == Some(b) {
                quote = None
            } else if quote.is_none() {
                if b == b'\'' || b == b'"' {
                    quote = Some(b)
                } else if b == b'>' {
                    break;
                }
            }
            end += 1;
        }
        if end == span.end {
            break;
        }
        let mut cursor = at + 1;
        let closing = bytes.get(cursor) == Some(&b'/');
        if closing {
            cursor += 1
        }
        let start = cursor;
        while cursor < end && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'-') {
            cursor += 1;
        }
        let tag = text[start..cursor].to_ascii_lowercase();
        if ["pre", "code", "script", "style", "textarea"].contains(&tag.as_str()) {
            if closing {
                if let Some(index) = excluded.iter().rposition(|t| t == &tag) {
                    excluded.truncate(index)
                }
            } else {
                excluded.push(tag)
            }
            at = end + 1;
            continue;
        }
        if tag == "img" && !closing && excluded.is_empty() {
            while cursor < end {
                while cursor < end && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/')
                {
                    cursor += 1;
                }
                let name = cursor;
                while cursor < end
                    && !bytes[cursor].is_ascii_whitespace()
                    && !b"=/>".contains(&bytes[cursor])
                {
                    cursor += 1;
                }
                if name == cursor {
                    cursor += 1;
                    continue;
                }
                let src = text[name..cursor].eq_ignore_ascii_case("src");
                while cursor < end && bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(&b'=') {
                    continue;
                }
                cursor += 1;
                while cursor < end && bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                let quoted = bytes
                    .get(cursor)
                    .copied()
                    .filter(|b| *b == b'\'' || *b == b'"');
                if quoted.is_some() {
                    cursor += 1;
                }
                let start = cursor;
                while cursor < end
                    && if let Some(q) = quoted {
                        bytes[cursor] != q
                    } else {
                        !bytes[cursor].is_ascii_whitespace()
                    }
                {
                    cursor += 1;
                }
                if src {
                    refs.push(Reference {
                        source: html_escape::decode_html_entities(&text[start..cursor])
                            .into_owned(),
                        start,
                        end: cursor,
                        line: line(text, at),
                        occurrences: 1,
                        syntax: Syntax::Html,
                    });
                    break;
                }
                if quoted.is_some() {
                    cursor += 1;
                }
            }
        }
        at = end + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commonmark_images_and_html_skip_all_code_and_preserve_shared_text_links() {
        let text = "![a](https://old.test/a\\(b\\).png \"title\")\n![nested [alt]][pic] ![pic][] ![pic]\n\n[pic]: <https://old.test/a.png> 'caption'\n\n[ordinary][shared] ![image][shared]\n\n[shared]: https://old.test/b.png \"shared title\"\n\n<img alt='>' data-src='wrong' SRC='https://old.test/a.png?a=1&amp;b=2'>\n\n~~~md\n![code](skip.png)\n~~~\n\n    ![indent](skip.png)\n\n`` ![](skip.png) ``\n\n<!-- <img src='skip.png'> -->\n<pre><code><img src='skip.png'></code></pre>\n<script>const s=\"<img src='skip.png'>\"</script>\n";
        let refs = scan(text);
        assert_eq!(refs.len(), 4, "{refs:?}");
        assert!(
            refs.iter()
                .all(|r| !r.source.contains("skip") && r.source != "wrong")
        );
        assert_eq!(refs[0].source, "https://old.test/a(b).png");
        assert_eq!(refs[1].occurrences, 3);
        assert!(matches!(refs[2].syntax, Syntax::SharedReference { .. }));
        assert_eq!(refs[3].source, "https://old.test/a.png?a=1&b=2");
        let mut result = text.to_owned();
        for r in refs.iter().rev() {
            result.replace_range(
                r.start..r.end,
                &r.replacement("https://new.test/x(y).png?a=1&b=2"),
            );
        }
        let after = scan(&result);
        assert!(
            after
                .iter()
                .all(|r| r.source == "https://new.test/x(y).png?a=1&b=2"),
            "{after:?}"
        );
        assert!(result.contains("[shared]: https://old.test/b.png"));
        assert!(result.contains("[ordinary][shared]"));
    }
}
