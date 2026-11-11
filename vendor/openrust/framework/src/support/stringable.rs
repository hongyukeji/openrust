//! Fluent string values.

use std::fmt;
use std::ops::Deref;

use serde::Serialize;

/// An owned, chainable string that serializes as a JSON string.
///
/// ```
/// use openrust::support::Stringable;
///
/// let value = Stringable::from("  OpenRust  ").trim();
/// assert_eq!(value.value(), "OpenRust");
/// assert_eq!(value.toString(), "OpenRust");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[must_use]
pub struct Stringable(String);

#[allow(non_snake_case, reason = "Matches Laravel's Stringable API")]
impl Stringable {
    pub fn value(&self) -> &str {
        &self.0
    }

    pub fn toString(&self) -> &str {
        self.value()
    }

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

fn trim_character(value: char) -> bool {
    matches!(value, '\0' | '\u{9}'..='\u{d}' | '\u{20}' | '\u{85}' | '\u{a0}' | '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}'..='\u{1160}' | '\u{1680}' | '\u{17b4}'..='\u{17b5}' | '\u{180e}' | '\u{2000}'..='\u{200f}' | '\u{2028}'..='\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{2060}'..='\u{2065}' | '\u{206a}'..='\u{206f}' | '\u{2800}' | '\u{3000}' | '\u{3164}' | '\u{feff}' | '\u{ffa0}' | '\u{1d159}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0020}')
}
