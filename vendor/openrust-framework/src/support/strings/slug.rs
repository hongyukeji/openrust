use super::{Str, ascii_fast_path};
use icu_properties::{
    CodePointMapData,
    props::{GeneralCategory, GeneralCategoryGroup},
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

// portable-ascii 2.0.2, commit 16c17671a804bb92602822113dd91fbc8a35d2af.
// The JSON serializes charsArrayWithSingleLanguageValues(false, false), each
// charsArrayWithOneLanguage(locale, false, false) delta, and ascii_language_max_key.php.
#[derive(Deserialize)]
struct AsciiData {
    base: HashMap<String, String>,
    locales: HashMap<String, HashMap<String, String>>,
    max_key_length: HashMap<String, usize>,
}

pub(super) fn ascii(value: &str, language: &str) -> String {
    static DATA: OnceLock<AsciiData> = OnceLock::new();
    let data = DATA.get_or_init(|| {
        serde_json::from_str(include_str!("portable-ascii-locales-2.0.2.json"))
            .expect("valid checked-in portable-ascii locale data")
    });
    let normalized = language.to_ascii_lowercase().replace('-', "_");
    let language = normalized
        .split_once('_')
        .and_then(|(first, rest)| rest.strip_prefix(first).map(|rest| (first, rest)))
        .filter(|(first, _)| !first.is_empty() && first.bytes().all(|b| b.is_ascii_lowercase()))
        .map_or(normalized.clone(), |(first, rest)| format!("{first}{rest}"));
    let localized = data.locales.get(&language);
    let max_length = data.max_key_length.get(&language).copied().unwrap_or(0);
    let characters: Vec<char> = value
        .chars()
        .filter(|ch| !ascii_fast_path(ch.encode_utf8(&mut [0; 4])))
        .collect();
    let mut output = value.to_owned();
    let mut replaced = HashSet::new();
    // The baseline visits original windows longest first and replaces each key globally.
    for length in (1..=max_length.clamp(3, 5)).rev() {
        for window in characters.windows(length) {
            let key: String = window.iter().collect();
            let replacement = localized
                .and_then(|map| map.get(&key))
                .or_else(|| data.base.get(&key));
            if let Some(replacement) = replacement
                && output.contains(&key)
                && replaced.insert(key.clone())
            {
                output = output.replace(&key, replacement);
            }
        }
    }
    if localized.is_none() {
        output = Str::transliterate_with_unknown(&output, None);
    }
    output = output.replace("\n\r", " ").replace(['\n', '\r', '\t'], " ");
    output.retain(|ch| ascii_fast_path(ch.encode_utf8(&mut [0; 4])));
    output
}

pub(super) fn slug(
    value: &str,
    separator: &str,
    language: Option<&str>,
    dictionary: &[(&str, &str)],
) -> String {
    let mut value = match language.filter(|language| !matches!(*language, "" | "0")) {
        Some(language) => ascii(value, language),
        None => value.into(),
    };
    let flip = if separator == "-" { '_' } else { '-' };
    let mut previous_flip = false;
    let mut flipped = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch != flip {
            flipped.push(ch);
        } else if !previous_flip {
            flipped.push_str(separator);
        }
        previous_flip = ch == flip;
    }
    value = flipped;
    for &(key, replacement) in dictionary {
        if !key.is_empty() {
            value = value.replace(key, &format!("{separator}{replacement}{separator}"));
        }
    }

    let categories = CodePointMapData::<GeneralCategory>::new();
    let letters_and_numbers = GeneralCategoryGroup::Letter.union(GeneralCategoryGroup::Number);
    let mut output = String::with_capacity(value.len());
    let mut pending_separator = false;
    for ch in value.to_lowercase().chars() {
        if separator.contains(ch) || ch.is_whitespace() {
            pending_separator = !output.is_empty();
        } else if letters_and_numbers.contains(categories.get(ch)) {
            if pending_separator {
                output.push_str(separator);
                pending_separator = false;
            }
            output.push(ch);
        }
    }
    output
}
