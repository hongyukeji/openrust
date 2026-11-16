//! CLDR/ICU rule-based number format (RBNF), formatting direction only.
//!
//! A port of the structure of ICU4J's `RuleBasedNumberFormat`, `NFRuleSet`,
//! `NFRule` and `NFSubstitution`: the same rule
//! selection (largest base value ≤ n, radix/exponent divisors, the rollback
//! rule), the same bracket splitting into two rules, the same substitution
//! semantics. Values retain decimal digits and use binary floating point
//! only for ICU's denominator selection. Formatting retains punctuation,
//! spacing and soft hyphens from the selected rules.
//!
//! [`parse`] builds the compiled representation, [`format`] evaluates it.

mod decimal;
mod format;
mod parse;

use std::fmt;

use icu_plurals::{PluralCategory, PluralRules};

use decimal::DecimalPattern;

/// A number to spell: sign, integer magnitude and the decimal digits after
/// the point. `fraction` is spoken verbatim, trailing zeros included
/// ("3,10" → "drei Komma eins null"), so the caller decides what a written
/// fraction means; ICU, holding a `double`, would drop them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Value<'a> {
    pub negative: bool,
    pub integer: u128,
    large_integer: Option<&'a str>,
    special: Option<Special>,
    /// ASCII digits, possibly empty. Anything else makes `format` return `None`.
    pub fraction: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Special {
    Infinity,
    Nan,
}

impl Value<'static> {
    pub(crate) fn non_finite(number: f64) -> Self {
        let special = if number.is_nan() {
            Special::Nan
        } else {
            Special::Infinity
        };
        Self {
            negative: number.is_infinite() && number.is_sign_negative(),
            special: Some(special),
            ..Self::int(0)
        }
    }
    pub(crate) fn int(n: i128) -> Self {
        Value {
            negative: n < 0,
            integer: n.unsigned_abs(),
            large_integer: None,
            special: None,
            fraction: "",
        }
    }
}

impl<'a> Value<'a> {
    /// Zero is never negative.
    pub(crate) fn decimal(negative: bool, integer: u128, fraction: &'a str) -> Self {
        let negative = negative && (integer != 0 || !fraction.is_empty());
        Value {
            negative,
            integer,
            large_integer: None,
            special: None,
            fraction,
        }
    }

    pub(crate) fn from_digits(negative: bool, digits: &'a str, fraction: &'a str) -> Self {
        match digits.parse() {
            Ok(integer) => Self::decimal(negative, integer, fraction),
            Err(_) => Self {
                negative,
                integer: u128::MAX,
                large_integer: Some(digits),
                special: None,
                fraction,
            },
        }
    }

    fn is_fraction(self) -> bool {
        !self.fraction.is_empty()
    }
}

/// The locale's number symbols the interpreter needs: the decimal separator
/// selects between `x.x` and `x,x` rules, the group separator and minus
/// sign render `=#,##0=` fallbacks.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct Symbols {
    decimal: String,
    group: String,
    minus: String,
    zero: char,
    infinity: String,
    nan: String,
}

/// Compiled rulesets of one locale. Built once per language and shared.
pub(crate) struct Rbnf {
    rulesets: Vec<RuleSet>,
    symbols: Symbols,
    /// Plural rules, built only when a rule uses `$(cardinal,…)$` / `$(ordinal,…)$`.
    cardinal: Option<PluralRules>,
    ordinal: Option<PluralRules>,
}

impl Rbnf {
    pub(crate) fn format(&self, default: &str, ordinal: bool, n: Value<'_>) -> Option<String> {
        let index_of = |name: &str| self.rulesets.iter().position(|rules| rules.name == name);
        let index = if ordinal {
            index_of("%spellout-ordinal")
        } else {
            None
        }
        .or_else(|| index_of(default))?;
        let mut out = String::new();
        self.format_value(index, n, &mut out, 0).ok()?;
        Some(out)
    }
}

struct RuleSet {
    name: String,
    denominators: bool,
    /// Normal rules in non-decreasing base-value order.
    rules: Vec<Rule>,
    /// `-x`
    negative: Option<Rule>,
    infinity: Option<Rule>,
    nan: Option<Rule>,
    /// `x.x`
    improper: Option<Rule>,
    /// `0.x`
    proper: Option<Rule>,
    /// `x.0`
    master: Option<Rule>,
}

struct Rule {
    base: u128,
    /// `radix^exponent`; 1 for the special rules.
    divisor: u128,
    /// Rule text and substitutions in source order; ICU stores the text
    /// with the substitution positions, which is the same thing.
    pieces: Vec<Piece>,
    has_modulus: bool,
}

enum Piece {
    Text(String),
    Sub(Substitution),
    Plural(PluralFormat),
}

struct Substitution {
    kind: SubKind,
    target: Target,
}

/// ICU's `NFSubstitution` subclasses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubKind {
    /// `<<` in a normal rule: `n / divisor`.
    Multiplier,
    /// `>>` in a normal rule: `n % divisor`.
    Modulus,
    /// `>>>`: `n % divisor` formatted directly by the previous rule.
    Predecessor(usize),
    /// `==`-family: the same value, elsewhere.
    SameValue,
    /// `<<` in a fraction rule: the integer part.
    IntegralPart,
    /// `>>` in a fraction rule: the fraction, one digit at a time.
    FractionalPart {
        spaces: bool,
    },
    Denominator,
    Numerator {
        pad: bool,
    },
    /// `>>` in the negative rule: the magnitude.
    AbsoluteValue,
}

enum Target {
    RuleSet(usize),
    Decimal(DecimalPattern),
}

/// `$(cardinal,one{st}other{th})$`
struct PluralFormat {
    ordinal: bool,
    forms: Vec<(PluralCategory, String)>,
}

/// Rule syntax outside the interpreter's scope, or an inconsistent ruleset.
/// Carries the ruleset name and the offending rule text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub ruleset: String,
    pub rule: String,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rbnf: {}: in {} rule {:?}",
            self.message, self.ruleset, self.rule
        )
    }
}

impl std::error::Error for ParseError {}
