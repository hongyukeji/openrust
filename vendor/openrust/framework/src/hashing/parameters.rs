//! PHP 参数转换与元数据扫描；不执行 KDF，也不施加计算预算。

fn decimal(text: &str) -> Option<(i64, &str)> {
    let text = text.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let bytes = text.as_bytes();
    let signed = bytes
        .first()
        .is_some_and(|byte| matches!(*byte, b'+' | b'-'));
    let start = usize::from(signed);
    let digits = bytes
        .get(start..)?
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let end = start + digits;
    let value = text.get(..end)?.parse().ok()?;
    Some((value, text.get(end..)?))
}

pub(super) fn bcrypt_cost(encoded: &str) -> i64 {
    encoded
        .strip_prefix("$2y$")
        .and_then(decimal)
        .map_or(12, |(value, _)| value)
}

pub(super) fn argon_parameters(encoded: &str, defaults: [i64; 3]) -> [i64; 3] {
    let Some(tail) = encoded
        .strip_prefix("$argon2id$v=")
        .or_else(|| encoded.strip_prefix("$argon2i$v="))
    else {
        return defaults;
    };
    let Some((_, tail)) = decimal(tail) else {
        return defaults;
    };
    let mut values = defaults;
    let Some(tail) = tail.strip_prefix("$m=") else {
        return values;
    };
    let Some((memory, tail)) = decimal(tail) else {
        return values;
    };
    values[0] = memory;
    let Some(tail) = tail.strip_prefix(",t=") else {
        return values;
    };
    let Some((time, tail)) = decimal(tail) else {
        return values;
    };
    values[1] = time;
    let Some(tail) = tail.strip_prefix(",p=") else {
        return values;
    };
    if let Some((threads, _)) = decimal(tail) {
        values[2] = threads;
    }
    values
}
