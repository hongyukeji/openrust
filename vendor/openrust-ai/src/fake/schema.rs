use serde_json::{Map, Value};

use crate::{AiError, AiResult};

pub(super) fn sample(schema: &Value) -> AiResult<Value> {
    let kind = schema["type"].as_str().or_else(|| {
        schema["type"].as_array().and_then(|types| {
            types
                .iter()
                .filter_map(Value::as_str)
                .find(|kind| *kind != "null")
        })
    });
    if let Some(value) = schema["enum"].as_array().and_then(|values| values.first()) {
        return Ok(if kind == Some("array") && !value.is_array() {
            Value::Array(vec![value.clone()])
        } else {
            value.clone()
        });
    }
    if let Some(default) = schema.get("default") {
        return Ok(default.clone());
    }
    if let Some(branches) = schema["anyOf"].as_array() {
        return sample(branches.first().ok_or(AiError::InvalidSchema)?);
    }
    match kind {
        Some("object") => {
            let mut fields = Map::new();
            if let Some(properties) = schema["properties"].as_object() {
                for (name, schema) in properties {
                    fields.insert(name.clone(), sample(schema)?);
                }
            }
            Ok(Value::Object(fields))
        }
        Some("array") => {
            let mut values = Vec::new();
            if let Some(item) = schema.get("items") {
                let count = size(schema, "minItems", "maxItems")?;
                values
                    .try_reserve_exact(count)
                    .map_err(|_| AiError::InvalidSchema)?;
                for _ in 0..count {
                    values.push(sample(item)?);
                }
            }
            Ok(Value::Array(values))
        }
        Some("string") => string(schema).map(Value::String),
        Some("integer") => integer(schema),
        Some("number") => number(schema),
        Some("boolean") => Ok(Value::Bool(false)),
        _ => Ok(Value::Null),
    }
}

fn size(schema: &Value, minimum: &str, maximum: &str) -> AiResult<usize> {
    let maximum = schema[maximum].as_u64().unwrap_or(u64::MAX);
    let minimum = schema[minimum].as_u64().unwrap_or(1.min(maximum));
    if minimum > maximum {
        return Err(AiError::InvalidSchema);
    }
    usize::try_from(minimum).map_err(|_| AiError::InvalidSchema)
}

fn string(schema: &Value) -> AiResult<String> {
    let size = size(schema, "minLength", "maxLength")?;
    let formatted = match schema["format"].as_str() {
        Some("date") => Some("2000-01-01"),
        Some("date-time") => Some("2000-01-01T00:00:00Z"),
        Some("email") => Some("user@example.com"),
        Some("time") => Some("00:00:00"),
        Some("uri" | "url") => Some("https://example.com"),
        Some("uuid") => Some("00000000-0000-4000-8000-000000000000"),
        _ => None,
    };
    if let Some(value) = formatted {
        return Ok(value.into());
    }
    let mut value = String::new();
    value
        .try_reserve_exact(size)
        .map_err(|_| AiError::InvalidSchema)?;
    value.extend(std::iter::repeat_n('a', size));
    Ok(value)
}

fn integer(schema: &Value) -> AiResult<Value> {
    let minimum = integer_bound(&schema["minimum"], f64::ceil)
        .unwrap_or(i64::MIN.into())
        .max(i64::MIN.into());
    let maximum = integer_bound(&schema["maximum"], f64::floor)
        .unwrap_or(u64::MAX.into())
        .min(u64::MAX.into());
    if minimum > maximum {
        return Err(AiError::InvalidSchema);
    }
    let multiple = schema["multipleOf"].as_number();
    if multiple.is_some_and(|value| value.as_f64().is_none_or(|value| value <= 0.0)) {
        return Err(AiError::InvalidSchema);
    }
    let selected = if minimum <= 0 && maximum >= 0 {
        0
    } else {
        let multiple = multiple.map(integer_multiple).transpose()?.unwrap_or(1);
        if minimum > 0 {
            ((minimum - 1) / multiple + 1)
                .checked_mul(multiple)
                .ok_or(AiError::InvalidSchema)?
        } else {
            maximum.div_euclid(multiple) * multiple
        }
    };
    if selected < minimum || selected > maximum {
        return Err(AiError::InvalidSchema);
    }
    if let Ok(signed) = i64::try_from(selected) {
        Ok(Value::from(signed))
    } else {
        u64::try_from(selected)
            .map(Value::from)
            .map_err(|_| AiError::InvalidSchema)
    }
}

fn integer_bound(value: &Value, round: fn(f64) -> f64) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
        .or_else(|| value.as_f64().map(|value| round(value) as i128))
}

fn integer_multiple(value: &serde_json::Number) -> AiResult<i128> {
    if let Some(integer) = value.as_u64() {
        return Ok(integer.into());
    }
    // 整数是分数 n / d 的倍数时，也必为约分后分子的倍数。
    let decimal = value.to_string();
    let (mantissa, exponent) = decimal.split_once('e').unwrap_or((&decimal, "0"));
    let exponent: i32 = exponent.parse().map_err(|_| AiError::InvalidSchema)?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut numerator: i128 = format!("{whole}{fraction}")
        .parse()
        .map_err(|_| AiError::InvalidSchema)?;
    let places = fraction.len() as i32 - exponent;
    if places < 0 {
        return 10_i128
            .checked_pow(places.unsigned_abs())
            .and_then(|power| numerator.checked_mul(power))
            .ok_or(AiError::InvalidSchema);
    }
    for factor in [2, 5] {
        for _ in 0..places {
            if numerator % factor != 0 {
                break;
            }
            numerator /= factor;
        }
    }
    Ok(numerator)
}

fn number(schema: &Value) -> AiResult<Value> {
    let minimum = schema["minimum"].as_f64().unwrap_or(f64::MIN);
    let maximum = schema["maximum"].as_f64().unwrap_or(f64::MAX);
    if minimum > maximum {
        return Err(AiError::InvalidSchema);
    }
    let multiple = schema["multipleOf"].as_f64();
    let selected = match multiple {
        Some(value) if value <= 0.0 => return Err(AiError::InvalidSchema),
        Some(value) if minimum > 0.0 => (minimum / value).next_down().ceil() * value,
        Some(value) if maximum < 0.0 => (maximum / value).next_up().floor() * value,
        _ => 0.0_f64.clamp(minimum, maximum),
    };
    if selected.next_up() < minimum || selected.next_down() > maximum {
        return Err(AiError::InvalidSchema);
    }
    serde_json::Number::from_f64(selected.clamp(minimum, maximum))
        .map(Value::Number)
        .ok_or(AiError::InvalidSchema)
}
