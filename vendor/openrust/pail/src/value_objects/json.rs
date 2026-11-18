use std::fmt::{self, Display as _, Write as _};

use serde_json::{Map, Value};

// PHP JSON_UNESCAPED_SLASHES retains object order, escapes Unicode and uses PHP float notation.
struct Json<'a>(&'a Value);

impl fmt::Display for Json<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Value::Null => formatter.write_str("null"),
            Value::Bool(value) => value.fmt(formatter),
            Value::Number(value) if value.is_f64() => {
                let value = value.as_f64().ok_or(fmt::Error)?;
                let absolute = value.abs();
                if absolute != 0.0 && !(0.0001..1e17).contains(&absolute) {
                    let exponent = format!("{value:e}");
                    let (mantissa, power) = exponent.split_once('e').ok_or(fmt::Error)?;
                    formatter.write_str(mantissa)?;
                    if !mantissa.contains('.') {
                        formatter.write_str(".0")?;
                    }
                    formatter.write_char('e')?;
                    if !power.starts_with('-') {
                        formatter.write_char('+')?;
                    }
                    formatter.write_str(power)
                } else {
                    value.fmt(formatter)
                }
            }
            Value::Number(value) => value.fmt(formatter),
            Value::String(value) => string(formatter, value),
            Value::Array(values) => {
                formatter.write_char('[')?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        formatter.write_char(',')?;
                    }
                    Json(value).fmt(formatter)?;
                }
                formatter.write_char(']')
            }
            Value::Object(values) => object(formatter, values),
        }
    }
}

pub(super) fn object(
    formatter: &mut fmt::Formatter<'_>,
    values: &Map<String, Value>,
) -> fmt::Result {
    formatter.write_char('{')?;
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            formatter.write_char(',')?;
        }
        string(formatter, key)?;
        formatter.write_char(':')?;
        Json(value).fmt(formatter)?;
    }
    formatter.write_char('}')
}

pub(super) fn string(formatter: &mut fmt::Formatter<'_>, value: &str) -> fmt::Result {
    let json = serde_json::to_string(value).map_err(|_| fmt::Error)?;
    for character in json.chars() {
        if character.is_ascii() {
            formatter.write_char(character)?;
        } else {
            for unit in character.encode_utf16(&mut [0; 2]) {
                write!(formatter, "\\u{unit:04x}")?;
            }
        }
    }
    Ok(())
}
