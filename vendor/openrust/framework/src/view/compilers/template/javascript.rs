use std::fmt::Write as _;

use super::{ViewResult, ViewValue, render_error};

pub(super) fn write_expression(output: &mut String, value: &ViewValue) -> ViewResult<()> {
    match value {
        ViewValue::String(value) => {
            output.push('\'');
            write_escaped(output, value);
            output.push('\'');
        }
        ViewValue::Array(values) if !values.is_empty() => write_container(output, value)?,
        ViewValue::Object(values) if !values.is_empty() => write_container(output, value)?,
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
