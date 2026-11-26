use super::NumberError;
use fixed_decimal::{Decimal, Sign};
use icu_decimal::DecimalFormatter;

pub(super) fn decimal(formatter: &DecimalFormatter, input: &str) -> Result<String, NumberError> {
    let digits: [char; 10] = std::array::from_fn(|value| {
        formatter
            .format_to_string(&Decimal::from(u8::try_from(value).expect("decimal digit")))
            .chars()
            .next()
            .expect("formatted decimal digit")
    });
    let digit_value = |character: char| {
        character.to_digit(10).or_else(|| {
            digits
                .iter()
                .position(|&digit| digit == character)
                .and_then(|index| u32::try_from(index).ok())
        })
    };
    let zero = digits[0];
    let one = digits[1];
    let fraction = formatter.format_to_string(&"0.1".parse().expect("decimal literal"));
    let decimal_separator = fraction
        .strip_prefix(zero)
        .and_then(|value| value.strip_suffix(one))
        .ok_or(NumberError::InvalidNumber)?;
    let grouped = formatter.format_to_string(&Decimal::from(111_111_111_111_i64));
    let group_separator = grouped.trim_start_matches(one).trim_end_matches(one);
    let group_separator = group_separator.split(one).next().unwrap_or_default();
    let group_sizes: Vec<usize> = if group_separator.is_empty() {
        Vec::new()
    } else {
        grouped
            .split(group_separator)
            .map(|group| group.chars().count())
            .collect()
    };
    let negative = formatter.format_to_string(&Decimal::from(0).with_sign(Sign::Negative));
    let positive = formatter.format_to_string(&Decimal::from(0).with_sign(Sign::Positive));
    let negative_prefix = negative.split_once(zero).map_or("-", |(prefix, _)| prefix);
    let positive_prefix = positive.split_once(zero).map_or("+", |(prefix, _)| prefix);
    let mut rest = input.trim_start();
    let mut output = String::new();
    if let Some(value) = strip_sign(rest, negative_prefix, '-') {
        rest = value;
        output.push('-');
    } else if let Some(value) = strip_sign(rest, positive_prefix, '+') {
        rest = value;
    }
    if rest.starts_with("∞") {
        output.push_str("inf");
        return Ok(output);
    }
    if rest.starts_with("NaN") {
        return Ok("NaN".into());
    }

    let mut whole_digits = 0;
    let mut fraction_digits = 0;
    let mut groups = Vec::new();
    let mut in_fraction = false;
    while let Some(character) = rest.chars().next() {
        if let Some(digit) = digit_value(character) {
            output.push(char::from_digit(digit, 10).expect("decimal digit"));
            if in_fraction {
                fraction_digits += 1;
            } else {
                whole_digits += 1;
            }
            rest = &rest[character.len_utf8()..];
        } else if !in_fraction && let Some(value) = rest.strip_prefix(decimal_separator) {
            if whole_digits == 0 {
                output.push('0');
            }
            output.push('.');
            in_fraction = true;
            rest = value;
        } else if !in_fraction
            && !group_separator.is_empty()
            && let Some(value) = rest.strip_prefix(group_separator)
            && value.chars().next().and_then(digit_value).is_some()
        {
            groups.push(whole_digits);
            whole_digits = 0;
            rest = value;
        } else {
            break;
        }
    }

    if whole_digits + fraction_digits == 0 {
        return Err(NumberError::InvalidNumber);
    }
    if !groups.is_empty() {
        let primary = group_sizes.last().copied().unwrap_or(0);
        let secondary = group_sizes.iter().rev().nth(1).copied().unwrap_or(primary);
        if whole_digits != primary
            || !(1..=secondary).contains(&groups[0])
            || groups[1..].iter().any(|length| *length != secondary)
        {
            return Err(NumberError::InvalidNumber);
        }
    }
    if output.ends_with('.') {
        output.pop();
    }
    if let Some(exponent) = rest.strip_prefix(['e', 'E']) {
        let digits = exponent
            .trim_start_matches(['-', '+'])
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits > 0 {
            let sign = usize::from(exponent.starts_with(['-', '+']));
            let exponent = exponent[..sign + digits]
                .parse::<i16>()
                .map_err(|_| NumberError::InvalidNumber)?;
            let mut value = output
                .parse::<Decimal>()
                .map_err(|_| NumberError::InvalidNumber)?;
            // The maximum exponent is deliberately bounded to representable f64 input.
            if !(-400..=400).contains(&exponent) {
                return Err(NumberError::InvalidNumber);
            }
            value.multiply_pow10(exponent);
            output = value.to_string();
        }
    }
    Ok(output)
}

fn strip_sign<'a>(input: &'a str, prefix: &str, ascii: char) -> Option<&'a str> {
    (!prefix.is_empty())
        .then(|| input.strip_prefix(prefix))
        .flatten()
        .or_else(|| input.strip_prefix(ascii))
}
