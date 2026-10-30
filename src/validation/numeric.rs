//! min/max 的十进制比较不经 f64，避免整数边界与数字字符串被舍入。
use serde_json::Value;
use std::cmp::Ordering;

struct Decimal {
    negative: bool,
    digits: String,
    point: i32,
}

pub(super) fn compare(value: &Value, bound: i64) -> std::result::Result<Option<Ordering>, ()> {
    let text = match value {
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        _ => return Ok(None),
    };
    let Some(value) = parse(&text)? else {
        return Ok(None);
    };
    let Some(bound) = parse(&bound.to_string())? else {
        return Err(());
    };
    if value.digits.is_empty() {
        return Ok(Some(if bound.digits.is_empty() {
            Ordering::Equal
        } else if bound.negative {
            Ordering::Greater
        } else {
            Ordering::Less
        }));
    }
    if bound.digits.is_empty() {
        return Ok(Some(if value.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }));
    }
    if value.negative != bound.negative {
        return Ok(Some(if value.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }));
    }
    let comparison = value.point.cmp(&bound.point).then_with(|| {
        let size = value.digits.len().max(bound.digits.len());
        (0..size)
            .map(|index| {
                (
                    value.digits.as_bytes().get(index).copied().unwrap_or(b'0'),
                    bound.digits.as_bytes().get(index).copied().unwrap_or(b'0'),
                )
            })
            .find_map(|(left, right)| (left != right).then(|| left.cmp(&right)))
            .unwrap_or(Ordering::Equal)
    });
    Ok(Some(if value.negative {
        comparison.reverse()
    } else {
        comparison
    }))
}

fn parse(value: &str) -> std::result::Result<Option<Decimal>, ()> {
    let value = value.trim_ascii();
    let negative = value.starts_with('-');
    let value = value.strip_prefix(['+', '-']).unwrap_or(value);
    let mut scientific = value.split(['e', 'E']);
    let mantissa = scientific.next().unwrap_or_default();
    let exponent = scientific.next();
    if scientific.next().is_some() {
        return Ok(None);
    }
    let mut parts = mantissa.split('.');
    let before = parts.next().unwrap_or_default();
    let after = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || before.is_empty() && after.is_empty()
        || !before
            .bytes()
            .chain(after.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return Ok(None);
    }
    let exponent = if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok(None);
        }
        let exponent = exponent.parse::<i32>().map_err(|_| ())?;
        if !(-1000..=1000).contains(&exponent) {
            return Err(());
        }
        exponent
    } else {
        0
    };
    let mut digits = format!("{before}{after}");
    let leading = digits.len() - digits.trim_start_matches('0').len();
    let point = i32::try_from(before.len())
        .map_err(|_| ())?
        .checked_add(exponent)
        .and_then(|value| value.checked_sub(i32::try_from(leading).ok()?))
        .ok_or(())?;
    digits.drain(..leading);
    digits.truncate(digits.trim_end_matches('0').len());
    if digits.is_empty() {
        return Ok(Some(Decimal {
            negative: false,
            digits: String::new(),
            point: 0,
        }));
    }
    Ok(Some(Decimal {
        negative,
        digits,
        point,
    }))
}
