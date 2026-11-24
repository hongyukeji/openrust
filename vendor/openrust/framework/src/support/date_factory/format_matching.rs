//! 复用固定格式模式与现有 PCRE2；保留格式匹配和日期创建之间的区别。
use super::{DateError, DateFacade, DateFactory};
use pcre2::bytes::{Captures, Regex};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};
#[derive(Deserialize)]
struct Patterns {
    #[serde(rename = "regexFormats")]
    formats: BTreeMap<String, String>,
    #[serde(rename = "regexFormatModifiers")]
    modifiers: BTreeMap<String, String>,
}
static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../resources/calendar/date-format-patterns.json"
    ))
    .expect("固定日期格式模式资源有效")
});
static LETTERS: LazyLock<Regex> = LazyLock::new(|| {
    let keys = PATTERNS
        .formats
        .keys()
        .chain(PATTERNS.modifiers.keys())
        .cloned()
        .collect::<String>();
    Regex::new(&format!(r"(?<!\\)((?:\\{{2}})*)([{keys}])")).expect("固定日期格式替换表达式有效")
});
static ESCAPED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?<!\\)((?:\\{2})*)\\(\w)").expect("固定日期格式转义表达式有效"));
static SLASHES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?<!\\)((?:\\{2})*)/").expect("固定日期格式分隔符表达式有效"));
fn replacement(
    regex: &Regex,
    text: &str,
    replace: impl Fn(&Captures<'_>) -> Option<String>,
) -> Option<String> {
    let mut output = String::new();
    let mut end = 0;
    for captures in regex.captures_iter(text.as_bytes()) {
        let captures = captures.ok()?;
        let matched = captures.get(0)?;
        output.push_str(text.get(end..matched.start())?);
        output.push_str(&replace(&captures)?);
        end = matched.end();
    }
    output.push_str(text.get(end..)?);
    Some(output)
}
fn capture<'a>(captures: &Captures<'a>, index: usize) -> Option<&'a str> {
    std::str::from_utf8(captures.get(index)?.as_bytes()).ok()
}
fn quote(format: &str) -> String {
    let mut result = String::new();
    for ch in format.chars() {
        if "\\.+*?[^]$(){}=!<>|:-#/".contains(ch) {
            result.push('\\');
        }
        result.push(ch);
    }
    result
}
fn matches(date: &str, format: &str, modifiers: bool) -> bool {
    if date.len() > 20 * 1024 * 1024 || format.len() > 20 * 1024 * 1024 {
        return false;
    }
    let format = if modifiers {
        format.to_owned()
    } else {
        quote(format)
    };
    let format = format.replace("\\\\", "\\");
    let Some(regex) = replacement(&LETTERS, &format, |captures| {
        let prefix = capture(captures, 1)?;
        let token = capture(captures, 2)?;
        let value = PATTERNS
            .formats
            .get(token)
            .or_else(|| modifiers.then(|| PATTERNS.modifiers.get(token)).flatten());
        Some(format!("{prefix}{}", value.map_or(token, String::as_str)))
    }) else {
        return false;
    };
    let Some(regex) = replacement(&ESCAPED, &regex, |captures| {
        Some(format!(
            "{}{}",
            capture(captures, 1)?,
            capture(captures, 2)?
        ))
    }) else {
        return false;
    };
    let Some(regex) = replacement(&SLASHES, &regex, |captures| {
        Some(format!("{}\\/", capture(captures, 1)?))
    }) else {
        return false;
    };
    Regex::new(&format!("^{regex}$"))
        .and_then(|regex| regex.is_match(date.as_bytes()))
        .unwrap_or(false)
}
impl DateFactory {
    pub fn has_format(&self, date: &str, format: &str) -> bool {
        matches(date, format, false)
    }
    pub fn has_format_with_modifiers(&self, date: &str, format: &str) -> bool {
        matches(date, format, true)
    }
}
impl DateFacade {
    pub fn has_format(date: &str, format: &str) -> Result<bool, DateError> {
        Ok(Self::factory()?.has_format(date, format))
    }
    pub fn has_format_with_modifiers(date: &str, format: &str) -> Result<bool, DateError> {
        Ok(Self::factory()?.has_format_with_modifiers(date, format))
    }
}

impl DateFactory {
    /// 使用两个固定 UTC 参考日期判断依赖关系，复用既有表达式解析器。
    pub fn has_relative_keywords<'a>(&self, input: impl Into<Option<&'a str>>) -> bool {
        let Some(input) = input
            .into()
            .filter(|input| !input.is_empty() && *input != "0")
        else {
            return false;
        };
        let reference =
            |seconds| jiff::Timestamp::from_second(seconds).expect("固定相对日期探测时间有效");
        let parse = |seconds| {
            crate::support::calendar::parse_timestamp_expression(
                input,
                reference(seconds),
                crate::support::DateTimezone::UTC,
            )
        };
        match (parse(946_684_800), parse(1_009_238_400)) {
            (Ok(first), Ok(second)) => first != second,
            _ => false,
        }
    }
}
impl DateFacade {
    pub fn has_relative_keywords<'a>(input: impl Into<Option<&'a str>>) -> Result<bool, DateError> {
        Ok(Self::factory()?.has_relative_keywords(input))
    }
}
