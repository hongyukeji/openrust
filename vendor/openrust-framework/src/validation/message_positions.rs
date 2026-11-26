use super::{Value, path, standard_rules};
use crate::support::{Number, php_float_string, php_integer_cast};
use regex::{Captures, Regex};
use std::sync::LazyLock;

/// 使用规范路径的实际数字段，不能把转义的字面点拆成数组层级。
pub(super) fn replace(message: &str, attribute: &str) -> String {
    static PLACEHOLDERS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"(?i):(?:(first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth|other)-)?(ordinal-position|position|index)"
    ).expect("固定位置占位符语法有效")
    });
    if !PLACEHOLDERS.is_match(message) {
        return message.to_owned();
    }
    let indexes: Vec<_> = path::parse(attribute)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|segment| {
            let path::Segment::Key(key) = segment else {
                return None;
            };
            let key = Value::String(key);
            standard_rules::numeric(&key).then(|| php_integer_cast(&key))
        })
        .collect();
    PLACEHOLDERS
        .replace_all(message, |captures: &Captures<'_>| {
            let level = match captures
                .get(1)
                .map(|value| value.as_str().to_ascii_lowercase())
                .as_deref()
            {
                None | Some("first") => 0,
                Some("second") => 1,
                Some("third") => 2,
                Some("fourth") => 3,
                Some("fifth") => 4,
                Some("sixth") => 5,
                Some("seventh") => 6,
                Some("eighth") => 7,
                Some("ninth") => 8,
                Some("tenth") => 9,
                _ => 10,
            };
            let Some(index) = indexes.get(level).copied() else {
                return captures[0].to_owned();
            };
            match captures[2].to_ascii_lowercase().as_str() {
                "index" => index.to_string(),
                "position" => index.checked_add(1).map_or_else(
                    || php_float_string(index as f64 + 1.0),
                    |value| value.to_string(),
                ),
                _ => {
                    let number = Number::default();
                    if let Some(position) = index.checked_add(1) {
                        number
                            .ordinal(position)
                            .map(|value| {
                                if position < 0 {
                                    value.replacen('-', "−", 1)
                                } else {
                                    value
                                }
                            })
                            .unwrap_or_else(|_| captures[0].to_owned())
                    } else {
                        // 整数上界加一后按既有浮点格式化；该十进制数的英语序数后缀为th。
                        number
                            .format(index as f64 + 1.0, None, None)
                            .map(|value| format!("{value}th"))
                            .unwrap_or_else(|_| captures[0].to_owned())
                    }
                }
            }
        })
        .into_owned()
}

/// 消息的通配符只消费同一层级；字面点字段不能越过分段边界。
pub(super) fn matches(pattern: &str, key: &str) -> bool {
    let pieces: Vec<_> = pattern.split('*').collect();
    let mut remaining = key;
    for (index, piece) in pieces.iter().enumerate() {
        if index == 0 {
            let Some(tail) = remaining.strip_prefix(piece) else {
                return false;
            };
            remaining = tail;
        } else if index == pieces.len() - 1 {
            let Some(prefix) = remaining.strip_suffix(piece) else {
                return false;
            };
            return !prefix.contains('.');
        } else {
            let Some(at) = remaining.find(piece) else {
                return false;
            };
            if remaining[..at].contains('.') {
                return false;
            }
            remaining = &remaining[at + piece.len()..];
        }
    }
    remaining.is_empty()
}
