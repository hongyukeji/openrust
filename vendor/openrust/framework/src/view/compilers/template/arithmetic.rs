use super::{ViewResult, ViewValue};
use crate::{
    contracts::container::Container as _,
    support::input_scalars::{decimal_float, numeric_prefix},
    view::rendering::render_error,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum Literal {
    Integer(i64),
    FloatBits(u64),
}
impl Literal {
    pub(super) fn number(self) -> Number {
        match self {
            Self::Integer(n) => Number::Integer(n),
            Self::FloatBits(n) => Number::Float(f64::from_bits(n)),
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Number {
    Integer(i64),
    Float(f64),
}
impl Number {
    pub(super) fn literal_node(self) -> Literal {
        match self {
            Self::Integer(n) => Literal::Integer(n),
            Self::Float(n) => Literal::FloatBits(n.to_bits()),
        }
    }
    pub(super) fn double(self) -> f64 {
        match self {
            Self::Integer(v) => v as f64,
            Self::Float(v) => v,
        }
    }
    pub(super) fn literal(source: &str) -> Option<Self> {
        static LITERAL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?x)^(?:0[xX][0-9a-fA-F](?:_?[0-9a-fA-F])*|0[bB][01](?:_?[01])*|0[oO][0-7](?:_?[0-7])*|(?:[0-9](?:_?[0-9])*(?:\.(?:[0-9](?:_?[0-9])*)?)?|\.[0-9](?:_?[0-9])*)(?:[eE][+-]?[0-9](?:_?[0-9])*)?)$").expect("valid numeric literal grammar")
        });
        if source == "INF" {
            return Some(Self::Float(f64::INFINITY));
        }
        if source == "NAN" {
            return Some(Self::Float(f64::NAN));
        }
        if !LITERAL.is_match(source) {
            return None;
        }
        let text = source.replace('_', "");
        let radix = if text.starts_with("0x") || text.starts_with("0X") {
            Some((16, &text[2..]))
        } else if text.starts_with("0b") || text.starts_with("0B") {
            Some((2, &text[2..]))
        } else if text.starts_with("0o") || text.starts_with("0O") {
            Some((8, &text[2..]))
        } else if text.len() > 1 && text.starts_with('0') && !text.contains(['.', 'e', 'E']) {
            Some((8, text.as_str()))
        } else {
            None
        };
        if let Some((radix, digits)) = radix {
            if !digits.chars().all(|c| c.is_digit(radix)) {
                return None;
            }
            return Some(
                i64::from_str_radix(digits, radix)
                    .map(Self::Integer)
                    .unwrap_or_else(|_| Self::Float(radix_float(digits, radix))),
            );
        }
        Self::decimal(&text)
    }
    fn decimal(text: &str) -> Option<Self> {
        text.parse::<i64>()
            .map(Self::Integer)
            .ok()
            .or_else(|| decimal_float(text).map(Self::Float))
    }
    pub(super) fn from_value(value: &ViewValue) -> ViewResult<Self> {
        match value {
            ViewValue::Null => Ok(Self::Integer(0)),
            ViewValue::Bool(v) => Ok(Self::Integer(i64::from(*v))),
            ViewValue::Number(n) => Ok(n
                .as_i64()
                .map(Self::Integer)
                .unwrap_or_else(|| Self::Float(n.as_f64().expect("JSON number")))),
            ViewValue::String(s) => {
                let s = s.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
                let prefix = numeric_prefix(s).ok_or_else(|| {
                    render_error("Unsupported operand types in Blade arithmetic.")
                })?;
                if prefix.len() != s.len() {
                    return Err(render_error(
                        "A non-numeric value encountered in Blade arithmetic.",
                    ));
                }
                Self::decimal(s).ok_or_else(|| render_error("Invalid Blade numeric operand."))
            }
            _ => Err(render_error(
                "Unsupported operand types in Blade arithmetic.",
            )),
        }
    }
    pub(super) fn json(self) -> ViewResult<ViewValue> {
        Ok(match self {
            Self::Integer(n) => n.into(),
            Self::Float(n) => n.into(),
        })
    }
    pub(super) fn negate(self) -> Self {
        match self {
            Self::Integer(n) => n
                .checked_neg()
                .map(Self::Integer)
                .unwrap_or(Self::Float(-(n as f64))),
            Self::Float(n) => Self::Float(-n),
        }
    }
    pub(super) fn modulo_integer(self) -> i64 {
        match self {
            Self::Integer(n) => n,
            Self::Float(n) => {
                let integer = crate::support::input_scalars::double_integer(n);
                if integer as f64 != n {
                    deprecation(&format!(
                        "Implicit conversion from float {} to int loses precision",
                        crate::support::input_scalars::double_string(n)
                    ));
                }
                integer
            }
        }
    }
    pub(super) fn modulo_value(value: &ViewValue) -> ViewResult<i64> {
        let number = Self::from_value(value)?;
        if let ViewValue::String(text) = value {
            let integer = crate::support::input_scalars::integer_string(text);
            if number.double().fract() != 0.0
                || number.double() >= -(i64::MIN as f64)
                || number.double() < i64::MIN as f64
            {
                deprecation(&format!(
                    "Implicit conversion from float-string \"{text}\" to int loses precision"
                ));
            }
            Ok(integer)
        } else {
            Ok(number.modulo_integer())
        }
    }
    pub(super) fn operation(self, op: Operator, rhs: Self) -> ViewResult<Self> {
        use Operator::*;
        if matches!(op, Modulo) {
            let (a, b) = (self.modulo_integer(), rhs.modulo_integer());
            if b == 0 {
                return Err(render_error("Modulo by zero in Blade arithmetic."));
            }
            return Ok(Self::Integer(a.checked_rem(b).unwrap_or(0)));
        }
        if matches!(op, Power) && self.double() == 0.0 && rhs.double() < 0.0 {
            deprecation("Power of base 0 and negative exponent is deprecated");
        }
        if matches!(op, Divide) && rhs.double() == 0.0 {
            return Err(render_error("Division by zero in Blade arithmetic."));
        }
        if let (Self::Integer(a), Self::Integer(b)) = (self, rhs) {
            let integer = match op {
                Add => a.checked_add(b),
                Subtract => a.checked_sub(b),
                Multiply => a.checked_mul(b),
                Divide if a.checked_rem(b) == Some(0) => a.checked_div(b),
                Power if b >= 0 => u32::try_from(b).ok().and_then(|b| a.checked_pow(b)),
                _ => None,
            };
            if let Some(n) = integer {
                return Ok(Self::Integer(n));
            }
            if matches!(op, Power) && b >= 0 && matches!(a, -1..=1) {
                return Ok(Self::Integer(if b == 0 || a == 1 {
                    1
                } else if a == 0 {
                    0
                } else if b % 2 == 0 {
                    1
                } else {
                    -1
                }));
            }
        }
        let (a, b) = (self.double(), rhs.double());
        Ok(Self::Float(match op {
            Add => a + b,
            Subtract => a - b,
            Multiply => a * b,
            Divide => a / b,
            Power => a.powf(b),
            Modulo => unreachable!(),
        }))
    }
}

pub(super) fn binary_position(source: &str, token: &str, right_associative: bool) -> Option<usize> {
    let mut positions = super::separators(source, token).filter(|index| {
        let before = source[..*index].trim_end();
        let after = &source[*index + token.len()..];
        if token == "*" && (before.ends_with('*') || after.starts_with('*')) {
            return false;
        }
        if matches!(token, "+" | "-") {
            if before.is_empty()
                || before.ends_with([
                    '+', '-', '*', '/', '%', '!', '&', '|', '?', '=', '<', '>', ',', ':',
                ])
                || (token == "-" && after.starts_with('>'))
            {
                return false;
            }
            if before.ends_with(['e', 'E']) {
                let mantissa = &before[..before.len() - 1];
                let start = mantissa
                    .rfind(|c: char| !c.is_ascii_digit() && c != '.' && c != '_')
                    .map_or(0, |i| i + 1);
                if (start == 0
                    || !mantissa[..start]
                        .ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$'))
                    && Number::literal(&mantissa[start..]).is_some()
                {
                    return false;
                }
            }
        }
        true
    });
    if right_associative {
        positions.next()
    } else {
        positions.last()
    }
}

// Power-of-two bases can be rounded once using a mantissa, guard and sticky bits.
fn radix_float(digits: &str, radix: u32) -> f64 {
    let width = radix.trailing_zeros();
    let (mut count, mut mantissa, mut guard, mut sticky) = (0usize, 0u64, false, false);
    for digit in digits
        .chars()
        .map(|c| c.to_digit(radix).expect("validated radix"))
    {
        for shift in (0..width).rev() {
            let bit = (digit >> shift) & 1;
            if count == 0 && bit == 0 {
                continue;
            }
            count += 1;
            if count <= 53 {
                mantissa = (mantissa << 1) | u64::from(bit);
            } else if count == 54 {
                guard = bit != 0;
            } else {
                sticky |= bit != 0;
            }
        }
    }
    if count <= 53 {
        return mantissa as f64;
    }
    if guard && (sticky || mantissa & 1 != 0) {
        mantissa += 1;
    }
    (mantissa as f64) * 2f64.powi(i32::try_from(count - 53).unwrap_or(i32::MAX))
}

// PHP deprecations are warnings, not render failures. Logging is best-effort.
#[track_caller]
fn deprecation(message: &str) {
    let source = std::panic::Location::caller();
    use crate::{
        log::{Log, LogContext},
        support::facades::Config,
    };
    let _ = (|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some(container) = crate::support::facades::current_container() else {
            return Ok(());
        };
        if container
            .get::<str>("env")
            .is_ok_and(|env| env.as_ref() == "testing")
        {
            let enabled = container
                .make::<crate::config::Environment>()
                .ok()
                .and_then(|env| {
                    env.get_utf8("LOG_DEPRECATIONS_WHILE_TESTING")
                        .ok()
                        .flatten()
                        .map(str::to_owned)
                })
                .is_some_and(|value| environment_truthy(&value));
            if !enabled {
                return Ok(());
            }
        }
        if Config::get("logging.channels.deprecations")?.is_none() {
            let config = Config::get("logging.deprecations")?.unwrap_or(serde_json::Value::Null);
            let name = config
                .as_str()
                .or_else(|| config.get("channel").and_then(serde_json::Value::as_str))
                .unwrap_or("null");
            let channel = Config::get(&format!("logging.channels.{name}"))?
                .unwrap_or_else(|| serde_json::json!({"driver":"null"}));
            Config::set("logging.channels.deprecations", channel)?;
        }
        let logger = Log::channel(Some("deprecations"))?;
        if Config::get("logging.deprecations.trace")?.is_some_and(|v| v == true) {
            let mut context = LogContext::new();
            context.insert("file".into(), source.file().into());
            context.insert("line".into(), source.line().into());
            context.insert(
                "trace".into(),
                std::backtrace::Backtrace::force_capture()
                    .to_string()
                    .into(),
            );
            logger.logException(
                crate::log::Level::Warning,
                message,
                &std::io::Error::other(message),
                &context,
            )?;
        } else {
            logger.warning(
                &format!("{message} in {} on line {}", source.file(), source.line()),
                &LogContext::new(),
            )?;
        }
        Ok(())
    })();
}

fn environment_truthy(value: &str) -> bool {
    match value.to_ascii_lowercase().as_str() {
        "true" | "(true)" => true,
        "false" | "(false)" | "empty" | "(empty)" | "null" | "(null)" => false,
        _ => {
            let value = ['\'', '"']
                .into_iter()
                .find_map(|quote| {
                    value
                        .strip_prefix(quote)
                        .and_then(|v| v.strip_suffix(quote))
                        .filter(|v| !v.contains('\n'))
                })
                .unwrap_or(value);
            !value.is_empty() && value != "0"
        }
    }
}
