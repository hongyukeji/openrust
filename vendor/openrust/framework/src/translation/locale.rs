use std::{borrow::Cow, collections::HashSet};

use icu_locale::LocaleCanonicalizer;
use icu_locale_core::Locale;

use super::{TranslationError, TranslationResult};

pub(super) fn language_tag(locale: &str) -> TranslationResult<Cow<'_, str>> {
    let tag = canonical_locale(locale)?;
    let locale: Locale = tag.parse().map_err(|_| TranslationError::InvalidLocale)?;
    Ok(Cow::Owned(locale.id.to_string()))
}

/// 规范化 Unicode BCP 47 标签；资源匹配及中文三语选择由消费者负责。
/// 不接受旧下划线形式，也不将地区标签改写为文字体系。
pub fn canonical_locale(locale: &str) -> TranslationResult<String> {
    if !locale
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(TranslationError::InvalidLocale);
    }
    let mut parsed: Locale = locale
        .parse()
        .map_err(|_| TranslationError::InvalidLocale)?;
    if !unique_subtags(locale) {
        return Err(TranslationError::InvalidLocale);
    }
    LocaleCanonicalizer::new_extended().canonicalize(&mut parsed);
    Ok(parsed.to_string())
}

// ICU 会保留第一个重复 Unicode／transform 键；入口显式拒绝，避免静默丢弃参数。
// 语法仍由 ICU 检查，这里只验证不能重复的子标签，不检查 private-use 内容。
fn unique_subtags(input: &str) -> bool {
    let input = input.to_ascii_lowercase();
    let mut extension = None;
    let mut in_keywords = false;
    let mut keys = HashSet::new();
    let mut attributes = HashSet::new();
    let mut variants = HashSet::new();
    for part in input.split('-').skip(1) {
        if part.len() == 1 {
            if part == "x" {
                break;
            }
            extension = Some(part);
            in_keywords = false;
            keys.clear();
            variants.clear();
            continue;
        }
        let key = part.len() == 2
            && (extension == Some("u")
                || (extension == Some("t") && part.as_bytes()[1].is_ascii_digit()));
        if key {
            in_keywords = true;
            if !keys.insert(part) {
                return false;
            }
        } else if extension == Some("u") && !in_keywords {
            if !attributes.insert(part) {
                return false;
            }
        } else if !in_keywords
            && matches!(extension, None | Some("t"))
            && (part.len() >= 5 || (part.len() == 4 && part.as_bytes()[0].is_ascii_digit()))
            && !variants.insert(part)
        {
            return false;
        }
    }
    true
}
