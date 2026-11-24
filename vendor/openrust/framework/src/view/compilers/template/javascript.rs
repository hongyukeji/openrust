use std::fmt::Write as _;

use super::{ViewResult, ViewValue, render_error};

pub(crate) fn write_expression(output: &mut String, value: &ViewValue) -> ViewResult<()> {
    match value {
        ViewValue::String(value) => {
            output.push('\'');
            write_escaped(output, value);
            output.push('\'');
        }
        ViewValue::Array(values) if !values.is_empty() => write_container(output, value)?,
        ViewValue::Object(values) if !values.is_empty() => write_container(output, value)?,
        ViewValue::Instance(_) => {
            let projection = value
                .to_json()
                .map_err(|error| super::ViewError::RenderFailed(Box::new(error)))?;
            write_expression(output, &projection.into())?;
        }
        ViewValue::Number(number) => write_number(output, number)?,
        _ => write!(output, "{value}").unwrap(),
    }
    Ok(())
}

fn write_container(output: &mut String, value: &ViewValue) -> ViewResult<()> {
    let mut payload = String::new();
    write_json(&mut payload, value, 0)?;
    output.push_str("JSON.parse('");
    write_escaped(output, &payload);
    output.push_str("')");
    Ok(())
}

fn write_json(output: &mut String, value: &ViewValue, depth: usize) -> ViewResult<()> {
    if depth >= 512 {
        return Err(render_error("Blade JavaScript data is nested too deeply."));
    }
    match value {
        ViewValue::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_json(output, value, depth + 1)?;
            }
            output.push(']');
        }
        ViewValue::Object(values) => {
            output.push('{');
            for (index, (name, value)) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                output.push('"');
                write_escaped(output, name);
                output.push_str("\":");
                write_json(output, value, depth + 1)?;
            }
            output.push('}');
        }
        ViewValue::String(value) => {
            output.push('"');
            write_escaped(output, value);
            output.push('"');
        }
        ViewValue::Instance(_) => {
            let projection = value
                .to_json()
                .map_err(|error| super::ViewError::RenderFailed(Box::new(error)))?;
            write_json(output, &projection.into(), depth + 1)?;
        }
        ViewValue::Number(number) => write_number(output, number)?,
        _ => write!(output, "{value}").unwrap(),
    }
    Ok(())
}

fn write_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '/' => output.push_str("\\/"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            '\u{C}' => output.push_str("\\f"),
            '<' | '>' | '&' | '\'' | '"' | '\u{2028}' | '\u{2029}' | '\u{0}'..='\u{1f}' => {
                write!(output, "\\u{:04X}", u32::from(character)).unwrap();
            }
            _ => output.push(character),
        }
    }
}

fn write_number(output: &mut String, number: &crate::view::ViewNumber) -> ViewResult<()> {
    if !number.is_finite() {
        return Err(render_error("Inf and NaN cannot be JSON encoded."));
    }
    let text = number.to_string();
    // Js uses PHP's default JSON flags, without JSON_PRESERVE_ZERO_FRACTION.
    output.push_str(if number.is_f64() {
        text.strip_suffix(".0").unwrap_or(&text)
    } else {
        &text
    });
    Ok(())
}
