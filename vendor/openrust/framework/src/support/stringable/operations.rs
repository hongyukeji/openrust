use super::{
    super::{HtmlString, InvalidUri, Str, Uri},
    Stringable,
};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Stringable {
    pub fn afterLast(self, search: &str) -> Self {
        Self(Str::afterLast(&self.0, search).into())
    }

    pub fn beforeLast(self, search: &str) -> Self {
        Self(Str::beforeLast(&self.0, search).into())
    }

    pub fn betweenFirst(self, from: &str, to: &str) -> Self {
        Self(Str::betweenFirst(&self.0, from, to).into())
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

    pub fn padLeft(self, length: usize, pad: &str) -> Self {
        Self(Str::padLeft(&self.0, length, pad))
    }

    pub fn padRight(self, length: usize, pad: &str) -> Self {
        Self(Str::padRight(&self.0, length, pad))
    }

    pub fn padBoth(self, length: usize, pad: &str) -> Self {
        Self(Str::padBoth(&self.0, length, pad))
    }

    pub fn replace(self, replacements: &[(&str, &str)], case_sensitive: bool) -> Self {
        Self(Str::replace(&self.0, replacements, case_sensitive))
    }

    pub fn replaceFirst(self, search: &str, replacement: &str) -> Self {
        Self(Str::replaceFirst(search, replacement, &self.0))
    }

    pub fn replaceLast(self, search: &str, replacement: &str) -> Self {
        Self(Str::replaceLast(search, replacement, &self.0))
    }

    pub fn replaceArray(self, search: &str, replacements: &[&str]) -> Self {
        Self(Str::replaceArray(search, replacements, &self.0))
    }

    pub fn remove(self, search: &[&str], case_sensitive: bool) -> Self {
        Self(Str::remove(&self.0, search, case_sensitive))
    }

    pub fn swap(self, replacements: &[(&str, &str)]) -> Self {
        Self(Str::swap(&self.0, replacements))
    }

    pub fn substrReplace(self, replacement: &str, offset: isize, length: Option<isize>) -> Self {
        Self(Str::substrReplace(&self.0, replacement, offset, length))
    }

    pub fn repeat(self, times: usize) -> Self {
        Self(self.0.repeat(times))
    }

    pub fn toBase64(self) -> Self {
        Self(Str::toBase64(&self.0))
    }

    pub fn contains(&self, needles: &[&str], ignore_case: bool) -> bool {
        Str::contains(&self.0, needles, ignore_case)
    }

    pub fn containsAll(&self, needles: &[&str], ignore_case: bool) -> bool {
        Str::containsAll(&self.0, needles, ignore_case)
    }

    pub fn doesntContain(&self, needles: &[&str], ignore_case: bool) -> bool {
        Str::doesntContain(&self.0, needles, ignore_case)
    }

    pub fn endsWith(&self, needles: &[&str]) -> bool {
        Str::endsWith(&self.0, needles)
    }

    pub fn doesntEndWith(&self, needles: &[&str]) -> bool {
        Str::doesntEndWith(&self.0, needles)
    }

    pub fn startsWith(&self, needles: &[&str]) -> bool {
        Str::startsWith(&self.0, needles)
    }

    pub fn doesntStartWith(&self, needles: &[&str]) -> bool {
        Str::doesntStartWith(&self.0, needles)
    }

    pub fn is(&self, patterns: &[&str], ignore_case: bool) -> bool {
        Str::is(patterns, &self.0, ignore_case)
    }

    pub fn exactly(&self, expected: &str) -> bool {
        self.0 == expected
    }

    pub fn isAscii(&self) -> bool {
        Str::isAscii(&self.0)
    }

    pub fn isJson(&self) -> bool {
        Str::isJson(&self.0)
    }

    pub fn isUrl(&self, protocols: &[&str]) -> bool {
        Str::isUrl(&self.0, protocols)
    }

    pub fn isEmpty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn isNotEmpty(&self) -> bool {
        !self.isEmpty()
    }

    pub fn whenContains(
        self,
        needles: &[&str],
        ignore_case: bool,
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.contains(needles, ignore_case);
        self.when(condition, callback)
    }

    pub fn whenContainsAll(
        self,
        needles: &[&str],
        ignore_case: bool,
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.containsAll(needles, ignore_case);
        self.when(condition, callback)
    }

    pub fn whenStartsWith(self, needles: &[&str], callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.startsWith(needles);
        self.when(condition, callback)
    }

    pub fn whenDoesntStartWith(
        self,
        needles: &[&str],
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.doesntStartWith(needles);
        self.when(condition, callback)
    }

    pub fn whenEndsWith(self, needles: &[&str], callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.endsWith(needles);
        self.when(condition, callback)
    }

    pub fn whenDoesntEndWith(self, needles: &[&str], callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.doesntEndWith(needles);
        self.when(condition, callback)
    }

    pub fn whenExactly(self, expected: &str, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.exactly(expected);
        self.when(condition, callback)
    }

    pub fn whenNotExactly(self, expected: &str, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = !self.exactly(expected);
        self.when(condition, callback)
    }

    pub fn whenIs(
        self,
        patterns: &[&str],
        ignore_case: bool,
        callback: impl FnOnce(Self) -> Self,
    ) -> Self {
        let condition = self.is(patterns, ignore_case);
        self.when(condition, callback)
    }

    pub fn whenIsAscii(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.isAscii();
        self.when(condition, callback)
    }

    pub fn whenIsUlid(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.isUlid();
        self.when(condition, callback)
    }

    pub fn whenIsUuid(self, version: Option<u8>, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.isUuid(version);
        self.when(condition, callback)
    }

    pub fn whenEmpty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.isEmpty();
        self.when(condition, callback)
    }

    pub fn whenNotEmpty(self, callback: impl FnOnce(Self) -> Self) -> Self {
        let condition = self.isNotEmpty();
        self.when(condition, callback)
    }

    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        if condition { callback(self) } else { self }
    }

    pub fn whenTest(
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

    pub fn newLine(mut self, times: usize) -> Self {
        self.0.extend(std::iter::repeat_n('\n', times));
        self
    }

    pub fn charAt(&self, index: isize) -> Option<char> {
        Str::charAt(&self.0, index)
    }

    pub fn excerpt(&self, phrase: &str, radius: usize, omission: &str) -> Option<Self> {
        Str::excerpt(&self.0, phrase, radius, omission).map(Self)
    }

    pub fn ucsplit(&self) -> Vec<&str> {
        Str::ucsplit(&self.0)
    }

    pub fn wordCount(&self, additional: Option<&str>) -> usize {
        Str::wordCount(&self.0, additional)
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
        self.isMatch(&[pattern])
    }

    pub fn fromBase64(&self) -> Result<Vec<u8>, base64::DecodeError> {
        Str::fromBase64(&self.0)
    }

    pub fn toHtmlString(self) -> HtmlString {
        HtmlString::new(self.0)
    }

    pub fn toUri(&self) -> Result<Uri, InvalidUri> {
        Uri::of(&self.0)
    }
    pub fn classBasename(self) -> Self {
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
    pub fn stripTags(self, allowed: &[&str]) -> Self {
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
