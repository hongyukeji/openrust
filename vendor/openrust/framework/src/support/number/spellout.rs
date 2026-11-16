//! Laravel spellout rules from ICU 76.1 / CLDR 46. See spellout/README.md.

mod rbnf;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use icu_locale::{LocaleCanonicalizer, LocaleExpander};
use icu_locale_core::Locale;

use self::rbnf::{Rbnf, Symbols, Value};
use super::{NumberError, NumberValue};

type LocaleRules = (&'static str, Symbols);
type RuleGroups = (
    &'static str,
    Vec<(&'static str, Vec<(&'static str, &'static str)>)>,
);

static LOCALES: LazyLock<HashMap<&str, LocaleRules>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("spellout/locales.json")).expect("bundled ICU locales")
});
static RULES: LazyLock<HashMap<&str, RuleGroups>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("spellout/rules.json")).expect("bundled ICU spellout rules")
});

thread_local! {
    static COMPILED: RefCell<HashMap<&'static str, Rbnf>> = RefCell::default();
}

pub(super) fn format(
    locale: &Locale,
    number: NumberValue,
    ordinal: bool,
) -> Result<String, NumberError> {
    let decimal;
    let value = match number {
        NumberValue::Float(number) if !number.is_finite() => Value::non_finite(number),
        _ => {
            decimal = number.decimal()?.to_string();
            let negative = decimal.starts_with('-');
            let unsigned = decimal.strip_prefix('-').unwrap_or(&decimal);
            let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
            Value::from_digits(negative, integer, fraction)
        }
    };
    let (name, (rules, symbols)) = locale_rules(locale).ok_or(NumberError::InvalidLocale)?;
    COMPILED.with_borrow_mut(|cache| {
        let (default, groups) = &RULES[*rules];
        let rules = match cache.entry(name) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(
                Rbnf::compile(
                    name,
                    symbols.clone(),
                    groups.iter().map(|(name, rules)| (*name, rules.as_slice())),
                )
                .map_err(|_| NumberError::UnsupportedSpellout)?,
            ),
        };
        rules
            .format(default, ordinal, value)
            .ok_or(NumberError::UnsupportedSpellout)
    })
}

fn locale_rules(locale: &Locale) -> Option<(&'static str, &'static LocaleRules)> {
    if let Some((&key, rules)) = LOCALES.get_key_value(locale.id.to_string().as_str()) {
        return Some((key, rules));
    }
    let mut locale = locale.clone();
    LocaleCanonicalizer::new_extended().canonicalize(&mut locale);
    let mut id = locale.id;
    id.variants = Default::default();

    if let Some((&key, rules)) = LOCALES.get_key_value(id.to_string().as_str()) {
        return Some((key, rules));
    }
    let mut likely = id.clone();
    LocaleExpander::new_extended().maximize(&mut likely);
    if let Some((&key, rules)) = LOCALES.get_key_value(likely.to_string().as_str()) {
        return Some((key, rules));
    }
    id.region = None;
    LOCALES
        .get_key_value(id.to_string().as_str())
        .or_else(|| LOCALES.get_key_value(id.language.as_str()))
        .map(|(&key, rules)| (key, rules))
}
