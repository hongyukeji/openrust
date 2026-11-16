use std::sync::OnceLock;

use regex::{NoExpand, Regex, RegexBuilder};

use super::Str;

#[derive(Clone, Copy)]
pub(super) enum Padding {
    Left,
    Right,
    Both,
}

pub(super) fn pad(value: &str, length: usize, padding: &str, position: Padding) -> String {
    let missing = length.saturating_sub(value.chars().count());
    if missing == 0 || padding.is_empty() {
        return value.to_owned();
    }
    let left = match position {
        Padding::Left => missing,
        Padding::Right => 0,
        Padding::Both => missing / 2,
    };
    let mut output = String::new();
    output.extend(padding.chars().cycle().take(left));
    output.push_str(value);
    output.extend(padding.chars().cycle().take(missing - left));
    output
}

pub(super) fn replace(value: &str, replacements: &[(&str, &str)], case_sensitive: bool) -> String {
    let mut value = value.to_owned();
    for &(search, replacement) in replacements {
        if search.is_empty() {
            continue;
        }
        if case_sensitive {
            value = value.replace(search, replacement);
        } else if search.is_ascii() {
            // PHP uses ASCII-only case matching when the search term is ASCII.
            let subject = value.to_ascii_lowercase();
            let search = search.to_ascii_lowercase();
            let mut output = String::with_capacity(value.len());
            let mut previous = 0;
            for (index, item) in subject.match_indices(&search) {
                output.push_str(&value[previous..index]);
                output.push_str(replacement);
                previous = index + item.len();
            }
            output.push_str(&value[previous..]);
            value = output;
        } else if let Ok(pattern) = RegexBuilder::new(&regex::escape(search))
            .case_insensitive(true)
            .build()
        {
            value = pattern
                .replace_all(&value, NoExpand(replacement))
                .into_owned();
        }
    }
    value
}

pub(super) fn excerpt(value: &str, phrase: &str, radius: usize, omission: &str) -> Option<String> {
    let pattern = RegexBuilder::new(&format!(r"\A(.*?)({})(.*)\n?\z", regex::escape(phrase)))
        .case_insensitive(true)
        .build()
        .ok()?;
    let parts = pattern.captures(value)?;
    let radius = isize::try_from(radius).unwrap_or(isize::MAX);
    let initial = |ch: char| matches!(ch, ' ' | '\0' | '\n' | '\r' | '\t' | '\x0b');
    let before = parts[1].trim_start_matches(initial);
    let after = parts[3].trim_end_matches(initial);
    let left = if radius == 0 {
        ""
    } else {
        Str::ltrim(Str::substr(before, -radius, None))
    };
    let right = Str::rtrim(Str::substr(after, 0, Some(radius)));
    let left_marker = if left == before { "" } else { omission };
    let right_marker = if right == after { "" } else { omission };
    Some(format!(
        "{left_marker}{left}{}{right}{right_marker}",
        &parts[2]
    ))
}

pub(super) fn swap(value: &str, replacements: &[(&str, &str)]) -> String {
    let mut entries: Vec<_> = replacements
        .iter()
        .filter(|(key, _)| !key.is_empty())
        .collect();
    entries.sort_by_key(|(key, _)| std::cmp::Reverse(key.len()));
    let mut output = String::new();
    let mut rest = value;
    while let Some(ch) = rest.chars().next() {
        if let Some(&&(key, replacement)) = entries.iter().find(|(key, _)| rest.starts_with(key)) {
            output.push_str(replacement);
            rest = &rest[key.len()..];
        } else {
            output.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    output
}

// Laravel's word count calls PHP's byte-oriented implementation in the C locale.
pub(super) fn word_count(value: &str, additional: &str) -> usize {
    let mut mask = [false; 256];
    let mut extras = additional.as_bytes();
    while let Some((&first, rest)) = extras.split_first() {
        match rest {
            [b'.', b'.', last, tail @ ..] if first <= *last => {
                mask[usize::from(first)..=usize::from(*last)].fill(true);
                extras = tail;
            }
            _ => {
                mask[usize::from(first)] = true;
                extras = rest;
            }
        }
    }
    let mut bytes = value.as_bytes();
    if let Some((&first, rest)) = bytes.split_first()
        && matches!(first, b'\'' | b'-')
        && !mask[usize::from(first)]
    {
        bytes = rest;
    }
    if !mask[usize::from(b'-')] {
        bytes = bytes.strip_suffix(b"-").unwrap_or(bytes);
    }
    bytes
        .split(|&byte| {
            !(byte.is_ascii_alphabetic() || mask[usize::from(byte)] || matches!(byte, b'\'' | b'-'))
        })
        .filter(|part| !part.is_empty())
        .count()
}

pub(in crate::support) fn strip_tags(value: &str, allowed: &[&str]) -> String {
    static TAGS: OnceLock<Regex> = OnceLock::new();
    let tags = TAGS.get_or_init(|| {
        Regex::new(r#"(?s:<!--.*?-->|<\?.*?\?>)|</?([A-Za-z][A-Za-z0-9:_-]*)(?:"[^"]*"|'[^']*'|[^'"<>])*>"#)
            .expect("tag matching expression")
    });
    tags.replace_all(value, |captures: &regex::Captures<'_>| {
        let keep = captures.get(1).is_some_and(|name| {
            allowed.iter().any(|allowed| {
                name.as_str()
                    .eq_ignore_ascii_case(allowed.trim_matches(['<', '>']))
            })
        });
        if keep {
            captures[0].to_owned()
        } else {
            String::new()
        }
    })
    .into_owned()
}

pub(super) fn title(value: &str) -> String {
    static WORD: OnceLock<Regex> = OnceLock::new();
    let words = WORD.get_or_init(|| {
        Regex::new(r"[\p{L}\p{M}\p{N}]+(?:['’][\p{L}\p{M}\p{N}]+)*").expect("valid word pattern")
    });
    words
        .replace_all(value, |captures: &regex::Captures<'_>| {
            let word = captures[0].to_lowercase();
            let Some((index, ch)) = word
                .char_indices()
                .find(|(_, ch)| ch.is_lowercase() || ch.is_uppercase())
            else {
                return word;
            };
            let mut output = word[..index].to_owned();
            let mut capital = ch.to_uppercase();
            if let Some(first) = capital.next() {
                output.push(first);
            }
            output.extend(capital.flat_map(char::to_lowercase));
            output.push_str(&word[index + ch.len_utf8()..]);
            output
        })
        .into_owned()
}

pub(super) fn apa(value: &str) -> String {
    const MINOR: &[&str] = &[
        "and", "as", "but", "for", "if", "nor", "or", "so", "yet", "a", "an", "the", "at", "by",
        "in", "of", "off", "on", "per", "to", "up", "via", "et", "ou", "un", "une", "la", "le",
        "les", "de", "du", "des", "par", "à",
    ];
    if value
        .trim_matches([' ', '\t', '\n', '\r', '\x0b', '\0'])
        .is_empty()
    {
        return value.to_owned();
    }
    let mut first_or_after_punctuation = true;
    let mut words = Vec::new();
    for word in value.split_whitespace() {
        let lowercase = word.to_lowercase();
        let result = if lowercase.contains('-') {
            lowercase
                .split('-')
                .map(|part| {
                    if MINOR.contains(&part) {
                        part.to_owned()
                    } else {
                        uppercase_first(part)
                    }
                })
                .collect::<Vec<_>>()
                .join("-")
        } else if first_or_after_punctuation || !MINOR.contains(&lowercase.as_str()) {
            uppercase_first(&lowercase)
        } else {
            lowercase
        };
        first_or_after_punctuation = result.ends_with(['.', '!', '?', ':', '—', ',']);
        words.push(result);
    }
    words.join(" ")
}

fn uppercase_first(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        format!("{}{}", first.to_uppercase(), chars.as_str())
    })
}

pub(super) const DEFAULT_WORD_SEPARATORS: &str = " \t\r\n\x0c\x0b";

pub(super) fn ucwords(value: &str, separators: &str) -> String {
    let mut boundary = true;
    let mut output = String::new();
    for ch in value.chars() {
        if boundary && ch.is_lowercase() {
            output.extend(ch.to_uppercase());
        } else {
            output.push(ch);
        }
        boundary = separators.contains(ch);
    }
    output
}

pub(super) fn is_url(value: &str, protocols: &[&str]) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    // URL parsing normalizes several characters that Laravel's grammar rejects.
    if rest.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '\\')
        || value.as_bytes().iter().enumerate().any(|(index, byte)| {
            *byte == b'%'
                && !value
                    .as_bytes()
                    .get(index + 1..index + 3)
                    .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        })
    {
        return false;
    }
    let allowed = if protocols.is_empty() {
        include_str!("url-schemes.txt")
            .lines()
            .any(|item| item.eq_ignore_ascii_case(scheme))
    } else {
        protocols
            .iter()
            .any(|item| item.eq_ignore_ascii_case(scheme))
    };
    allowed && url::Url::parse(value).is_ok_and(|url| url.has_host())
}
