//! The `=#,##0=` family: ICU falls back to a `DecimalFormat` pattern for
//! values a ruleset does not spell (10¹⁸ and above, fractional ordinals).
//! Only the pattern subset CLDR spellout rules use is accepted: optional
//! grouping, `0`/`#` digit placeholders, an optional fraction part.

use std::cmp::Ordering;

use super::{Special, Symbols, Value};

impl Symbols {
    pub(super) fn special(&self, special: Special) -> &str {
        match special {
            Special::Infinity => &self.infinity,
            Special::Nan => &self.nan,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DecimalPattern {
    /// Digits per group, from the last `,` to the end of the integer part.
    grouping: Option<(usize, usize)>,
    min_integer: usize,
    min_fraction: usize,
    max_fraction: usize,
}

impl DecimalPattern {
    pub(super) fn parse(pattern: &str) -> Result<Self, &'static str> {
        let (integer, fraction) = match pattern.split_once('.') {
            Some((i, f)) => (i, f),
            None => (pattern, ""),
        };
        if integer.is_empty() || !integer.bytes().all(|b| matches!(b, b'#' | b'0' | b',')) {
            return Err("unsupported decimal-format pattern");
        }
        let mut groups = integer.rsplit(',');
        let primary = groups.next().unwrap_or_default().len();
        let grouping = groups.next().map(|secondary| (primary, secondary.len()));
        if grouping.is_some_and(|(a, b)| a == 0 || b == 0) {
            return Err("unsupported decimal-format pattern");
        }
        let grouping = grouping.map(|(a, b)| (a, if groups.next().is_some() { b } else { a }));
        let min_fraction = fraction.bytes().take_while(|&b| b == b'0').count();
        if !fraction[min_fraction..].bytes().all(|b| b == b'#') {
            return Err("unsupported decimal-format pattern");
        }
        Ok(DecimalPattern {
            grouping,
            min_integer: integer.bytes().filter(|&b| b == b'0').count(),
            min_fraction,
            max_fraction: fraction.len(),
        })
    }

    pub(super) fn format(&self, v: Value<'_>, symbols: &Symbols, out: &mut String) {
        if let Some(special) = v.special {
            if v.negative {
                out.push_str(&symbols.minus);
            }
            out.push_str(symbols.special(special));
            return;
        }
        let (integer, fraction) = round_half_even(v.integer, v.fraction, self.max_fraction);
        if v.negative {
            out.push_str(&symbols.minus);
        }
        let digits = match v.large_integer {
            Some(digits) => std::borrow::Cow::Borrowed(digits),
            None => format!("{integer:0width$}", width = self.min_integer).into(),
        };
        let local = |digit| {
            char::from_u32(u32::from(symbols.zero) + u32::from(digit))
                .expect("locale decimal digits")
        };
        for (index, digit) in digits.bytes().enumerate() {
            let remaining = digits.len() - index;
            if let Some((primary, secondary)) = self.grouping
                && index > 0
                && remaining >= primary
                && (remaining - primary).is_multiple_of(secondary)
            {
                out.push_str(&symbols.group);
            }
            out.push(local(digit - b'0'));
        }
        let mut keep = fraction.len();
        while keep > self.min_fraction && fraction[keep - 1] == 0 {
            keep -= 1;
        }
        if keep > 0 || self.min_fraction > 0 {
            out.push_str(&symbols.decimal);
            out.extend(fraction[..keep].iter().copied().map(local));
            out.extend(std::iter::repeat_n(
                symbols.zero,
                self.min_fraction.saturating_sub(keep),
            ));
        }
    }
}

/// `DecimalFormat`'s default rounding, on digit strings: keeps `max`
/// fraction digits, rounding ties to the even neighbour.
fn round_half_even(integer: u128, fraction: &str, max: usize) -> (u128, Vec<u8>) {
    let digits: Vec<u8> = fraction.bytes().map(|b| b - b'0').collect();
    if digits.len() <= max {
        return (integer, digits);
    }
    let (keep, rest) = digits.split_at(max);
    let mut keep = keep.to_vec();
    let last = keep.last().copied().unwrap_or((integer % 10) as u8);
    let up = match rest[0].cmp(&5) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => rest[1..].iter().any(|&d| d != 0) || last % 2 == 1,
    };
    if !up {
        return (integer, keep);
    }
    for d in keep.iter_mut().rev() {
        if *d == 9 {
            *d = 0;
        } else {
            *d += 1;
            return (integer, keep);
        }
    }
    (integer.saturating_add(1), keep)
}
