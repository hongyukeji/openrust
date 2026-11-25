use crate::localization::{Line, Localizer};
use std::collections::BTreeMap;

/// 复用翻译器的组级回退；不能逐键回退后让后备语言覆盖当前语言的通配词条。
pub(super) fn lines(localizer: &Localizer, group: &str) -> BTreeMap<String, String> {
    let Ok(lines) = localizer.array(group, &Default::default(), None) else {
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
    localizer: &Localizer,
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
        if let Ok(message) = localizer.string(&key, &Default::default(), None)
            && message != key
        {
            return Some(message);
        }
        let catalog = catalog.get_or_insert_with(|| lines(localizer, "validation.custom"));
        if let Some((_, message)) = catalog.iter().find(|(pattern, _)| {
            *pattern == &short
                || pattern.contains('*') && crate::support::Str::is(&[pattern], &short, false)
        }) {
            return Some(message.clone());
        }
    }
    None
}

/// 内联消息先按字段规则、规则、字段查找；每层保留调用方有序映射的遍历顺序。
pub(super) fn inline<'a>(
    messages: &'a BTreeMap<String, String>,
    attribute: &str,
    rule: &str,
) -> Option<&'a str> {
    let specific = format!("{attribute}.{rule}");
    for key in [specific.as_str(), rule, attribute] {
        if let Some((_, message)) = messages.iter().find(|(pattern, _)| {
            pattern.as_str() == key
                || pattern.contains('*') && super::message_positions::matches(pattern, key)
        }) {
            return Some(message);
        }
    }
    None
}
