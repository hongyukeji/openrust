//! 拥有所有权的字符串包装。

use std::fmt;
use std::ops::Deref;

use serde::Serialize;

/// 支持消费式链式操作的字符串；序列化时直接输出字符串。
///
/// ```
/// use openrust::support::Stringable;
///
/// let value = Stringable::from("  OpenRust  ").trim();
/// assert_eq!(value.value(), "OpenRust");
/// assert_eq!(value.into_string(), "OpenRust");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[must_use]
pub struct Stringable(String);

impl Stringable {
    /// 借用当前字符串。
    pub fn value(&self) -> &str {
        &self.0
    }

    /// 取出拥有所有权的字符串。
    pub fn into_string(self) -> String {
        self.0
    }

    /// 用固定 Unicode 字符集合裁剪首尾空白和不可见字符。
    ///
    /// 包括 Unicode 空白、BOM、零宽字符和 NUL；内部字符保持原样。
    /// 空结果仍是字符串，不应用请求密码排除或空字符串转 null 的策略。
    pub fn trim(mut self) -> Self {
        let end = self.0.trim_end_matches(trim_character).len();
        self.0.truncate(end);
        let start = self.0.len() - self.0.trim_start_matches(trim_character).len();
        self.0.drain(..start);
        self
    }
}

impl AsRef<str> for Stringable {
    fn as_ref(&self) -> &str {
        self.value()
    }
}

impl super::DataObject for Stringable {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn json_projection(&self) -> Option<serde_json::Value> {
        Some(serde_json::Value::String(self.0.clone()))
    }

    fn string_projection(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

impl Deref for Stringable {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.value()
    }
}

impl fmt::Display for Stringable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.value())
    }
}

impl From<String> for Stringable {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Stringable {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

// Laravel v13.39 Str::trim：与请求规范化一致的 PCRE Unicode whitespace 和不可见字符。
pub(crate) fn trim_character(value: char) -> bool {
    matches!(value, '\0' | '\u{9}'..='\u{d}' | '\u{20}' | '\u{85}' | '\u{a0}' | '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}'..='\u{1160}' | '\u{1680}' | '\u{17b4}'..='\u{17b5}' | '\u{180e}' | '\u{2000}'..='\u{200f}' | '\u{2028}'..='\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{2060}'..='\u{2065}' | '\u{206a}'..='\u{206f}' | '\u{2800}' | '\u{3000}' | '\u{3164}' | '\u{feff}' | '\u{ffa0}' | '\u{1d159}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0020}')
}

#[cfg(test)]
mod tests {
    use super::Stringable;

    // 使用固定字符集合，避免普通 Rust trim 漏掉额外不可见字符。
    #[test]
    fn trim_matches_fixed_unicode_and_invisible_character_set() {
        let characters = "\0\t\n\u{b}\u{c}\r \u{85}\u{a0}\u{ad}\u{34f}\u{61c}\u{115f}\u{1160}\u{1680}\u{17b4}\u{17b5}\u{180e}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{200b}\u{200c}\u{200d}\u{200e}\u{200f}\u{2028}\u{2029}\u{202f}\u{205f}\u{2060}\u{2061}\u{2062}\u{2063}\u{2064}\u{2065}\u{206a}\u{206b}\u{206c}\u{206d}\u{206e}\u{206f}\u{2800}\u{3000}\u{3164}\u{feff}\u{ffa0}\u{1d159}\u{1d173}\u{1d174}\u{1d175}\u{1d176}\u{1d177}\u{1d178}\u{1d179}\u{1d17a}\u{e0020}";
        for character in characters.chars() {
            assert_eq!(
                Stringable::from(format!("{character}正文{character}"))
                    .trim()
                    .value(),
                "正文",
                "未裁剪 U+{:04X}",
                u32::from(character),
            );
        }
        assert_eq!(Stringable::from(characters).trim().value(), "");
        // Unicode 控制字符并非一律删除；这些相邻码点不在固定集合中。
        for character in ['\u{8}', '\u{e}', '\u{2066}', '\u{2069}', '\u{e0021}'] {
            let text = format!("{character}正文{character}");
            assert_eq!(Stringable::from(text.clone()).trim().value(), text);
        }
    }

    // 防止裁剪误删内部字符，或链式方法丢失字符串所有权和空串表示。
    #[test]
    fn trim_chains_and_preserves_internal_characters() {
        let value = Stringable::from("\u{feff} \u{200b}甲\0 \u{feff}\u{200b}乙\u{200b} ")
            .trim()
            .trim();
        assert_eq!(value.value(), "甲\0 \u{feff}\u{200b}乙");
        assert_eq!(value.as_ref(), &*value);
        assert_eq!(value.to_string(), value.value());
        assert_eq!(value.clone().into_string(), value.value());
        assert_eq!(Stringable::from(String::new()).trim().into_string(), "");
    }

    // 防止包装类型意外序列化为对象或把空字符串转为 null。
    #[test]
    fn serializes_as_plain_string() -> serde_json::Result<()> {
        for text in ["", "正文\0\u{feff}\u{200b}", "  保留原文  "] {
            assert_eq!(
                serde_json::to_value(Stringable::from(text))?,
                serde_json::Value::String(text.to_owned()),
            );
        }
        Ok(())
    }
}
