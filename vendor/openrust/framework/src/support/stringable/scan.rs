use serde::Serialize;

use crate::support::Collection;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ScanValue {
    Integer(i64),
    Float(f64),
    Text(String),
    /// A byte-width field can cut through a UTF-8 scalar.
    Bytes(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    InvalidConversion,
    UnclosedSet,
    MixedPositions,
    InvalidPosition,
    DuplicatePosition,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidConversion => "invalid scan conversion",
            Self::UnclosedSet => "unclosed scan character set",
            Self::MixedPositions => "cannot mix positional and sequential scan fields",
            Self::InvalidPosition => "scan position must be between 1 and 255",
            Self::DuplicatePosition => "scan position is assigned more than once",
        })
    }
}

impl std::error::Error for ScanError {}

enum Step {
    Whitespace,
    Literal(u8),
    Field {
        kind: Kind,
        width: usize,
        slot: Option<usize>,
    },
}

enum Kind {
    Count,
    Integer { radix: u32, unsigned: bool },
    Float,
    Text { skip_space: bool },
    Set(Box<[bool; 256]>),
}

enum Failure {
    Mismatch,
    Underflow,
}

pub(super) fn scan(input: &str, format: &str) -> Result<Collection<Option<ScanValue>>, ScanError> {
    let (steps, count) = parse_format(until_null(format))?;
    let input = until_null(input);
    let mut fields = vec![None; count];
    let mut offset = 0;
    let mut converted = false;
    for step in steps {
        let rest = &input[offset..];
        match step {
            Step::Whitespace => offset += spaces(rest),
            Step::Literal(byte) => match rest.first() {
                Some(&actual) if actual == byte => offset += 1,
                Some(_) => break,
                None if !converted => return Ok(Collection::new()),
                None => break,
            },
            Step::Field {
                kind: Kind::Count,
                slot,
                ..
            } => {
                if let Some(slot) = slot {
                    fields[slot] = Some(ScanValue::Integer(
                        i64::try_from(offset).expect("string length fits i64"),
                    ));
                }
                converted = true;
            }
            Step::Field { kind, width, slot } => match read_field(rest, &kind, width) {
                Ok((value, consumed)) => {
                    if let Some(slot) = slot {
                        fields[slot] = Some(value);
                    }
                    offset += consumed;
                    converted = true;
                }
                Err(Failure::Underflow) if !converted => return Ok(Collection::new()),
                Err(_) => break,
            },
        }
    }
    Ok(Collection::make(fields))
}

fn parse_format(mut format: &[u8]) -> Result<(Vec<Step>, usize), ScanError> {
    let mut steps = Vec::new();
    let mut positions = [false; 255];
    let mut count = 0;
    let mut positional = None;
    while let Some((&byte, remaining)) = format.split_first() {
        format = remaining;
        if whitespace(byte) {
            steps.push(Step::Whitespace);
            format = &format[spaces(format)..];
            continue;
        }
        if byte != b'%' || format.first() == Some(&b'%') {
            steps.push(Step::Literal(byte));
            if byte == b'%' {
                format = &format[1..];
            }
            continue;
        }

        let suppress = format.first() == Some(&b'*');
        if suppress {
            format = &format[1..];
        }
        let (number, digits) = prefix_number(format);
        let explicit = !suppress && digits > 0 && format.get(digits) == Some(&b'$');
        let slot = if suppress {
            None
        } else {
            if positional.is_some_and(|mode| mode != explicit) {
                return Err(ScanError::MixedPositions);
            }
            positional = Some(explicit);
            if explicit {
                let slot = number
                    .checked_sub(1)
                    .filter(|&slot| slot < positions.len())
                    .ok_or(ScanError::InvalidPosition)?;
                if std::mem::replace(&mut positions[slot], true) {
                    return Err(ScanError::DuplicatePosition);
                }
                format = &format[digits + 1..];
                count = count.max(slot + 1);
                Some(slot)
            } else {
                count += 1;
                Some(count - 1)
            }
        };

        let (width, digits) = prefix_number(format);
        format = &format[digits..];
        if matches!(format.first(), Some(b'l' | b'L' | b'h')) {
            format = &format[1..];
        }
        let (&conversion, rest) = format.split_first().ok_or(ScanError::InvalidConversion)?;
        format = rest;
        let kind = match conversion {
            b'n' => Kind::Count,
            b'd' | b'D' => Kind::Integer {
                radix: 10,
                unsigned: false,
            },
            b'i' => Kind::Integer {
                radix: 0,
                unsigned: false,
            },
            b'o' => Kind::Integer {
                radix: 8,
                unsigned: false,
            },
            b'x' | b'X' => Kind::Integer {
                radix: 16,
                unsigned: false,
            },
            b'u' => Kind::Integer {
                radix: 10,
                unsigned: true,
            },
            b'f' | b'e' | b'E' | b'g' => Kind::Float,
            b's' => Kind::Text { skip_space: true },
            b'c' => Kind::Text { skip_space: false },
            b'[' => {
                let (set, rest) = parse_set(format)?;
                format = rest;
                Kind::Set(set)
            }
            _ => return Err(ScanError::InvalidConversion),
        };
        let width = match (conversion, width) {
            (b'c', 0) => 1,
            (_, 0) => usize::MAX,
            (_, width) => width,
        };
        steps.push(Step::Field { kind, width, slot });
    }
    Ok((steps, count))
}

type ByteSet = Box<[bool; 256]>;

fn parse_set(format: &[u8]) -> Result<(ByteSet, &[u8]), ScanError> {
    let inverted = format.first() == Some(&b'^');
    let format = &format[usize::from(inverted)..];
    let end = format
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, &byte)| (byte == b']').then_some(index))
        .ok_or(ScanError::UnclosedSet)?;
    let mut set = Box::new([inverted; 256]);
    let mut members = &format[..end];
    if matches!(members.first(), Some(b'-' | b']')) {
        set[usize::from(members[0])] = !inverted;
        members = &members[1..];
    }
    while let Some((&first, rest)) = members.split_first() {
        if let [b'-', last, tail @ ..] = rest {
            let start = usize::from(first.min(*last));
            let finish = usize::from(first.max(*last));
            set[start..=finish].fill(!inverted);
            members = tail;
        } else {
            set[usize::from(first)] = !inverted;
            members = rest;
        }
    }
    Ok((set, &format[end + 1..]))
}

fn read_field(rest: &[u8], kind: &Kind, width: usize) -> Result<(ScanValue, usize), Failure> {
    let skip = match kind {
        Kind::Set(_) | Kind::Text { skip_space: false } => 0,
        _ => spaces(rest),
    };
    let rest = &rest[skip..];
    if rest.is_empty() {
        return Err(Failure::Underflow);
    }
    let field = &rest[..width.min(rest.len())];
    let (value, length) = match kind {
        Kind::Text { .. } => {
            let length = field.iter().take_while(|&&byte| !whitespace(byte)).count();
            (text(&field[..length]), length)
        }
        Kind::Set(set) => {
            let length = field
                .iter()
                .take_while(|&&byte| set[usize::from(byte)])
                .count();
            if length == 0 {
                return Err(Failure::Mismatch);
            }
            (text(&field[..length]), length)
        }
        Kind::Integer { radix, unsigned } => {
            integer(&field[..field.len().min(63)], rest.len(), *radix, *unsigned)?
        }
        Kind::Float => float(&field[..field.len().min(63)], rest.len())?,
        Kind::Count => unreachable!("count handled without consuming input"),
    };
    Ok((value, skip + length))
}

fn integer(
    field: &[u8],
    input_len: usize,
    base: u32,
    unsigned: bool,
) -> Result<(ScanValue, usize), Failure> {
    let sign = usize::from(matches!(field.first(), Some(b'-' | b'+')));
    let negative = field.first() == Some(&b'-');
    let token = &field[sign..];
    let hex = sign == 0
        && matches!(base, 0 | 16)
        && (token.starts_with(b"0x") || token.starts_with(b"0X"))
        && token.get(2).is_some_and(u8::is_ascii_hexdigit);
    let radix = if hex {
        16
    } else if base != 0 {
        base
    } else if token.first() == Some(&b'0') {
        8
    } else {
        10
    };
    let start = sign + if hex { 2 } else { 0 };
    let mut magnitude = 0_u64;
    let mut overflow = false;
    let mut end = start;
    for &byte in &field[start..] {
        let Some(digit) = char::from(byte).to_digit(radix) else {
            break;
        };
        if let Some(value) = magnitude
            .checked_mul(u64::from(radix))
            .and_then(|value| value.checked_add(u64::from(digit)))
        {
            magnitude = value;
        } else {
            overflow = true;
            magnitude = u64::MAX;
        }
        end += 1;
    }
    if end == start {
        return Err(if end == input_len {
            Failure::Underflow
        } else {
            Failure::Mismatch
        });
    }
    let value = if unsigned {
        let magnitude = if negative && !overflow {
            magnitude.wrapping_neg()
        } else {
            magnitude
        };
        i64::try_from(magnitude).map_or_else(
            |_| ScanValue::Text(magnitude.to_string()),
            ScanValue::Integer,
        )
    } else if negative {
        ScanValue::Integer(i64::try_from(-i128::from(magnitude)).unwrap_or(i64::MIN))
    } else {
        ScanValue::Integer(i64::try_from(magnitude).unwrap_or(i64::MAX))
    };
    Ok((value, end))
}

fn float(field: &[u8], input_len: usize) -> Result<(ScanValue, usize), Failure> {
    let sign = usize::from(matches!(field.first(), Some(b'-' | b'+')));
    let mut end = sign + decimal_digits(&field[sign..]);
    let mut digits = end - sign;
    if field.get(end) == Some(&b'.') {
        end += 1;
        let fraction = decimal_digits(&field[end..]);
        digits += fraction;
        end += fraction;
    }
    if digits == 0 {
        return Err(if end == input_len {
            Failure::Underflow
        } else {
            Failure::Mismatch
        });
    }
    if matches!(field.get(end), Some(b'e' | b'E')) {
        let exponent = &field[end + 1..];
        let exponent_sign = usize::from(matches!(exponent.first(), Some(b'-' | b'+')));
        let exponent_digits = decimal_digits(&exponent[exponent_sign..]);
        if exponent_digits > 0 {
            end += 1 + exponent_sign + exponent_digits;
        }
    }
    let value = std::str::from_utf8(&field[..end])
        .expect("ASCII numeric token")
        .parse()
        .expect("validated decimal token");
    Ok((ScanValue::Float(value), end))
}

fn text(bytes: &[u8]) -> ScanValue {
    match std::str::from_utf8(bytes) {
        Ok(value) => ScanValue::Text(value.into()),
        Err(_) => ScanValue::Bytes(bytes.to_vec()),
    }
}

fn prefix_number(input: &[u8]) -> (usize, usize) {
    let length = decimal_digits(input);
    let number = input[..length].iter().fold(0_usize, |number, &digit| {
        number
            .saturating_mul(10)
            .saturating_add(usize::from(digit - b'0'))
    });
    (number, length)
}

fn decimal_digits(input: &[u8]) -> usize {
    input
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count()
}

fn spaces(input: &[u8]) -> usize {
    input.iter().take_while(|&&byte| whitespace(byte)).count()
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t'..=b'\r')
}

fn until_null(input: &str) -> &[u8] {
    input
        .as_bytes()
        .split(|&byte| byte == 0)
        .next()
        .unwrap_or_default()
}
