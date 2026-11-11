//! 字符串分支语法和地区复数规则。
//!
//! 复数规则来自 Zend Framework（2010-09-25），采用 New BSD License。
//! Copyright (c) 2005–2010 Zend Technologies USA Inc.

use std::{borrow::Cow, cmp::Ordering};

use super::{TranslationError, TranslationResult};

/// Selects the appropriate plural form for a locale and count.
pub struct MessageSelector;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PluralCount {
    Integer(i64),
    Float(f64),
}

impl From<i64> for PluralCount {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for PluralCount {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl std::fmt::Display for PluralCount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer(value) => value.fmt(formatter),
            Self::Float(value) => formatter.write_str(&double_string(*value)),
        }
    }
}

impl PluralCount {
    #[allow(
        clippy::cast_precision_loss,
        reason = "Laravel converts mixed integer and double operands to double"
    )]
    fn as_float(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    fn numeric_string(value: &str) -> Option<Self> {
        value
            .parse::<i64>()
            .map(Self::Integer)
            .ok()
            .or_else(|| value.parse::<f64>().map(Self::Float).ok())
    }
}

impl MessageSelector {
    pub fn choose<'a>(
        &self,
        line: &'a str,
        count: impl Into<PluralCount>,
        locale: &str,
    ) -> TranslationResult<&'a str> {
        let count = count.into();
        if let PluralCount::Float(number) = count
            && !number.is_finite()
        {
            return Err(TranslationError::InvalidCount);
        }
        let locale = if locale.contains('_') {
            Cow::Owned(locale.replace('_', "-"))
        } else {
            Cow::Borrowed(locale)
        };
        select(line, count, &locale)
    }
}

pub(super) fn select<'a>(
    line: &'a str,
    count: PluralCount,
    locale: &str,
) -> TranslationResult<&'a str> {
    let segments = line.split('|');
    let matched = segments
        .clone()
        .filter_map(condition_prefix)
        .find_map(|(condition, body)| matches_condition(count, condition).then_some(body));
    if let Some(body) = matched {
        return Ok(body.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']));
    }

    let index = plural_rule(locale).map_or(Ok(0), |rule| rule.index(count))?;
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

fn matches_condition(count: PluralCount, condition: &str) -> bool {
    if let Some((from, to)) = condition.split_once(',') {
        let starts_at_or_below = from == "*" || compare_bound(count, from).is_ge();
        let ends_at_or_above = to == "*" || compare_bound(count, to).is_le();
        if starts_at_or_below && ends_at_or_above {
            return true;
        }
    }
    compare_bound(count, condition).is_eq()
}

fn compare_bound(count: PluralCount, bound: &str) -> Ordering {
    if let Some(bound) = PluralCount::numeric_string(bound) {
        return compare_numbers(count, bound);
    }
    // 匹配前缀语法但非数值的边界走 PHP number/string 的字节比较。
    match count {
        PluralCount::Integer(count) => count.to_string().as_str().cmp(bound),
        PluralCount::Float(count) => double_string(count).as_str().cmp(bound),
    }
}

fn compare_numbers(left: PluralCount, right: PluralCount) -> Ordering {
    match (left, right) {
        (PluralCount::Integer(left), PluralCount::Integer(right)) => left.cmp(&right),
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

fn is_count(count: PluralCount, expected: i64) -> bool {
    compare_numbers(count, PluralCount::Integer(expected)).is_eq()
}

impl PluralRule {
    fn index(self, count: PluralCount) -> TranslationResult<usize> {
        let count = match count {
            PluralCount::Integer(value) => value
                .checked_abs()
                .map_or(PluralCount::Float(TWO_TO_63), PluralCount::Integer),
            PluralCount::Float(value) => PluralCount::Float(value.abs()),
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
    fn index(self, count: PluralCount) -> TranslationResult<usize> {
        let is = |number| is_count(count, number);
        match self {
            Self::Maltese | Self::Polish | Self::Romanian if is(1) => return Ok(0),
            Self::Latvian | Self::Arabic if is(0) => return Ok(0),
            Self::Maltese | Self::Romanian if is(0) => return Ok(1),
            Self::Arabic if is(1) => return Ok(1),
            Self::Arabic if is(2) => return Ok(2),
            _ => {}
        }

        // PHP 8.5 在显式整数转换超界时发出 E_WARNING，固定上游的默认异常处理将其升级。
        let integer = match count {
            PluralCount::Integer(value) => value,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "Laravel truncates finite fractional operands before modulo"
            )]
            PluralCount::Float(value) if value < TWO_TO_63 => value as i64,
            PluralCount::Float(_) => return Err(TranslationError::InvalidCount),
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

// 固定上游消息选择器的完整地区表。Locale 使用 BCP 47
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

fn double_string(number: f64) -> String {
    if number.is_nan() {
        return "NAN".to_owned();
    }
    if number.is_infinite() {
        return if number.is_sign_negative() {
            "-INF"
        } else {
            "INF"
        }
        .to_owned();
    }
    let scientific = format!("{number:.13e}");
    let (coefficient, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    let (sign, coefficient) = coefficient
        .strip_prefix('-')
        .map_or(("", coefficient), |coefficient| ("-", coefficient));
    let digits = coefficient.replace('.', "");
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    if !(-4..14).contains(&exponent) {
        let fraction = if digits.len() > 1 { &digits[1..] } else { "0" };
        return format!("{sign}{}.{fraction}E{exponent:+}", &digits[..1]);
    }
    let decimal = exponent + 1;
    if decimal <= 0 {
        return format!(
            "{sign}0.{}{digits}",
            "0".repeat(usize::try_from(-decimal).unwrap_or(0))
        );
    }
    let decimal = usize::try_from(decimal).unwrap_or(0);
    if decimal >= digits.len() {
        format!("{sign}{digits}{}", "0".repeat(decimal - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..decimal], &digits[decimal..])
    }
}
