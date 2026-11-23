use super::{
    super::{HtmlString, InvalidUri, Str, Uri},
    Stringable,
};

impl Stringable {
    pub fn after_last(self, search: &str) -> Self {
        Self(Str::after_last(&self.0, search).into())
    }

    pub fn before_last(self, search: &str) -> Self {
        Self(Str::before_last(&self.0, search).into())
    }

    pub fn between_first(self, from: &str, to: &str) -> Self {
        Self(Str::between_first(&self.0, from, to).into())
    }

    pub fn apa(self) -> Self {
        Self(Str::apa(&self.0))
    }

    pub fn headline(self) -> Self {
        Self(Str::headline(&self.0))
    }

    pub fn title(self) -> Self {
        Self(Str::title(&self.0))
    }

    pub fn initials(self, capitalize: bool) -> Self {
        Self(Str::initials(&self.0, capitalize))
    }

    pub fn lcfirst(self) -> Self {
        Self(Str::lcfirst(&self.0))
    }

    pub fn ucfirst(self) -> Self {
        Self(Str::ucfirst(&self.0))
    }

    pub fn ucwords(self, separators: Option<&str>) -> Self {
        Self(Str::ucwords(&self.0, separators))
    }

    pub fn kebab(self) -> Self {
        Self(Str::kebab(&self.0))
    }

    pub fn studly(self, normalize: bool) -> Self {
        Self(Str::studly(&self.0, normalize))
    }

    pub fn start(self, prefix: &str) -> Self {
        Self(Str::start(&self.0, prefix))
    }

    pub fn finish(self, suffix: &str) -> Self {
        Self(Str::finish(&self.0, suffix))
    }

    pub fn wrap(self, before: &str, after: Option<&str>) -> Self {
        Self(Str::wrap(&self.0, before, after))
    }

    pub fn unwrap(self, before: &str, after: Option<&str>) -> Self {
        Self(Str::unwrap(&self.0, before, after).into())
    }

    pub fn pad_left(self, length: usize, pad: &str) -> Self {
        Self(Str::pad_left(&self.0, length, pad))
    }

    pub fn pad_right(self, length: usize, pad: &str) -> Self {
        Self(Str::pad_right(&self.0, length, pad))
    }

    pub fn pad_both(self, length: usize, pad: &str) -> Self {
        Self(Str::pad_both(&self.0, length, pad))
    }

    pub fn replace(self, replacements: &[(&str, &str)], case_sensitive: bool) -> Self {
        Self(Str::replace(&self.0, replacements, case_sensitive))
    }

    pub fn replace_first(self, search: &str, replacement: &str) -> Self {
        Self(Str::replace_first(search, replacement, &self.0))
    }

    pub fn replace_last(self, search: &str, replacement: &str) -> Self {
        Self(Str::replace_last(search, replacement, &self.0))
    }

    pub fn replace_array(self, search: &str, replacements: &[&str]) -> Self {
        Self(Str::replace_array(search, replacements, &self.0))
    }

    pub fn remove(self, search: &[&str], case_sensitive: bool) -> Self {
        Self(Str::remove(&self.0, search, case_sensitive))
    }

    pub fn swap(self, replacements: &[(&str, &str)]) -> Self {
        Self(Str::swap(&self.0, replacements))
    }

    pub fn substr_replace(self, replacement: &str, offset: isize, length: Option<isize>) -> Self {
        Self(Str::substr_replace(&self.0, replacement, offset, length))
    }

    pub fn repeat(self, times: usize) -> Self {
        Self(self.0.repeat(times))
    }

    pub fn to_base64(self) -> Self {
        Self(Str::to_base64(&self.0))
    }

    pub fn contains(&self, needles: &[&str], ignore_case: bool) -> bool {
        Str::contains(&self.0, needles, ignore_case)
    }

    pub fn contains_all(&self, needles: &[&str], ignore_case: bool) -> bool {
        Str::contains_all(&self.0, needles, ignore_case)
    }

    pub fn doesnt_contain(&self, needles: &[&str], ignore_case: bool) -> bool {
        Str::doesnt_contain(&self.0, needles, ignore_case)
    }

    pub fn ends_with(&self, needles: &[&str]) -> bool {
        Str::ends_with(&self.0, needles)
    }

    pub fn doesnt_end_with(&self, needles: &[&str]) -> bool {
        Str::doesnt_end_with(&self.0, needles)
    }

    pub fn starts_with(&self, needles: &[&str]) -> bool {
        Str::starts_with(&self.0, needles)
    }

    pub fn doesnt_start_with(&self, needles: &[&str]) -> bool {
        Str::doesnt_start_with(&self.0, needles)
    }

    pub fn is(&self, patterns: &[&str], ignore_case: bool) -> bool {
        Str::is(patterns, &self.0, ignore_case)
    }

    pub fn exactly(&self, expected: &str) -> bool {
        self.0 == expected
    }

    pub fn is_ascii(&self) -> bool {
        Str::is_ascii(&self.0)
    }

    pub fn is_json(&self) -> bool {
        Str::is_json(&self.0)
    }

    pub fn is_url(&self, protocols: &[&str]) -> bool {
        Str::is_url(&self.0, protocols)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }

    pub fn when_contains(
        self,
        needles: &[&str],
        ignore_case: bool,
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.contains(needles, ignore_case);
        self.when(condition, callback)
    }

    pub fn when_contains_all(
        self,
        needles: &[&str],
        ignore_case: bool,
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.contains_all(needles, ignore_case);
        self.when(condition, callback)
    }

    pub fn when_starts_with(self, needles: &[&str], callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.starts_with(needles);
        self.when(condition, callback)
    }

    pub fn when_doesnt_start_with(
        self,
        needles: &[&str],
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.doesnt_start_with(needles);
        self.when(condition, callback)
    }

    pub fn when_ends_with(self, needles: &[&str], callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.ends_with(needles);
        self.when(condition, callback)
    }

    pub fn when_doesnt_end_with(
        self,
        needles: &[&str],
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.doesnt_end_with(needles);
        self.when(condition, callback)
    }

    pub fn when_exactly(self, expected: &str, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.exactly(expected);
        self.when(condition, callback)
    }

    pub fn when_not_exactly(self, expected: &str, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = !self.exactly(expected);
        self.when(condition, callback)
    }

    pub fn when_is(
        self,
        patterns: &[&str],
        ignore_case: bool,
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.is(patterns, ignore_case);
        self.when(condition, callback)
    }

    pub fn when_is_ascii(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.is_ascii();
        self.when(condition, callback)
    }

    pub fn when_is_ulid(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.is_ulid();
        self.when(condition, callback)
    }

    pub fn when_is_uuid(self, version: Option<u8>, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.is_uuid(version);
        self.when(condition, callback)
    }

    pub fn when_empty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.is_empty();
        self.when(condition, callback)
    }

    pub fn when_not_empty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.is_not_empty();
        self.when(condition, callback)
    }

    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        if condition { callback(self) } else { self }
    }

    pub fn when_test(
        self,
        pattern: &str,
        callback: impl FnOnce(Self) -> Self,
    ) -> Result<Self, regex::Error> {
        let matched = self.test(pattern)?;
        Ok(self.when(matched, callback))
    }

    pub fn tap(self, callback: impl FnOnce(&Self)) -> Self {
        callback(&self);
        self
    }

    pub fn pipe<T>(self, callback: impl FnOnce(Self) -> T) -> T {
        callback(self)
    }

    pub fn append(mut self, parts: &[&str]) -> Self {
        self.0.extend(parts.iter().copied());
        self
    }

    pub fn prepend(self, parts: &[&str]) -> Self {
        Self(
            parts
                .iter()
                .copied()
                .chain(std::iter::once(self.0.as_str()))
                .collect(),
        )
    }

    pub fn new_line(mut self, times: usize) -> Self {
        self.0.extend(std::iter::repeat_n('\n', times));
        self
    }

    pub fn char_at(&self, index: isize) -> Option<char> {
        Str::char_at(&self.0, index)
    }

    pub fn excerpt(&self, phrase: &str, radius: usize, omission: &str) -> Option<Self> {
        Str::excerpt(&self.0, phrase, radius, omission).map(Self)
    }

    pub fn ucsplit(&self) -> Vec<&str> {
        Str::ucsplit(&self.0)
    }

    pub fn word_count(&self, additional: Option<&str>) -> usize {
        Str::word_count(&self.0, additional)
    }

    pub fn explode(&self, delimiter: &str) -> Vec<&str> {
        if delimiter.is_empty() {
            vec![&self.0]
        } else {
            self.0.split(delimiter).collect()
        }
    }

    pub fn split(&self, pattern: &str) -> Result<Vec<&str>, regex::Error> {
        Ok(regex::Regex::new(pattern)?.split(&self.0).collect())
    }

    pub fn test(&self, pattern: &str) -> Result<bool, regex::Error> {
        self.is_match(&[pattern])
    }

    pub fn from_base64(&self) -> Result<Vec<u8>, base64::DecodeError> {
        Str::from_base64(&self.0)
    }

    pub fn to_html_string(self) -> HtmlString {
        HtmlString::new(self.0)
    }

    pub fn to_uri(&self) -> Result<Uri, InvalidUri> {
        Uri::of(&self.0)
    }
    pub fn class_basename(self) -> Self {
        Self(self.0.rsplit('\\').next().unwrap_or_default().into())
    }

    pub fn basename(self, suffix: &str) -> Self {
        let file = self
            .0
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let shortened = file
            .strip_suffix(suffix)
            .filter(|name| !name.is_empty())
            .unwrap_or(file);
        Self(shortened.into())
    }

    pub fn dirname(self, levels: std::num::NonZeroUsize) -> Self {
        let mut name = self.0.as_str();
        for _ in 0..levels.get() {
            let previous = name;
            let trimmed = name.trim_end_matches('/');
            name = if trimmed.is_empty() {
                if name.starts_with('/') { "/" } else { "" }
            } else if let Some((parent, _)) = trimmed.rsplit_once('/') {
                let parent = parent.trim_end_matches('/');
                if parent.is_empty() { "/" } else { parent }
            } else {
                "."
            };
            if name == previous {
                break;
            }
        }
        Self(name.to_owned())
    }

    pub fn encrypt(self, serialize: bool) -> Result<Self, crate::encryption::EncryptException> {
        crate::encryption::Crypt::encrypt(self.0, serialize).map(Self)
    }

    pub fn decrypt(
        &self,
        unserialize: bool,
    ) -> Result<Vec<u8>, crate::encryption::DecryptException> {
        crate::encryption::Crypt::decrypt(&self.0, unserialize)
    }
    pub fn strip_tags(self, allowed: &[&str]) -> Self {
        Self(crate::support::strings::editing::strip_tags(
            &self.0, allowed,
        ))
    }
    /// Hashes text with a Rust digest implementation such as `md5::Md5`.
    pub fn hash<D: md5::Digest>(self) -> Self {
        use std::fmt::Write as _;

        let digest = D::digest(self.0.as_bytes());
        let mut encoded = String::with_capacity(digest.len() * 2);
        for byte in digest.iter() {
            write!(encoded, "{byte:02x}").expect("writing to a String");
        }
        Self(encoded)
    }
}
