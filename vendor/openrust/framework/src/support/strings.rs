use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

use super::{MarkdownOptions, Stringable, markdown};

mod display_width;
mod pattern;
mod random;
mod slug;
mod strict_transliteration;
mod text_layout;

pub use random::{EmptyPasswordAlphabet, PasswordOptions};
pub use text_layout::WordWrapError;

/// Unicode string helpers with Laravel-compatible names.
pub struct Str;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransliterationData {
    banks: BTreeMap<u32, Vec<Option<String>>>,
    whitespace: Vec<(String, String)>,
    msword: Vec<(String, String)>,
    bidi: Vec<u32>,
}
static DATA: OnceLock<TransliterationData> = OnceLock::new();

fn data() -> &'static TransliterationData {
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("strings/portable-ascii-2.0.2.json"))
            .expect("valid checked-in portable-ascii data")
    })
}

#[allow(non_snake_case, reason = "Matches Laravel's Str API")]
impl Str {
    pub fn of(value: impl Into<String>) -> Stringable {
        Stringable::from(value.into())
    }

    pub fn after<'a>(subject: &'a str, search: &str) -> &'a str {
        if search.is_empty() {
            return subject;
        }
        subject.split_once(search).map_or(subject, |(_, tail)| tail)
    }

    pub fn afterLast<'a>(subject: &'a str, search: &str) -> &'a str {
        if search.is_empty() {
            return subject;
        }
        subject
            .rsplit_once(search)
            .map_or(subject, |(_, tail)| tail)
    }

    pub fn before<'a>(subject: &'a str, search: &str) -> &'a str {
        if search.is_empty() {
            return subject;
        }
        subject.split_once(search).map_or(subject, |(head, _)| head)
    }

    pub fn beforeLast<'a>(subject: &'a str, search: &str) -> &'a str {
        if search.is_empty() {
            return subject;
        }
        subject
            .rsplit_once(search)
            .map_or(subject, |(head, _)| head)
    }

    pub fn between<'a>(subject: &'a str, from: &str, to: &str) -> &'a str {
        if from.is_empty() || to.is_empty() {
            return subject;
        }
        Self::beforeLast(Self::after(subject, from), to)
    }

    pub fn betweenFirst<'a>(subject: &'a str, from: &str, to: &str) -> &'a str {
        if from.is_empty() || to.is_empty() {
            return subject;
        }
        Self::before(Self::after(subject, from), to)
    }

    pub fn contains(subject: &str, needles: &[&str], ignore_case: bool) -> bool {
        if ignore_case {
            let subject = subject.to_lowercase();
            needles
                .iter()
                .any(|needle| !needle.is_empty() && subject.contains(&needle.to_lowercase()))
        } else {
            needles
                .iter()
                .any(|needle| !needle.is_empty() && subject.contains(needle))
        }
    }

    pub fn length(value: &str) -> usize {
        value.chars().count()
    }

    pub fn charAt(value: &str, index: isize) -> Option<char> {
        if let Ok(index) = usize::try_from(index) {
            value.chars().nth(index)
        } else {
            value.chars().rev().nth(index.unsigned_abs() - 1)
        }
    }

    pub fn lower(value: &str) -> String {
        value.to_lowercase()
    }

    pub fn upper(value: &str) -> String {
        value.to_uppercase()
    }

    pub fn startsWith(subject: &str, needles: &[&str]) -> bool {
        needles
            .iter()
            .any(|needle| !needle.is_empty() && subject.starts_with(needle))
    }

    pub fn endsWith(subject: &str, needles: &[&str]) -> bool {
        needles
            .iter()
            .any(|needle| !needle.is_empty() && subject.ends_with(needle))
    }

    pub fn containsAll(subject: &str, needles: &[&str], ignore_case: bool) -> bool {
        needles
            .iter()
            .all(|needle| Self::contains(subject, &[*needle], ignore_case))
    }

    pub fn is(patterns: &[&str], value: &str, ignore_case: bool) -> bool {
        patterns.iter().any(|pattern| {
            if *pattern == "*" || *pattern == value {
                return true;
            }
            if ignore_case && pattern.to_lowercase() == value.to_lowercase() {
                return true;
            }
            let pattern = pattern
                .split('*')
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join(".*");
            regex::RegexBuilder::new(&format!(r"\A{pattern}\z"))
                .case_insensitive(ignore_case)
                .dot_matches_new_line(true)
                .build()
                .is_ok_and(|pattern| pattern.is_match(value))
        })
    }

    /// Slices by Unicode scalar offsets. Negative positions and lengths count from the end.
    pub fn substr(value: &str, start: isize, length: Option<isize>) -> &str {
        let count = Self::length(value);
        let start = if start < 0 {
            count.saturating_sub(start.unsigned_abs())
        } else {
            start as usize
        };
        if start >= count {
            return "";
        }
        let end = match length {
            Some(length) if length < 0 => count.saturating_sub(length.unsigned_abs()),
            Some(length) => start.saturating_add(length as usize).min(count),
            None => count,
        };
        if end <= start {
            return "";
        }
        let mut indices = value.char_indices();
        let start_byte = indices.nth(start).expect("in-range start").0;
        let end_byte = indices
            .nth(end - start - 1)
            .map_or(value.len(), |(index, _)| index);
        &value[start_byte..end_byte]
    }

    pub fn substrCount(value: &str, needle: &str) -> usize {
        if needle.is_empty() {
            0
        } else {
            value.matches(needle).count()
        }
    }

    pub fn start(value: &str, prefix: &str) -> String {
        if prefix.is_empty() {
            return value.into();
        }
        format!("{prefix}{}", value.trim_start_matches(prefix))
    }

    pub fn finish(value: &str, suffix: &str) -> String {
        if suffix.is_empty() {
            return value.into();
        }
        format!("{}{suffix}", value.trim_end_matches(suffix))
    }

    pub fn wrap(value: &str, before: &str, after: Option<&str>) -> String {
        format!("{before}{value}{}", after.unwrap_or(before))
    }

    pub fn unwrap<'a>(value: &'a str, before: &str, after: Option<&str>) -> &'a str {
        let value = value.strip_prefix(before).unwrap_or(value);
        value.strip_suffix(after.unwrap_or(before)).unwrap_or(value)
    }

    pub fn replaceFirst(search: &str, replace: &str, value: &str) -> String {
        if search.is_empty() {
            value.into()
        } else {
            value.replacen(search, replace, 1)
        }
    }

    pub fn replaceLast(search: &str, replace: &str, value: &str) -> String {
        if search.is_empty() {
            return value.into();
        }
        value.rsplit_once(search).map_or_else(
            || value.into(),
            |(left, right)| format!("{left}{replace}{right}"),
        )
    }

    pub fn repeat(value: &str, count: usize) -> String {
        value.repeat(count)
    }

    pub fn reverse(value: &str) -> String {
        value.chars().rev().collect()
    }

    pub fn isAscii(value: &str) -> bool {
        value.is_ascii()
    }

    pub fn isJson(value: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(value).is_ok()
    }

    pub fn toBase64(value: impl AsRef<[u8]>) -> String {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        STANDARD.encode(value)
    }

    pub fn fromBase64(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        STANDARD.decode(value)
    }

    pub fn words(value: &str, limit: usize, end: &str) -> String {
        if limit == 0 {
            return value.into();
        }
        let mut count = 0;
        let mut in_word = false;
        for (index, character) in value.char_indices() {
            if character.is_whitespace() {
                in_word = false;
            } else if !in_word {
                if count == limit {
                    return format!("{}{end}", value[..index].trim_end());
                }
                count += 1;
                in_word = true;
            }
        }
        value.into()
    }

    pub fn studly(value: &str, normalize: bool) -> String {
        let mut result = String::with_capacity(value.len());
        for word in value
            .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
            .filter(|word| !word.is_empty())
        {
            let mut chars = word.chars();
            let first = chars.next().expect("non-empty word");
            result.extend(first.to_uppercase());
            if normalize && word.bytes().all(|byte| byte.is_ascii_uppercase()) {
                result.push_str(&chars.as_str().to_ascii_lowercase());
            } else {
                result.push_str(chars.as_str());
            }
        }
        result
    }

    pub fn camel(value: &str) -> String {
        let studly = Self::studly(value, false);
        let mut chars = studly.chars();
        let Some(first) = chars.next() else {
            return studly;
        };
        first.to_lowercase().chain(chars).collect()
    }

    pub fn snake(value: &str, delimiter: &str) -> String {
        let mut result = String::with_capacity(value.len());
        let mut begins_word = true;
        for character in value.chars() {
            if character.is_whitespace() {
                begins_word = true;
                continue;
            }
            let character = if begins_word {
                character.to_ascii_uppercase()
            } else {
                character
            };
            if character.is_ascii_uppercase() && !result.is_empty() {
                result.push_str(delimiter);
            }
            result.extend(character.to_lowercase());
            begins_word = false;
        }
        result
    }

    pub fn kebab(value: &str) -> String {
        Self::snake(value, "-")
    }

    /// Transliterates text using the default portable-ascii mapping.
    pub fn chopStart<'a>(value: &'a str, prefixes: &[&str]) -> &'a str {
        prefixes
            .iter()
            .filter(|prefix| !prefix.is_empty())
            .find_map(|prefix| value.strip_prefix(prefix))
            .unwrap_or(value)
    }

    pub fn chopEnd<'a>(value: &'a str, suffixes: &[&str]) -> &'a str {
        suffixes
            .iter()
            .filter(|suffix| !suffix.is_empty())
            .find_map(|suffix| value.strip_suffix(suffix))
            .unwrap_or(value)
    }

    pub fn replaceStart(search: &str, replacement: &str, value: &str) -> String {
        if !search.is_empty()
            && let Some(tail) = value.strip_prefix(search)
        {
            return format!("{replacement}{tail}");
        }
        value.to_owned()
    }

    pub fn replaceEnd(search: &str, replacement: &str, value: &str) -> String {
        if !search.is_empty()
            && let Some(head) = value.strip_suffix(search)
        {
            return format!("{head}{replacement}");
        }
        value.to_owned()
    }

    pub fn deduplicate(value: &str, characters: &[char]) -> String {
        characters.iter().fold(value.to_owned(), |input, target| {
            let mut previous = None;
            input
                .chars()
                .filter(|current| {
                    let repeated = current == target && previous == Some(*current);
                    previous = Some(*current);
                    !repeated
                })
                .collect()
        })
    }

    pub fn position(value: &str, search: &str, offset: isize) -> Option<usize> {
        let length = Self::length(value);
        let offset = if offset < 0 {
            length.checked_sub(offset.unsigned_abs())?
        } else {
            offset.unsigned_abs()
        };
        if offset > length {
            return None;
        }
        let tail = Self::substr(value, offset as isize, None);
        let found = tail.find(search)?;
        Some(offset + Self::length(&tail[..found]))
    }

    pub fn take(value: &str, limit: isize) -> &str {
        if limit < 0 {
            Self::substr(value, limit, None)
        } else {
            Self::substr(value, 0, Some(limit))
        }
    }

    pub fn mask(value: &str, token: &str, start: isize, length: Option<isize>) -> String {
        let masked = Self::length(Self::substr(value, start, length));
        if token.is_empty() || masked == 0 {
            return value.to_owned();
        }
        let start = if start < 0 {
            Self::length(value).saturating_sub(start.unsigned_abs())
        } else {
            start.unsigned_abs()
        };
        let mut output = Self::substr(value, 0, Some(start as isize)).to_owned();
        output.extend(token.chars().cycle().take(masked));
        output.push_str(Self::substr(value, (start + masked) as isize, None));
        output
    }

    pub fn trim(value: &str) -> &str {
        value.trim_matches(super::stringable::trim_character)
    }

    pub fn ltrim(value: &str) -> &str {
        value.trim_start_matches(super::stringable::trim_character)
    }

    pub fn rtrim(value: &str) -> &str {
        value.trim_end_matches(super::stringable::trim_character)
    }

    pub fn squish(value: &str) -> String {
        Self::trim(value)
            .split(|c: char| c.is_whitespace() || matches!(c, '\u{3164}' | '\u{1160}'))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn r#match<'a>(pattern: &str, subject: &'a str) -> Result<&'a str, regex::Error> {
        pattern::first(pattern, subject)
    }

    pub fn matchAll<'a>(pattern: &str, subject: &'a str) -> Result<Vec<&'a str>, regex::Error> {
        pattern::all(pattern, subject)
    }

    pub fn isMatch(patterns: &[&str], subject: &str) -> Result<bool, regex::Error> {
        pattern::any(patterns, subject)
    }

    pub fn replaceMatches(
        pattern: &str,
        replacement: &str,
        subject: &str,
        count: Option<usize>,
    ) -> Result<String, regex::Error> {
        pattern::replace(pattern, replacement, subject, count)
    }

    pub fn replaceMatchesWith(
        pattern: &str,
        replacement: impl FnMut(&regex::Captures<'_>) -> String,
        subject: &str,
        count: Option<usize>,
    ) -> Result<String, regex::Error> {
        pattern::replace_with(pattern, replacement, subject, count)
    }

    pub fn width(value: &str) -> usize {
        value.chars().map(text_layout::width).sum()
    }

    pub fn limit(value: &str, columns: usize, end: &str, preserve_words: bool) -> String {
        text_layout::limit(value, columns, end, preserve_words)
    }

    pub fn wordWrap(
        value: &str,
        columns: usize,
        line_break: &str,
        cut_long_words: bool,
    ) -> Result<String, WordWrapError> {
        text_layout::wrap(value, columns, line_break, cut_long_words)
    }

    pub fn random(length: usize) -> String {
        random::alphanumeric(length)
    }

    pub fn password(
        length: usize,
        options: PasswordOptions,
    ) -> Result<String, EmptyPasswordAlphabet> {
        random::password(length, options)
    }

    pub fn uuid() -> Result<uuid::Uuid, aws_lc_rs::error::Unspecified> {
        let mut bytes = [0u8; 16];
        aws_lc_rs::rand::fill(&mut bytes)?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Ok(uuid::Uuid::from_bytes(bytes))
    }

    pub fn uuid7() -> uuid::Uuid {
        uuid::Uuid::now_v7()
    }

    pub fn ulid() -> ulid::Ulid {
        ulid::Ulid::new()
    }

    pub fn isUuid(value: &str, version: Option<u8>) -> bool {
        if value.len() != 36 {
            return false;
        }
        let Ok(value) = uuid::Uuid::parse_str(value) else {
            return false;
        };
        match version {
            None => true,
            Some(0) => value.is_nil(),
            Some(version) => {
                value.get_variant() == uuid::Variant::RFC4122
                    && value.get_version_num() == usize::from(version)
            }
        }
    }

    pub fn isUlid(value: &str) -> bool {
        value
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b'0'..=b'7'))
            && ulid::Ulid::from_string(value).is_ok()
    }

    /// Renders GFM with HTML escaping and safe link schemes by default.
    pub fn markdown(value: &str) -> String {
        Self::markdownWith(value, MarkdownOptions::default())
    }

    pub fn markdownWith(value: &str, options: MarkdownOptions) -> String {
        markdown::render(value, options, false)
    }

    pub fn inlineMarkdown(value: &str) -> String {
        Self::inlineMarkdownWith(value, MarkdownOptions::default())
    }

    pub fn inlineMarkdownWith(value: &str, options: MarkdownOptions) -> String {
        markdown::render(value, options, true)
    }

    /// Inflects an English noun unless the count is 1 or -1.
    pub fn plural(value: &str, count: i64) -> String {
        if count.unsigned_abs() == 1
            || value.eq_ignore_ascii_case("recommended")
            || value.eq_ignore_ascii_case("related")
            || !value.chars().next_back().is_some_and(|end| {
                end.is_ascii_alphanumeric() || ('\u{80}'..='\u{ffff}').contains(&end)
            })
        {
            return value.to_owned();
        }
        pluralizer::pluralize(value, 2, false)
    }

    pub fn singular(value: &str) -> String {
        pluralizer::pluralize(value, 1, false)
    }

    pub fn pluralStudly(value: &str, count: i64) -> String {
        let start = value
            .char_indices()
            .rev()
            .find(|(_, char)| char.is_ascii_uppercase())
            .map_or(0, |(start, _)| start);
        let (prefix, last) = value.split_at(start);
        format!("{prefix}{}", Self::plural(last, count))
    }

    pub fn pluralPascal(value: &str, count: i64) -> String {
        Self::pluralStudly(value, count)
    }

    /// Formats a counted English noun with English digit grouping.
    pub fn counted(value: &str, count: i64) -> String {
        let digits = count.unsigned_abs().to_string();
        let mut output = String::new();
        if count.is_negative() {
            output.push('-');
        }
        for (index, digit) in digits.bytes().enumerate() {
            if index != 0 && (digits.len() - index).is_multiple_of(3) {
                output.push(',');
            }
            output.push(char::from(digit));
        }
        output.push(' ');
        output.push_str(&Self::plural(value, count));
        output
    }

    pub fn ascii(value: &str, language: &str) -> String {
        slug::ascii(value, language)
    }

    pub fn slug(value: &str) -> String {
        Self::slugWith(value, "-", Some("en"), &[("@", "at")])
    }

    /// Converts a title with an ordered dictionary. `None` keeps Unicode letters and numbers.
    pub fn slugWith(
        value: &str,
        separator: &str,
        language: Option<&str>,
        dictionary: &[(&str, &str)],
    ) -> String {
        slug::slug(value, separator, language, dictionary)
    }

    pub fn transliterate(value: &str) -> String {
        Self::transliterateWithUnknown(value, Some("?"))
    }

    /// Selects a replacement for unknown characters or preserves them with `None`.
    pub fn transliterateWithUnknown(value: &str, unknown: Option<&str>) -> String {
        Self::transliterateWithOptions(value, unknown, false)
    }

    pub fn transliterateWithOptions(value: &str, unknown: Option<&str>, strict: bool) -> String {
        if ascii_fast_path(value) {
            return value.into();
        }
        let data = data();
        let cleaned = clean(value, data);
        if cleaned != value && ascii_fast_path(&cleaned) {
            return cleaned;
        }
        let cleaned = if strict {
            strict_transliteration::transliterate(&cleaned)
        } else {
            cleaned
        };
        if ascii_fast_path(&cleaned) {
            return cleaned;
        }
        let mut output = String::with_capacity(cleaned.len());
        for character in cleaned.chars() {
            if character.is_ascii() {
                output.push(character);
                continue;
            }
            let codepoint = u32::from(character);
            let mapped = data
                .banks
                .get(&(codepoint >> 8))
                .and_then(|bank| bank.get((codepoint & 255) as usize))
                .and_then(Option::as_deref);
            let replacement = match mapped {
                Some("") if unknown.is_none() => None,
                Some("[?]" | "[?] ") | None => unknown,
                value => value,
            };
            if let Some(text) = replacement {
                output.push_str(text);
            } else {
                output.push(character);
            }
        }
        output
    }
}

fn ascii_fast_path(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, 0x09 | 0x10 | 0x13 | 0x0a | 0x0d | 0x20..=0x7e))
}
fn clean(value: &str, data: &TransliterationData) -> String {
    let mut cleaned: String = value
        .chars()
        .filter(|character| !data.bidi.contains(&u32::from(*character)))
        .collect();
    for (search, replacement) in &data.whitespace {
        cleaned = cleaned.replace(search, replacement);
    }
    for (search, replacement) in &data.msword {
        cleaned = cleaned.replace(search, replacement);
    }
    cleaned.retain(|character| !matches!(character, '\u{0000}'..='\u{0008}' | '\u{000b}'..='\u{000c}' | '\u{000e}'..='\u{001f}' | '\u{007f}'));
    cleaned
}
