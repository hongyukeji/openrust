use crate::translation::{Line, Translator};
use std::collections::BTreeMap;

/// 复用翻译器的组级回退；不能逐键回退后让后备语言覆盖当前语言的通配词条。
pub(super) fn lines(translator: &Translator, group: &str) -> BTreeMap<String, String> {
    let Ok(lines) = translator.array(group, &Default::default(), None) else {
        return BTreeMap::new();
    };
    let mut pending: Vec<_> = lines.into_iter().collect();
    let mut result = BTreeMap::new();
    while let Some((key, line)) = pending.pop() {
        match line {
            Line::Text(text) => {
                result.insert(key, text);
            }
            Line::Group(lines) => pending.extend(
                lines
                    .into_iter()
                    .map(|(name, line)| (format!("{key}.{name}"), line)),
            ),
            Line::List(lines) => pending.extend(
                lines
                    .into_iter()
                    .enumerate()
                    .map(|(index, line)| (format!("{key}.{index}"), line)),
            ),
        }
    }
    result
}

pub(super) fn attribute<'a>(names: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    names
        .get(name)
        .or_else(|| {
            names.iter().find_map(|(key, value)| {
                (key.contains('*') && super::message_positions::matches(key, name)).then_some(value)
            })
        })
        .map(String::as_str)
        .filter(|name| !name.is_empty() && *name != "0")
}

pub(super) fn custom(
    translator: &Translator,
    attribute: &str,
    rule: &str,
    category: Option<&str>,
) -> Option<String> {
    let ordinary = format!("{attribute}.{rule}");
    let keys = category
        .map(|category| format!("{ordinary}.{category}"))
        .into_iter()
        .chain(std::iter::once(ordinary));
    let mut catalog = None;
    for short in keys {
        let key = format!("validation.custom.{short}");
        if let Ok(message) = translator.string(&key, &Default::default(), None)
            && message != key
        {
            return Some(message);
        }
        let catalog = catalog.get_or_insert_with(|| lines(translator, "validation.custom"));
        if let Some((_, message)) = catalog.iter().find(|(pattern, _)| {
            *pattern == &short
                || pattern.contains('*') && crate::support::Str::is(&[pattern], &short, false)
        }) {
            return Some(message.clone());
        }
    }
    None
}
