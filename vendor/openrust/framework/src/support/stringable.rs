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

    pub fn lower(self) -> Self {
        Self(super::Str::lower(&self.0))
    }

    pub fn upper(self) -> Self {
        Self(super::Str::upper(&self.0))
    }

    pub fn after(self, search: &str) -> Self {
        Self::from(super::Str::after(&self.0, search))
    }

    pub fn before(self, search: &str) -> Self {
        Self::from(super::Str::before(&self.0, search))
    }

    pub fn length(&self) -> usize {
        super::Str::length(&self.0)
    }

    pub fn between(self, from: &str, to: &str) -> Self {
        Self::from(super::Str::between(&self.0, from, to))
    }

    pub fn snake(self, delimiter: &str) -> Self {
        Self(super::Str::snake(&self.0, delimiter))
    }

    pub fn camel(self) -> Self {
        Self(super::Str::camel(&self.0))
    }

    pub fn substr(self, start: isize, length: Option<isize>) -> Self {
        Self::from(super::Str::substr(&self.0, start, length))
    }

    pub fn words(self, limit: usize, end: &str) -> Self {
        Self(super::Str::words(&self.0, limit, end))
    }

    pub fn chopStart(self, prefixes: &[&str]) -> Self {
        Self::from(super::Str::chopStart(&self.0, prefixes))
    }

    pub fn chopEnd(self, suffixes: &[&str]) -> Self {
        Self::from(super::Str::chopEnd(&self.0, suffixes))
    }

    pub fn replaceStart(self, search: &str, replacement: &str) -> Self {
        Self(super::Str::replaceStart(search, replacement, &self.0))
    }

    pub fn replaceEnd(self, search: &str, replacement: &str) -> Self {
        Self(super::Str::replaceEnd(search, replacement, &self.0))
    }

    pub fn deduplicate(self, characters: &[char]) -> Self {
        Self(super::Str::deduplicate(&self.0, characters))
    }

    pub fn position(&self, search: &str, offset: isize) -> Option<usize> {
        super::Str::position(&self.0, search, offset)
    }

    pub fn take(self, limit: isize) -> Self {
        Self::from(super::Str::take(&self.0, limit))
    }

    pub fn mask(self, token: &str, start: isize, length: Option<isize>) -> Self {
        Self(super::Str::mask(&self.0, token, start, length))
    }

    pub fn ltrim(mut self) -> Self {
        let start = self.0.len() - super::Str::ltrim(&self.0).len();
        self.0.drain(..start);
        self
    }

    pub fn rtrim(mut self) -> Self {
        self.0.truncate(super::Str::rtrim(&self.0).len());
        self
    }

    pub fn squish(self) -> Self {
        Self(super::Str::squish(&self.0))
    }

    pub fn r#match(&self, pattern: &str) -> Result<Self, regex::Error> {
        super::Str::r#match(pattern, &self.0).map(Self::from)
    }

    pub fn matchAll(&self, pattern: &str) -> Result<Vec<&str>, regex::Error> {
        super::Str::matchAll(pattern, &self.0)
    }

    pub fn isMatch(&self, patterns: &[&str]) -> Result<bool, regex::Error> {
        super::Str::isMatch(patterns, &self.0)
    }

    pub fn replaceMatches(
        self,
        pattern: &str,
        replacement: &str,
        count: Option<usize>,
    ) -> Result<Self, regex::Error> {
        super::Str::replaceMatches(pattern, replacement, &self.0, count).map(Self)
    }

    pub fn replaceMatchesWith(
        self,
        pattern: &str,
        replacement: impl FnMut(&regex::Captures<'_>) -> String,
        count: Option<usize>,
    ) -> Result<Self, regex::Error> {
        super::Str::replaceMatchesWith(pattern, replacement, &self.0, count).map(Self)
    }

    pub fn width(&self) -> usize {
        super::Str::width(&self.0)
    }

    pub fn limit(self, columns: usize, end: &str, preserve_words: bool) -> Self {
        Self(super::Str::limit(&self.0, columns, end, preserve_words))
    }

    pub fn wordWrap(
        self,
        columns: usize,
        line_break: &str,
        cut_long_words: bool,
    ) -> Result<Self, super::WordWrapError> {
        super::Str::wordWrap(&self.0, columns, line_break, cut_long_words).map(Self)
    }

    pub fn ascii(self, language: &str) -> Self {
        Self(super::Str::ascii(&self.0, language))
    }

    pub fn slug(self) -> Self {
        Self(super::Str::slug(&self.0))
    }

    pub fn slugWith(
        self,
        separator: &str,
        language: Option<&str>,
        dictionary: &[(&str, &str)],
    ) -> Self {
        Self(super::Str::slugWith(
            &self.0, separator, language, dictionary,
        ))
    }

    pub fn transliterate(self) -> Self {
        Self(super::Str::transliterate(&self.0))
    }

    pub fn transliterateWithOptions(self, unknown: Option<&str>, strict: bool) -> Self {
        Self(super::Str::transliterateWithOptions(
            &self.0, unknown, strict,
        ))
    }

    pub fn isUuid(&self, version: Option<u8>) -> bool {
        super::Str::isUuid(&self.0, version)
    }

    pub fn isUlid(&self) -> bool {
        super::Str::isUlid(&self.0)
    }

    pub fn markdown(self) -> Self {
        Self(super::Str::markdown(&self.0))
    }

    pub fn inlineMarkdown(self) -> Self {
        Self(super::Str::inlineMarkdown(&self.0))
    }

    pub fn plural(self, count: i64) -> Self {
        Self(super::Str::plural(&self.0, count))
    }

    pub fn singular(self) -> Self {
        Self(super::Str::singular(&self.0))
    }

    pub fn pluralStudly(self, count: i64) -> Self {
        Self(super::Str::pluralStudly(&self.0, count))
    }

    pub fn pluralPascal(self, count: i64) -> Self {
        Self(super::Str::pluralPascal(&self.0, count))
    }

    pub fn pluralIn(self, count: i64, language: super::InflectionLanguage) -> Self {
        Self(super::Str::pluralIn(&self.0, count, language))
    }

    pub fn singularIn(self, language: super::InflectionLanguage) -> Self {
        Self(super::Str::singularIn(&self.0, language))
    }

    pub fn pluralStudlyIn(self, count: i64, language: super::InflectionLanguage) -> Self {
        Self(super::Str::pluralStudlyIn(&self.0, count, language))
    }

    pub fn pluralPascalIn(self, count: i64, language: super::InflectionLanguage) -> Self {
        Self(super::Str::pluralPascalIn(&self.0, count, language))
    }

    pub fn countedIn(self, count: i64, language: super::InflectionLanguage) -> Self {
        Self(super::Str::countedIn(&self.0, count, language))
    }

    pub fn counted(self, count: i64) -> Self {
        Self(super::Str::counted(&self.0, count))
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

pub(super) fn trim_character(value: char) -> bool {
    matches!(value, '\0' | '\u{9}'..='\u{d}' | '\u{20}' | '\u{85}' | '\u{a0}' | '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}'..='\u{1160}' | '\u{1680}' | '\u{17b4}'..='\u{17b5}' | '\u{180e}' | '\u{2000}'..='\u{200f}' | '\u{2028}'..='\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{2060}'..='\u{2065}' | '\u{206a}'..='\u{206f}' | '\u{2800}' | '\u{3000}' | '\u{3164}' | '\u{feff}' | '\u{ffa0}' | '\u{1d159}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0020}')
}
