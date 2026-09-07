//! Presentation-only translation. Stored record names, URLs and configuration keys stay unchanged.
use gpui_kit::{App, PromptLevel, SharedString, Window};
use regex::Regex;
use std::{
    collections::HashMap,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
static ENGLISH: AtomicBool = AtomicBool::new(false);
static CATALOG: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/en.json")).expect("English catalog")
});
static CACHE: LazyLock<Mutex<HashMap<String, SharedString>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static FIELDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^{}]*\}").unwrap());
struct Template {
    pattern: Regex,
    english: String,
}
static TEMPLATES: LazyLock<Vec<Template>> = LazyLock::new(|| {
    let mut rows = vec![];
    for (source, english) in CATALOG.iter() {
        let matches = FIELDS.find_iter(source).collect::<Vec<_>>();
        if matches.is_empty() {
            continue;
        }
        let mut pattern = String::from("(?s)^");
        let mut cursor = 0;
        for field in &matches {
            pattern.push_str(&regex::escape(&source[cursor..field.start()]));
            pattern.push_str("(.*?)");
            cursor = field.end();
        }
        pattern.push_str(&regex::escape(&source[cursor..]));
        pattern.push('$');
        rows.push((
            source.len() - matches.iter().map(|m| m.len()).sum::<usize>(),
            Template {
                pattern: Regex::new(&pattern).unwrap(),
                english: english.clone(),
            },
        ));
    }
    rows.sort_by_key(|(specificity, _)| std::cmp::Reverse(*specificity));
    rows.into_iter().map(|(_, t)| t).collect()
});
pub fn set_english(english: bool) {
    ENGLISH.store(english, Ordering::Relaxed);
    gpui_kit::component::set_locale(if english { "en" } else { "zh-CN" });
}
fn translate(value: &str, depth: u8) -> String {
    if let Some(translated) = CATALOG.get(value) {
        return translated.clone();
    }
    if depth > 5 {
        return value.into();
    }
    if let Some(label) = value.strip_suffix(" *")
        && CATALOG.contains_key(label)
    {
        return format!("{} *", translate(label, depth + 1));
    }
    for template in TEMPLATES.iter() {
        if let Some(captures) = template.pattern.captures(value) {
            let mut index = 0;
            return FIELDS
                .replace_all(&template.english, |_: &regex::Captures<'_>| {
                    index += 1;
                    translate(
                        captures.get(index).map(|v| v.as_str()).unwrap_or(""),
                        depth + 1,
                    )
                })
                .into_owned();
        }
    }
    value.into()
}
pub fn text(value: impl Into<SharedString>) -> SharedString {
    let value = value.into();
    if !ENGLISH.load(Ordering::Relaxed)
        || !value
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    {
        return value;
    }
    if let Some(cached) = CACHE.lock().unwrap().get(value.as_ref()).cloned() {
        return cached;
    }
    let translated: SharedString = translate(&value, 0).into();
    let mut cache = CACHE.lock().unwrap();
    if cache.len() >= 2048 {
        cache.clear();
    }
    cache.insert(value.to_string(), translated.clone());
    translated
}
pub fn prompt(
    window: &mut Window,
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    answers: &[&str],
    cx: &mut App,
) -> futures_channel::oneshot::Receiver<usize> {
    let message = text(message.to_owned());
    let detail = detail.map(|s| text(s.to_owned()));
    let answers = answers
        .iter()
        .map(|s| text((*s).to_owned()))
        .collect::<Vec<_>>();
    let refs = answers.iter().map(|s| s.as_ref()).collect::<Vec<&str>>();
    window.prompt(level, &message, detail.as_deref(), &refs, cx)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_preserves_placeholders_and_translates_dynamic_counts() {
        for (source, translated) in CATALOG.iter() {
            assert_eq!(
                FIELDS
                    .find_iter(source)
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>(),
                FIELDS
                    .find_iter(translated)
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>(),
                "{source}"
            );
        }
        assert_eq!(translate("已选 998 项", 0), "998 selected");
        assert_eq!(
            translate("清理 3 条记录？其中 2 项不在当前搜索结果中", 0),
            "Remove 3 records? 2 are outside the current search results."
        );
        assert_eq!(translate("存储源名称 *", 0), "Storage name *");
        assert_eq!(translate("holiday.png", 0), "holiday.png");
    }
}
