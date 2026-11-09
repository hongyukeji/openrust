//! 固定 Laravel 消息分支语法和旧版地区复数规则。
//!
//! 源码基线为 Laravel Framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181` 的
//! [MessageSelector.php](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Translation/MessageSelector.php)。
//! 复数规则来自 Zend Framework（2010-09-25），采用 New BSD License。
//! Copyright (c) 2005–2010 Zend Technologies USA Inc.

use std::cmp::Ordering;

use crate::support::{InputNumber, scalars};
use crate::{Error, Result};

use super::Locale;

pub(super) fn select<'a>(line: &'a str, count: InputNumber, locale: &Locale) -> Result<&'a str> {
    let segments = line.split('|');
    let matched = segments
        .clone()
        .filter_map(condition_prefix)
        .find_map(|(condition, body)| matches_condition(count, condition).then_some(body));
    if let Some(body) = matched {
        return Ok(body.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']));
    }

    let index = plural_rule(locale.as_str()).map_or(Ok(0), |rule| rule.index(count))?;
    let mut segments = segments.map(|part| condition_prefix(part).map_or(part, |(_, body)| body));
    let first = segments.next().unwrap_or_default();
    Ok(if index == 0 {
        first
    } else {
        segments.nth(index - 1).unwrap_or(first)
    })
}

// 固定 PCRE 允许两种括号任意配对，也识别空条件。普通文本仅剥离这个前缀。
fn condition_prefix(part: &str) -> Option<(&str, &str)> {
    let part = part.strip_prefix(['{', '['])?;
    let end = part.find(|character: char| {
        !matches!(character, '0'..='9' | '-' | '?' | '|' | '*' | ',' | '.')
    })?;
    let (condition, suffix) = part.split_at(end);
    Some((condition, suffix.strip_prefix(['}', ']'])?))
}

fn matches_condition(count: InputNumber, condition: &str) -> bool {
    if let Some((from, to)) = condition.split_once(',') {
        let starts_at_or_below = from == "*" || compare_bound(count, from).is_ge();
        let ends_at_or_above = to == "*" || compare_bound(count, to).is_le();
        if starts_at_or_below && ends_at_or_above {
            return true;
        }
    }
    compare_bound(count, condition).is_eq()
}

fn compare_bound(count: InputNumber, bound: &str) -> Ordering {
    if let Some(bound) = InputNumber::numeric_string(bound) {
        return compare_numbers(count, bound);
    }
    // 匹配前缀语法但非数值的边界走 PHP number/string 的字节比较。
    match count {
        InputNumber::Integer(count) => count.to_string().as_str().cmp(bound),
        InputNumber::Float(count) => scalars::double_string(count).as_str().cmp(bound),
    }
}

fn compare_numbers(left: InputNumber, right: InputNumber) -> Ordering {
    match (left, right) {
        (InputNumber::Integer(left), InputNumber::Integer(right)) => left.cmp(&right),
        // 调用入口拒绝非有限数。超范围的数值边界仍按 PHP 双精度比较。
        _ => left
            .as_float()
            .partial_cmp(&right.as_float())
            .unwrap_or(Ordering::Less),
    }
}

#[derive(Clone, Copy)]
enum PluralRule {
    Invariant,
    OneOther,
    ZeroOrOne,
    CzechSlovak,
    Irish,
    Welsh,
    Modulo(ModuloRule),
}

#[derive(Clone, Copy)]
enum ModuloRule {
    EastSlavic,
    Lithuanian,
    Slovenian,
    Macedonian,
    Maltese,
    Latvian,
    Polish,
    Romanian,
    Arabic,
}

const TWO_TO_63: f64 = 9_223_372_036_854_775_808.0;

fn is_count(count: InputNumber, expected: i64) -> bool {
    compare_numbers(count, InputNumber::Integer(expected)).is_eq()
}

impl PluralRule {
    fn index(self, count: InputNumber) -> Result<usize> {
        let count = match count {
            InputNumber::Integer(value) => value
                .checked_abs()
                .map_or(InputNumber::Float(TWO_TO_63), InputNumber::Integer),
            InputNumber::Float(value) => InputNumber::Float(value.abs()),
        };
        let is = |number| is_count(count, number);
        Ok(match self {
            Self::Invariant => 0,
            Self::OneOther => usize::from(!is(1)),
            Self::ZeroOrOne => usize::from(!is(0) && !is(1)),
            Self::CzechSlovak | Self::Irish | Self::Welsh if is(1) => 0,
            Self::CzechSlovak if (2.0..=4.0).contains(&count.as_float()) => 1,
            Self::Irish | Self::Welsh if is(2) => 1,
            Self::Welsh if is(8) || is(11) => 2,
            Self::Welsh => 3,
            Self::CzechSlovak | Self::Irish => 2,
            Self::Modulo(rule) => rule.index(count)?,
        })
    }
}

impl ModuloRule {
    fn index(self, count: InputNumber) -> Result<usize> {
        let is = |number| is_count(count, number);
        match self {
            Self::Maltese | Self::Polish | Self::Romanian if is(1) => return Ok(0),
            Self::Latvian | Self::Arabic if is(0) => return Ok(0),
            Self::Maltese | Self::Romanian if is(0) => return Ok(1),
            Self::Arabic if is(1) => return Ok(1),
            Self::Arabic if is(2) => return Ok(2),
            _ => {}
        }

        // PHP 8.5 在显式 int 转换超界时发出 E_WARNING，Laravel 默认将其转为异常。
        let integer = match count {
            InputNumber::Integer(value) => value,
            InputNumber::Float(value) if value < TWO_TO_63 => {
                scalars::integer(&serde_json::Value::from(value))
            }
            InputNumber::Float(_) => {
                return Err(Error::custom_with_status(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "translation_choice_count_out_of_range",
                    "复数规则无法将计数转换为 PHP 64 位整数",
                ));
            }
        };
        let mod10 = integer % 10;
        let mod100 = integer % 100;
        let slavic_one = mod10 == 1 && mod100 != 11;
        let outside_teens = !(10..20).contains(&mod100);
        Ok(match self {
            Self::EastSlavic | Self::Lithuanian if slavic_one => 0,
            Self::EastSlavic if (2..=4).contains(&mod10) && outside_teens => 1,
            Self::Lithuanian if mod10 >= 2 && outside_teens => 1,
            Self::Slovenian => match mod100 {
                1 => 0,
                2 => 1,
                3 | 4 => 2,
                _ => 3,
            },
            Self::Macedonian => usize::from(mod10 != 1),
            Self::Maltese if (2..=10).contains(&mod100) => 1,
            Self::Maltese if (11..20).contains(&mod100) => 2,
            Self::Maltese => 3,
            Self::Latvian if slavic_one => 1,
            Self::Polish if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) => 1,
            Self::Romanian if (1..20).contains(&mod100) => 1,
            Self::Arabic if (3..=10).contains(&mod100) => 3,
            Self::Arabic if (11..=99).contains(&mod100) => 4,
            Self::Arabic => 5,
            Self::EastSlavic | Self::Lithuanian | Self::Latvian | Self::Polish | Self::Romanian => {
                2
            }
        })
    }
}

// 固定 Laravel MessageSelector::getPluralIndex 的完整地区表。Locale 使用 BCP 47
// 连字符，因此直接匹配规范拼写。不将未列出的地区或书写系统扩展降级为语言规则。
#[allow(clippy::too_many_lines, reason = "保持固定上游地区分组可逐项核对")]
fn plural_rule(locale: &str) -> Option<PluralRule> {
    match locale {
        "az" | "az-AZ" | "bo" | "bo-CN" | "bo-IN" | "dz" | "dz-BT" | "id" | "id-ID" | "ja"
        | "ja-JP" | "jv" | "ka" | "ka-GE" | "km" | "km-KH" | "kn" | "kn-IN" | "ko" | "ko-KR"
        | "ms" | "ms-MY" | "th" | "th-TH" | "tr" | "tr-CY" | "tr-TR" | "vi" | "vi-VN" | "zh"
        | "zh-CN" | "zh-HK" | "zh-SG" | "zh-TW" => Some(PluralRule::Invariant),
        "af" | "af-ZA" | "bn" | "bn-BD" | "bn-IN" | "bg" | "bg-BG" | "ca" | "ca-AD" | "ca-ES"
        | "ca-FR" | "ca-IT" | "da" | "da-DK" | "de" | "de-AT" | "de-BE" | "de-CH" | "de-DE"
        | "de-LI" | "de-LU" | "el" | "el-CY" | "el-GR" | "en" | "en-AG" | "en-AU" | "en-BW"
        | "en-CA" | "en-DK" | "en-GB" | "en-HK" | "en-IE" | "en-IN" | "en-NG" | "en-NZ"
        | "en-PH" | "en-SG" | "en-US" | "en-ZA" | "en-ZM" | "en-ZW" | "eo" | "eo-US" | "es"
        | "es-AR" | "es-BO" | "es-CL" | "es-CO" | "es-CR" | "es-CU" | "es-DO" | "es-EC"
        | "es-ES" | "es-GT" | "es-HN" | "es-MX" | "es-NI" | "es-PA" | "es-PE" | "es-PR"
        | "es-PY" | "es-SV" | "es-US" | "es-UY" | "es-VE" | "et" | "et-EE" | "eu" | "eu-ES"
        | "eu-FR" | "fa" | "fa-IR" | "fi" | "fi-FI" | "fo" | "fo-FO" | "fur" | "fur-IT" | "fy"
        | "fy-DE" | "fy-NL" | "gl" | "gl-ES" | "gu" | "gu-IN" | "ha" | "ha-NG" | "he" | "he-IL"
        | "hu" | "hu-HU" | "is" | "is-IS" | "it" | "it-CH" | "it-IT" | "ku" | "ku-TR" | "lb"
        | "lb-LU" | "ml" | "ml-IN" | "mn" | "mn-MN" | "mr" | "mr-IN" | "nah" | "nb" | "nb-NO"
        | "ne" | "ne-NP" | "nl" | "nl-AW" | "nl-BE" | "nl-NL" | "nn" | "nn-NO" | "no" | "om"
        | "om-ET" | "om-KE" | "or" | "or-IN" | "pa" | "pa-IN" | "pa-PK" | "pap" | "pap-AN"
        | "pap-AW" | "pap-CW" | "ps" | "ps-AF" | "pt" | "pt-BR" | "pt-PT" | "so" | "so-DJ"
        | "so-ET" | "so-KE" | "so-SO" | "sq" | "sq-AL" | "sq-MK" | "sv" | "sv-FI" | "sv-SE"
        | "sw" | "sw-KE" | "sw-TZ" | "ta" | "ta-IN" | "ta-LK" | "te" | "te-IN" | "tk" | "tk-TM"
        | "ur" | "ur-IN" | "ur-PK" | "zu" | "zu-ZA" => Some(PluralRule::OneOther),
        "am" | "am-ET" | "bh" | "fil" | "fil-PH" | "fr" | "fr-BE" | "fr-CA" | "fr-CH" | "fr-FR"
        | "fr-LU" | "gun" | "hi" | "hi-IN" | "hy" | "hy-AM" | "ln" | "ln-CD" | "mg" | "mg-MG"
        | "nso" | "nso-ZA" | "ti" | "ti-ER" | "ti-ET" | "wa" | "wa-BE" | "xbr" => {
            Some(PluralRule::ZeroOrOne)
        }
        "be" | "be-BY" | "bs" | "bs-BA" | "hr" | "hr-HR" | "ru" | "ru-RU" | "ru-UA" | "sr"
        | "sr-ME" | "sr-RS" | "uk" | "uk-UA" => Some(PluralRule::Modulo(ModuloRule::EastSlavic)),
        "cs" | "cs-CZ" | "sk" | "sk-SK" => Some(PluralRule::CzechSlovak),
        "ga" | "ga-IE" => Some(PluralRule::Irish),
        "lt" | "lt-LT" => Some(PluralRule::Modulo(ModuloRule::Lithuanian)),
        "sl" | "sl-SI" => Some(PluralRule::Modulo(ModuloRule::Slovenian)),
        "mk" | "mk-MK" => Some(PluralRule::Modulo(ModuloRule::Macedonian)),
        "mt" | "mt-MT" => Some(PluralRule::Modulo(ModuloRule::Maltese)),
        "lv" | "lv-LV" => Some(PluralRule::Modulo(ModuloRule::Latvian)),
        "pl" | "pl-PL" => Some(PluralRule::Modulo(ModuloRule::Polish)),
        "cy" | "cy-GB" => Some(PluralRule::Welsh),
        "ro" | "ro-RO" => Some(PluralRule::Modulo(ModuloRule::Romanian)),
        "ar" | "ar-AE" | "ar-BH" | "ar-DZ" | "ar-EG" | "ar-IN" | "ar-IQ" | "ar-JO" | "ar-KW"
        | "ar-LB" | "ar-LY" | "ar-MA" | "ar-OM" | "ar-QA" | "ar-SA" | "ar-SD" | "ar-SS"
        | "ar-SY" | "ar-TN" | "ar-YE" => Some(PluralRule::Modulo(ModuloRule::Arabic)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/translation/choice_selector.rs"]
mod tests;
