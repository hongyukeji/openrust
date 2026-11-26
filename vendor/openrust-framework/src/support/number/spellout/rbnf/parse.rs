//! Rule text → compiled rules. Follows ICU4J `NFRule.makeRules`,
//! `parseRuleDescriptor`, `extractSubstitution` and `NFSubstitution.
//! makeSubstitution`, but rejects instead of tolerating: an unmatched
//! bracket, a stray `<`, a third substitution or a plural form ICU would
//! silently keep as text is an error here.

use std::collections::HashMap;

use icu_plurals::{PluralCategory, PluralRuleType, PluralRules};

use super::{
    DecimalPattern, ParseError, Piece, PluralFormat, Rbnf, Rule, RuleSet, SubKind, Substitution,
    Symbols, Target,
};

/// ICU treats `%%lenient-parse` as collation rules for parsing, not as a
/// ruleset; it never formats anything.
const LENIENT_PARSE: &str = "%%lenient-parse";

impl Rbnf {
    /// `rulesets` are `(name, [(descriptor, body)])` in file order. The
    /// caller selects the ICU locale's default ruleset.
    pub(crate) fn compile<'a>(
        language: &str,
        symbols: Symbols,
        rulesets: impl Iterator<Item = (&'a str, &'a [(&'a str, &'a str)])>,
    ) -> Result<Rbnf, ParseError> {
        let rulesets: Vec<_> = rulesets
            .filter(|(name, _)| *name != LENIENT_PARSE)
            .collect();
        let mut names = HashMap::with_capacity(rulesets.len());
        for (index, (name, _)) in rulesets.iter().enumerate() {
            if !name.starts_with('%') {
                return Err(error(name, "", "ruleset name must start with %"));
            }
            if names.insert(name.to_string(), index).is_some() {
                return Err(error(name, "", "duplicate ruleset"));
            }
        }
        let rule_names = &names;
        let denominator_sets: std::collections::HashSet<_> = rulesets
            .iter()
            .enumerate()
            .flat_map(|(index, (_, rules))| {
                rules.iter().map(move |(key, text)| (index, *key, *text))
            })
            .filter(|(_, key, _)| matches!(*key, "x.x" | "0.x" | "x.0" | "x,x" | "0,x" | "x,0"))
            .flat_map(|(index, _, text)| {
                text.split(['>', '→'])
                    .skip(1)
                    .step_by(2)
                    .filter_map(move |name| rule_names.get(name).copied())
                    .filter(move |&target| target != index)
            })
            .collect();
        let mut plurals = Plurals::default();
        let mut compiled = Vec::with_capacity(rulesets.len());
        for (index, (name, rules)) in rulesets.iter().enumerate() {
            let mut ctx = Ctx {
                names: &names,
                this: index,
                name,
                decimal: &symbols.decimal,
                denominators: denominator_sets.contains(&index),
                plurals: &mut plurals,
            };
            compiled.push(ctx.ruleset(rules)?);
        }

        let plural_rules = |wanted: bool, kind: PluralRuleType| -> Result<_, ParseError> {
            if !wanted {
                return Ok(None);
            }
            let locale = icu_locale_core::Locale::try_from_str(language)
                .map_err(|_| error("", "", &format!("invalid language tag {language:?}")))?;
            PluralRules::try_new(locale.into(), kind.into())
                .map(Some)
                .map_err(|e| error("", "", &format!("no plural rules for {language}: {e}")))
        };
        Ok(Rbnf {
            cardinal: plural_rules(plurals.cardinal, PluralRuleType::Cardinal)?,
            ordinal: plural_rules(plurals.ordinal, PluralRuleType::Ordinal)?,
            rulesets: compiled,
            symbols,
        })
    }
}

#[derive(Default)]
struct Plurals {
    cardinal: bool,
    ordinal: bool,
}

struct Ctx<'a> {
    names: &'a HashMap<String, usize>,
    this: usize,
    name: &'a str,
    decimal: &'a str,
    denominators: bool,
    plurals: &'a mut Plurals,
}

/// What a rule descriptor says the rule is for (ICU's special base values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal {
        base: u128,
        divisor: u128,
    },
    Negative,
    /// `x.x`, with its decimal point character.
    Improper(char),
    /// `0.x`
    Proper(char),
    /// `x.0`, ICU's "default rule".
    Master(char),
    Infinity,
    NaN,
}

impl Kind {
    fn is_fraction(self) -> bool {
        matches!(self, Kind::Improper(_) | Kind::Proper(_) | Kind::Master(_))
    }

    fn divisor(self) -> u128 {
        match self {
            Kind::Normal { divisor, .. } => divisor,
            _ => 1,
        }
    }
}

impl Ctx<'_> {
    fn err(&self, rule: &str, message: &str) -> ParseError {
        error(self.name, rule, message)
    }

    fn ruleset(&mut self, rules: &[(&str, &str)]) -> Result<RuleSet, ParseError> {
        let mut set = RuleSet {
            name: self.name.to_string(),
            denominators: self.denominators,
            rules: Vec::with_capacity(rules.len()),
            negative: None,
            infinity: None,
            nan: None,
            improper: None,
            proper: None,
            master: None,
        };
        for (descriptor, body) in rules {
            let predecessor = set.rules.len().checked_sub(1);
            self.rule(descriptor, body, predecessor, &mut set)?;
        }
        Ok(set)
    }

    /// One `(descriptor, body)` entry, which may expand to two rules.
    fn rule(
        &mut self,
        descriptor: &str,
        body: &str,
        predecessor: Option<usize>,
        set: &mut RuleSet,
    ) -> Result<(), ParseError> {
        let source = format!("{descriptor}: {body}");
        let kind = self.descriptor(descriptor, &source)?;

        let body = body.strip_suffix(';').unwrap_or(body);
        let mut text = body.replace('←', "<").replace('→', ">");
        if text.contains(';') {
            return Err(self.err(&source, "only one rule per body"));
        }
        if let Some(rest) = text.strip_prefix('\'') {
            text = rest.to_string();
        }

        // Optional text in brackets is shorthand for two rules: without
        // the bracketed text at the base value, with it one above.
        let (open, close) = (text.find('['), text.find(']'));
        let (with, without) = match (open, close) {
            (None, None) => (text, None),
            (Some(open), Some(close)) if open < close => {
                let inner = &text[open + 1..close];
                let after = &text[close + 1..];
                let (first, second) = match inner.split_once('|') {
                    Some((a, b)) => (a, b),
                    None => (inner, ""),
                };
                let with = format!("{}{first}{after}", &text[..open]);
                let without = format!("{}{second}{after}", &text[..open]);
                if with.contains(['[', ']']) || without.contains(['[', ']']) {
                    return Err(self.err(&source, "nested or repeated brackets"));
                }
                (with, Some(without))
            }
            _ => return Err(self.err(&source, "unmatched bracket")),
        };

        match kind {
            Kind::Normal { base, divisor } => {
                let mut base = base;
                if let (true, Some(without)) = (base > 0 && base % divisor == 0, without) {
                    let rule = self.build(kind, base, &without, predecessor, &source)?;
                    push_normal(set, rule, &source)?;
                    base += u128::from(!self.denominators);
                }
                let rule = self.build(kind, base, &with, predecessor, &source)?;
                push_normal(set, rule, &source)?;
            }
            Kind::Improper(point) => {
                if let Some(without) = without {
                    let rule = self.build(Kind::Proper(point), 0, &without, None, &source)?;
                    set_fraction(&mut set.proper, rule, point, self.decimal);
                }
                let rule = self.build(kind, 0, &with, None, &source)?;
                set_fraction(&mut set.improper, rule, point, self.decimal);
            }
            Kind::Master(point) => {
                if let Some(without) = without {
                    let rule = self.build(kind, 0, &without, None, &source)?;
                    set_fraction(&mut set.master, rule, point, self.decimal);
                    let rule = self.build(Kind::Improper(point), 0, &with, None, &source)?;
                    set_fraction(&mut set.improper, rule, point, self.decimal);
                } else {
                    let rule = self.build(kind, 0, &with, None, &source)?;
                    set_fraction(&mut set.master, rule, point, self.decimal);
                }
            }
            Kind::Proper(point) => {
                if without.is_some() {
                    return Err(self.err(&source, "brackets are not allowed in a 0.x rule"));
                }
                let rule = self.build(kind, 0, &with, None, &source)?;
                set_fraction(&mut set.proper, rule, point, self.decimal);
            }
            Kind::Negative => {
                if without.is_some() {
                    return Err(self.err(&source, "brackets are not allowed in the -x rule"));
                }
                set.negative = Some(self.build(kind, 0, &with, None, &source)?);
            }
            Kind::Infinity | Kind::NaN => {
                if without.is_some() {
                    return Err(self.err(&source, "brackets are not allowed in Inf/NaN rules"));
                }
                let rule = self.build(kind, 0, &with, None, &source)?;
                if kind == Kind::Infinity {
                    set.infinity = Some(rule);
                } else {
                    set.nan = Some(rule);
                }
            }
        }
        Ok(())
    }

    /// `NFRule.parseRuleDescriptor`. `bv`, `bv/rad`, `bv>`, `bv/rad>>`,
    /// with `,`, `.` and spaces ignored inside numbers.
    fn descriptor(&self, descriptor: &str, source: &str) -> Result<Kind, ParseError> {
        let bytes = descriptor.as_bytes();
        let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
            return Err(self.err(source, "empty rule descriptor"));
        };
        if first.is_ascii_digit() && *last != b'x' {
            let mut chars = descriptor.chars().peekable();
            let base = self.number(&mut chars, source)?;
            let mut radix = 10u128;
            if chars.peek() == Some(&'/') {
                chars.next();
                radix = self.number(&mut chars, source)?;
                if radix < 2 {
                    return Err(self.err(source, "radix must be at least 2"));
                }
            }
            let mut exponent = expected_exponent(radix, base);
            for c in chars {
                if c == '>' && exponent > 0 {
                    exponent -= 1;
                } else {
                    return Err(self.err(source, &format!("unexpected {c:?} in rule descriptor")));
                }
            }
            let divisor = radix
                .checked_pow(exponent)
                .ok_or_else(|| self.err(source, "divisor overflows u128"))?;
            return Ok(Kind::Normal { base, divisor });
        }
        let point = descriptor.chars().nth(1).unwrap_or('.');
        Ok(match descriptor {
            "-x" => Kind::Negative,
            "Inf" => Kind::Infinity,
            "NaN" => Kind::NaN,
            _ if descriptor.chars().count() == 3 && !point.is_ascii_alphanumeric() => {
                match (first, last) {
                    (b'0', b'x') => Kind::Proper(point),
                    (b'x', b'x') => Kind::Improper(point),
                    (b'x', b'0') => Kind::Master(point),
                    _ => return Err(self.err(source, "unknown rule descriptor")),
                }
            }
            _ => return Err(self.err(source, "unknown rule descriptor")),
        })
    }

    /// Digits with `,`, `.` and whitespace ignored, stopping at `/` or `>`.
    fn number(
        &self,
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        source: &str,
    ) -> Result<u128, ParseError> {
        let mut value = 0u128;
        while let Some(&c) = chars.peek() {
            match c {
                '/' | '>' => break,
                '0'..='9' => {
                    value = value
                        .checked_mul(10)
                        .and_then(|v| v.checked_add(c as u128 - '0' as u128))
                        .ok_or_else(|| self.err(source, "base value overflows u128"))?;
                }
                ',' | '.' => {}
                c if c.is_whitespace() => {}
                _ => return Err(self.err(source, &format!("unexpected {c:?} in rule descriptor"))),
            }
            chars.next();
        }
        Ok(value)
    }

    /// Rule text (arrows already ASCII, brackets resolved) → pieces.
    fn build(
        &mut self,
        kind: Kind,
        base: u128,
        text: &str,
        predecessor: Option<usize>,
        source: &str,
    ) -> Result<Rule, ParseError> {
        let mut pieces = Vec::new();
        let mut subs = 0;
        let mut plurals = 0;
        let mut has_modulus = false;
        let mut literal = String::new();
        let mut rest = text;
        while !rest.is_empty() {
            let Some(at) = rest.find(['<', '>', '=', '$']) else {
                literal.push_str(rest);
                break;
            };
            literal.push_str(&rest[..at]);
            rest = &rest[at..];
            if rest.starts_with('$') {
                if !rest.starts_with("$(") {
                    return Err(self.err(source, "stray $ in rule text"));
                }
                let end = rest
                    .find(")$")
                    .ok_or_else(|| self.err(source, "unterminated $( plural block"))?;
                plurals += 1;
                if plurals > 1 {
                    return Err(self.err(source, "more than one plural block"));
                }
                flush(&mut pieces, &mut literal);
                pieces.push(Piece::Plural(self.plural(&rest[2..end], source)?));
                rest = &rest[end + 2..];
                continue;
            }
            let c = rest.as_bytes()[0] as char;
            let token_len = if rest.starts_with(">>>") {
                3
            } else {
                rest[1..]
                    .find(c)
                    .map(|end| {
                        let len = end + 2;
                        len + usize::from(
                            self.denominators
                                && c == '<'
                                && rest.as_bytes().get(len) == Some(&b'<'),
                        )
                    })
                    .ok_or_else(|| self.err(source, &format!("unmatched {c:?} in rule text")))?
            };
            let token = &rest[..token_len];
            subs += 1;
            if subs > 2 {
                return Err(self.err(source, "more than two substitutions"));
            }
            let sub = self.substitution(token, kind, predecessor, source)?;
            has_modulus |= matches!(sub.kind, SubKind::Modulus | SubKind::Predecessor(_));
            flush(&mut pieces, &mut literal);
            pieces.push(Piece::Sub(sub));
            rest = &rest[token_len..];
        }
        flush(&mut pieces, &mut literal);
        Ok(Rule {
            base,
            divisor: kind.divisor(),
            pieces,
            has_modulus,
        })
    }

    /// `NFSubstitution.makeSubstitution`: the token character and the kind
    /// of rule it sits in decide what the substitution computes; the text
    /// between the token characters decides who formats the result.
    fn substitution(
        &self,
        token: &str,
        kind: Kind,
        predecessor: Option<usize>,
        source: &str,
    ) -> Result<Substitution, ParseError> {
        let c = token.as_bytes()[0];
        let pad = self.denominators && c == b'<' && token.ends_with("<<") && token.len() > 2;
        let inner = if token == ">>>" {
            ""
        } else {
            &token[1..token.len() - 1 - usize::from(pad)]
        };
        let unsupported = |what: &str| self.err(source, &format!("{token:?}: {what}"));

        let target = match inner.as_bytes().first() {
            None => Target::RuleSet(self.this),
            Some(b'%') => {
                let index = *self
                    .names
                    .get(inner)
                    .ok_or_else(|| unsupported(&format!("no ruleset named {inner}")))?;
                Target::RuleSet(index)
            }
            Some(b'#' | b'0') => {
                if c != b'=' && !(self.denominators && c == b'<') {
                    return Err(unsupported(
                        "decimal-format patterns are only supported in =…=",
                    ));
                }
                Target::Decimal(DecimalPattern::parse(inner).map_err(unsupported)?)
            }
            Some(_) => return Err(unsupported("unknown substitution")),
        };

        let sub_kind = match (c, kind) {
            (b'<', Kind::Normal { .. }) if self.denominators => SubKind::Numerator { pad },
            (b'<', Kind::Normal { .. }) => SubKind::Multiplier,
            (b'<', k) if k.is_fraction() => SubKind::IntegralPart,
            (b'<', _) => return Err(unsupported("<< is not allowed in this rule")),
            (b'>', Kind::Negative) => SubKind::AbsoluteValue,
            (b'>', k) if k.is_fraction() => {
                if !matches!(target, Target::RuleSet(i) if i == self.this) {
                    return Ok(Substitution {
                        kind: SubKind::Denominator,
                        target,
                    });
                }
                SubKind::FractionalPart {
                    spaces: token != ">>>",
                }
            }
            (b'>', Kind::Normal { .. }) if token == ">>>" => {
                let index = predecessor.ok_or_else(|| unsupported("no preceding rule"))?;
                SubKind::Predecessor(index)
            }
            (b'>', Kind::Normal { .. }) => SubKind::Modulus,
            (b'>', _) => return Err(unsupported(">> is not allowed in this rule")),
            (b'=', _) if inner.is_empty() => return Err(unsupported("== is not a legal token")),
            (b'=', _) => SubKind::SameValue,
            _ => unreachable!("tokenizer only yields <, > and ="),
        };
        Ok(Substitution {
            kind: sub_kind,
            target,
        })
    }

    /// `cardinal,one{st}two{nd}few{rd}other{th}` (the text between `$(` and `)$`).
    fn plural(&mut self, text: &str, source: &str) -> Result<PluralFormat, ParseError> {
        let (kind, mut rest) = text
            .split_once(',')
            .ok_or_else(|| self.err(source, "plural block without a type"))?;
        let ordinal = match kind {
            "cardinal" => false,
            "ordinal" => true,
            _ => return Err(self.err(source, &format!("unknown plural type {kind:?}"))),
        };
        let mut forms = Vec::new();
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let (name, after) = rest
                .split_once('{')
                .ok_or_else(|| self.err(source, "plural form without {…}"))?;
            let (form, after) = after
                .split_once('}')
                .ok_or_else(|| self.err(source, "unterminated plural form"))?;
            let category = match name.trim() {
                "zero" => PluralCategory::Zero,
                "one" => PluralCategory::One,
                "two" => PluralCategory::Two,
                "few" => PluralCategory::Few,
                "many" => PluralCategory::Many,
                "other" => PluralCategory::Other,
                other => {
                    return Err(self.err(source, &format!("unsupported plural form {other:?}")));
                }
            };
            forms.push((category, form.to_string()));
            rest = after;
        }
        if !forms.iter().any(|(c, _)| *c == PluralCategory::Other) {
            return Err(self.err(source, "plural block without an other{…} form"));
        }
        if ordinal {
            self.plurals.ordinal = true;
        } else {
            self.plurals.cardinal = true;
        }
        Ok(PluralFormat { ordinal, forms })
    }
}

fn flush(pieces: &mut Vec<Piece>, literal: &mut String) {
    if !literal.is_empty() {
        pieces.push(Piece::Text(std::mem::take(literal)));
    }
}

fn push_normal(set: &mut RuleSet, rule: Rule, source: &str) -> Result<(), ParseError> {
    if set.rules.last().is_some_and(|last| rule.base < last.base) {
        return Err(error(&set.name, source, "rules are not in order"));
    }
    set.rules.push(rule);
    Ok(())
}

/// `NFRuleSet.setBestFractionRule`: among several `x.x`-style rules the one
/// whose decimal point is the locale's wins; otherwise the first stays.
fn set_fraction(slot: &mut Option<Rule>, rule: Rule, point: char, decimal: &str) {
    if slot.is_none() || decimal.chars().eq(std::iter::once(point)) {
        *slot = Some(rule);
    }
}

/// Largest `e` with `radix^e <= base`; 0 for base 0 (ICU `expectedExponent`).
fn expected_exponent(radix: u128, base: u128) -> u32 {
    if radix < 2 || base < 1 {
        return 0;
    }
    let mut exponent = 0;
    let mut power = radix;
    while power <= base {
        exponent += 1;
        match power.checked_mul(radix) {
            Some(next) => power = next,
            None => break,
        }
    }
    exponent
}

fn error(ruleset: &str, rule: &str, message: &str) -> ParseError {
    ParseError {
        ruleset: ruleset.to_string(),
        rule: rule.to_string(),
        message: message.to_string(),
    }
}
