use crate::{Error, Result};
use pcre2::bytes::Regex;
use serde_json::Value;

fn expression(pattern: &str) -> Result<Regex> {
    let mut chars = pattern.char_indices();
    let (_, delimiter) = chars
        .next()
        .ok_or_else(|| Error::InvalidConfiguration("empty regular expression".into()))?;
    let closing = match delimiter {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        c => c,
    };
    if delimiter.is_alphanumeric() || delimiter == '\\' || delimiter.is_whitespace() {
        return Err(Error::InvalidConfiguration(
            "regular expression requires PHP delimiters".into(),
        ));
    }
    let end = pattern
        .char_indices()
        .rev()
        .find(|(index, c)| *index > 0 && *c == closing)
        .map(|(i, _)| i)
        .ok_or_else(|| Error::InvalidConfiguration("unclosed regular expression".into()))?;
    let mut options = String::new();
    for modifier in pattern[end + closing.len_utf8()..].chars() {
        match modifier {
            'i' | 'm' | 's' | 'x' | 'U' => options.push(modifier),
            'u' => {}
            _ => {
                return Err(Error::InvalidConfiguration(format!(
                    "unsupported regular expression modifier [{modifier}]"
                )));
            }
        }
    }
    let body = &pattern[delimiter.len_utf8()..end];
    let body = if options.is_empty() {
        body.to_owned()
    } else {
        format!("(?{options}){body}")
    };
    pcre2::bytes::RegexBuilder::new()
        .utf(pattern[end + closing.len_utf8()..].contains('u'))
        .build(&body)
        .map_err(|e| Error::InvalidConfiguration(e.to_string()))
}
pub fn matches(pattern: &str, value: &str) -> Result<bool> {
    expression(pattern)?
        .is_match(value.as_bytes())
        .map_err(|e| Error::InvalidConfiguration(e.to_string()))
}
pub fn ignored(patterns: &Value, value: &str) -> Result<bool> {
    for p in patterns
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if matches(p, value)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn rules(value: &Value) -> Result<Vec<(&str, &Value)>> {
    match value {
        Value::Null => Ok(vec![]),
        Value::Object(map) => Ok(map
            .iter()
            .map(|(key, value)| (key.as_str(), value))
            .collect()),
        Value::Array(list) => list
            .iter()
            .map(|rule| {
                let pair = rule.as_array().filter(|p| p.len() == 2).ok_or_else(|| {
                    Error::InvalidConfiguration(
                        "ordered rules require [pattern, value] pairs".into(),
                    )
                })?;
                let pattern = pair[0].as_str().ok_or_else(|| {
                    Error::InvalidConfiguration("rule pattern must be a string".into())
                })?;
                Ok((pattern, &pair[1]))
            })
            .collect(),
        _ => Err(Error::InvalidConfiguration(
            "rules require an object or ordered pairs".into(),
        )),
    }
}
pub fn group(groups: &Value, value: &str) -> Result<String> {
    for (pattern, replacement) in rules(groups)? {
        let Some(replacement) = replacement.as_str() else {
            return Err(Error::InvalidConfiguration(
                "group replacement must be a string".into(),
            ));
        };
        let regex = expression(pattern)?;
        let mut output = Vec::new();
        let mut last = 0;
        let mut changed = false;
        for capture in regex.captures_iter(value.as_bytes()) {
            let capture = capture.map_err(|e| Error::InvalidConfiguration(e.to_string()))?;
            let found = capture.get(0).ok_or(Error::StateUnavailable)?;
            output.extend_from_slice(&value.as_bytes()[last..found.start()]);
            let replacement = replacement.as_bytes();
            let mut i = 0;
            while i < replacement.len() {
                if (replacement[i] == b'$' || replacement[i] == b'\\') && i + 1 < replacement.len()
                {
                    let mut start = i + 1;
                    let brace = replacement[start] == b'{';
                    if brace {
                        start += 1;
                    }
                    let mut end = start;
                    while end < replacement.len()
                        && replacement[end].is_ascii_digit()
                        && end - start < 2
                    {
                        end += 1;
                    }
                    if end > start && (!brace || replacement.get(end) == Some(&b'}')) {
                        let n = std::str::from_utf8(&replacement[start..end])
                            .ok()
                            .and_then(|v| v.parse::<usize>().ok())
                            .ok_or(Error::StateUnavailable)?;
                        if let Some(v) = capture.get(n) {
                            output.extend_from_slice(v.as_bytes());
                        }
                        i = end + usize::from(brace);
                        continue;
                    }
                    if replacement[i] == b'\\' && replacement[i + 1] == b'\\' {
                        output.push(b'\\');
                        i += 2;
                        continue;
                    }
                }
                output.push(replacement[i]);
                i += 1;
            }
            last = found.end();
            changed = true;
        }
        if changed {
            output.extend_from_slice(&value.as_bytes()[last..]);
            return String::from_utf8(output).map_err(|_| Error::StateUnavailable);
        }
    }
    Ok(value.into())
}
pub fn threshold(config: &Value, key: &str) -> Result<f64> {
    if let Some(n) = config.as_f64() {
        return Ok(n);
    }
    let rules = rules(config)?;
    for (pattern, value) in &rules {
        if *pattern != "default" && matches(pattern, key)? {
            return value
                .as_f64()
                .ok_or_else(|| Error::InvalidConfiguration("threshold must be numeric".into()));
        }
    }
    Ok(rules
        .iter()
        .find(|(pattern, _)| *pattern == "default")
        .and_then(|(_, v)| v.as_f64())
        .unwrap_or(1000.0))
}
pub fn sample(rate: f64, seed: Option<&str>) -> Result<bool> {
    if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
        return Err(Error::InvalidConfiguration(
            "sample_rate must be between zero and one".into(),
        ));
    }
    if let Some(seed) = seed {
        let n = u128::from_be_bytes(md5::compute(seed.as_bytes()).0);
        return Ok(n as f64 / 2f64.powi(128) <= rate);
    }
    Ok(rate == 1.0 || (rate != 0.0 && fastrand::f64() < rate))
}
