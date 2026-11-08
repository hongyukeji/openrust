//! 固定 PHP JSON flags 在 Rust JSON 值载体上的编码。

use super::Json;
use crate::{Error, Result, support::InputNumber};
use serde_json::Value;

const MAX_DEPTH: usize = 512;

pub(super) fn encode(value: &Value, flags: u32) -> Result<Value> {
    let mut output = String::new();
    let mut frames: Vec<Frame<'_>> = Vec::new();
    let mut current = Some(value);
    while let Some(value) = current.take() {
        if let Some(items) = Items::for_value(value, flags) {
            if frames.len() == MAX_DEPTH && flags & Json::PARTIAL_OUTPUT_ON_ERROR == 0 {
                return invalid(flags, "model_json_depth", "模型 JSON 超出默认编码深度");
            }
            output.push(if items.is_object() { '{' } else { '[' });
            frames.push(Frame { items, first: true });
        } else if !scalar(&mut output, value, flags)? {
            return invalid(flags, "model_json_number", "模型 JSON 数字无法编码");
        }

        loop {
            let depth = frames.len();
            let Some(frame) = frames.last_mut() else {
                break;
            };
            if let Some((key, value)) = frame.items.next() {
                if !frame.first {
                    output.push(',');
                }
                frame.first = false;
                indent(&mut output, depth, flags);
                if let Some(key) = key {
                    match key {
                        Key::Index(index) => {
                            output.push('"');
                            output.push_str(&index.to_string());
                            output.push('"');
                        }
                        Key::Text(key) => string(&mut output, key, flags),
                    }
                    output.push(':');
                    if flags & Json::PRETTY_PRINT != 0 {
                        output.push(' ');
                    }
                }
                current = Some(value);
                break;
            }
            if !frame.first {
                indent(&mut output, depth - 1, flags);
            }
            output.push(if frame.items.is_object() { '}' } else { ']' });
            frames.pop();
        }
    }
    Ok(Value::String(output))
}

struct Frame<'a> {
    items: Items<'a>,
    first: bool,
}

enum Items<'a> {
    List {
        values: std::iter::Enumerate<std::slice::Iter<'a, Value>>,
        force_object: bool,
    },
    Map(serde_json::map::Iter<'a>),
}

enum Key<'a> {
    Index(usize),
    Text(&'a str),
}

impl<'a> Items<'a> {
    fn for_value(value: &'a Value, flags: u32) -> Option<Self> {
        match value {
            Value::Array(values) => Some(Self::List {
                values: values.iter().enumerate(),
                force_object: flags & Json::FORCE_OBJECT != 0,
            }),
            Value::Object(values) => Some(Self::Map(values.iter())),
            _ => None,
        }
    }

    fn is_object(&self) -> bool {
        matches!(
            self,
            Self::Map(_)
                | Self::List {
                    force_object: true,
                    ..
                }
        )
    }

    fn next(&mut self) -> Option<(Option<Key<'a>>, &'a Value)> {
        match self {
            Self::List {
                values,
                force_object,
            } => {
                let (index, value) = values.next()?;
                Some((force_object.then_some(Key::Index(index)), value))
            }
            Self::Map(values) => {
                let (key, value) = values.next()?;
                Some((Some(Key::Text(key)), value))
            }
        }
    }
}

fn invalid(flags: u32, code: &'static str, message: &'static str) -> Result<Value> {
    if flags & Json::THROW_ON_ERROR != 0 {
        return Err(Error::custom(code, message));
    }
    Ok(Value::Bool(false))
}

fn indent(output: &mut String, depth: usize, flags: u32) {
    if flags & Json::PRETTY_PRINT != 0 {
        output.push('\n');
        for _ in 0..depth {
            output.push_str("    ");
        }
    }
}

fn scalar(output: &mut String, value: &Value, flags: u32) -> Result<bool> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::String(value) => {
            let number = if flags & Json::NUMERIC_CHECK != 0 {
                InputNumber::numeric_string(value)
            } else {
                None
            };
            match number {
                Some(InputNumber::Integer(number)) => output.push_str(&number.to_string()),
                Some(InputNumber::Float(number)) if number.is_finite() => {
                    float(output, number, flags)?;
                }
                _ => string(output, value, flags),
            }
        }
        Value::Number(value) => {
            if let Some(number) = value.as_i64() {
                output.push_str(&number.to_string());
            } else if let Some(number) = value.as_f64().filter(|number| number.is_finite()) {
                float(output, number, flags)?;
            } else if flags & Json::PARTIAL_OUTPUT_ON_ERROR != 0 {
                output.push('0');
            } else {
                return Ok(false);
            }
        }
        Value::Array(_) | Value::Object(_) => return Ok(false),
    }
    Ok(true)
}

fn string(output: &mut String, value: &str, flags: u32) {
    output.push('"');
    for character in value.chars() {
        match character {
            '<' if flags & Json::HEX_TAG != 0 => output.push_str("\\u003C"),
            '>' if flags & Json::HEX_TAG != 0 => output.push_str("\\u003E"),
            '&' if flags & Json::HEX_AMP != 0 => output.push_str("\\u0026"),
            '\'' if flags & Json::HEX_APOS != 0 => output.push_str("\\u0027"),
            '"' if flags & Json::HEX_QUOT != 0 => output.push_str("\\u0022"),
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '/' if flags & Json::UNESCAPED_SLASHES == 0 => output.push_str("\\/"),
            '\u{0008}' => output.push_str("\\b"),
            '\u{000c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{0000}'..='\u{001f}' => {
                escape_unit(output, character.encode_utf16(&mut [0; 2])[0]);
            }
            _ if character.is_ascii()
                || (flags & Json::UNESCAPED_UNICODE != 0
                    && (flags & Json::UNESCAPED_LINE_TERMINATORS != 0
                        || !matches!(character, '\u{2028}' | '\u{2029}'))) =>
            {
                output.push(character);
            }
            _ => {
                for unit in character.encode_utf16(&mut [0; 2]) {
                    escape_unit(output, *unit);
                }
            }
        }
    }
    output.push('"');
}

fn escape_unit(output: &mut String, unit: u16) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push_str("\\u");
    for shift in [12, 8, 4, 0] {
        output.push(char::from(HEX[usize::from((unit >> shift) & 0x0f)]));
    }
}

fn float(output: &mut String, value: f64, flags: u32) -> Result<()> {
    // 固定 PHP 的 serialize_precision=-1：按十进制指数选择普通或科学写法。
    let text = value.abs().to_string();
    if value.is_sign_negative() {
        output.push('-');
    }
    if value == 0.0 {
        output.push_str(if flags & Json::PRESERVE_ZERO_FRACTION == 0 {
            "0"
        } else {
            "0.0"
        });
        return Ok(());
    }
    let decimal = text.find('.').unwrap_or(text.len());
    let mut digits: String = text.chars().filter(|character| *character != '.').collect();
    let leading = digits.len() - digits.trim_start_matches('0').len();
    let exponent = i32::try_from(decimal).map_err(Error::internal)?
        - i32::try_from(leading).map_err(Error::internal)?
        - 1;
    digits.drain(..leading);
    let digits = digits.trim_end_matches('0');
    if (-4..17).contains(&exponent) {
        output.push_str(&text);
        if decimal == text.len() && flags & Json::PRESERVE_ZERO_FRACTION != 0 {
            output.push_str(".0");
        }
    } else {
        output.push_str(&digits[..1]);
        output.push('.');
        if digits.len() == 1 {
            output.push('0');
        } else {
            output.push_str(&digits[1..]);
        }
        output.push('e');
        if exponent >= 0 {
            output.push('+');
        }
        output.push_str(&exponent.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encoder_flags_match_fixed_php_output_byte_for_byte() -> Result<()> {
        let golden: Value = serde_json::from_str(include_str!("json_encoding_golden.json"))
            .map_err(Error::internal)?;
        let cases = golden["flags"]
            .as_object()
            .ok_or_else(|| Error::custom("test_json_flags_missing", "缺少固定 PHP flags 用例"))?;
        let value = json!({
            "<>&'\"": "<>&'\"/中文😀\u{2028}\u{2029}\u{0000}\u{007f}",
            "numeric": ["0042"," +1.0 ","1e3","1e400","0x12","9223372036854775808","-0.0"],
            "float": [1.0,-0.0,1e16,1e17,1e-5,1e-4,1.2e17],
            "01": ["0","08"],
            "shape": [[], {}, [true,null]],
        });
        for (name, flags) in cases {
            let flags = flags
                .as_u64()
                .and_then(|flags| u32::try_from(flags).ok())
                .ok_or_else(|| Error::custom("test_json_flag_invalid", "固定 PHP flags 无效"))?;
            assert_eq!(
                Json::encode_default(&value, flags)?,
                golden["output"][name],
                "{name}"
            );
        }
        Ok(())
    }

    #[test]
    fn encoder_depth_and_partial_throw_precedence_match_fixed_php() -> Result<()> {
        let thread = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| -> Result<()> {
                for depth in [511, 512, 513, 600] {
                    let mut value = json!(0);
                    for _ in 0..depth {
                        value = Value::Array(vec![value]);
                    }
                    let normal = Json::encode_default(&value, 0);
                    let throwing = Json::encode_default(&value, Json::THROW_ON_ERROR);
                    let partial = Json::encode_default(
                        &value,
                        Json::PARTIAL_OUTPUT_ON_ERROR | Json::THROW_ON_ERROR,
                    );
                    // 深层输入的析构也不依赖测试线程的递归栈。
                    while let Value::Array(mut list) = value {
                        value = list.pop().unwrap_or(Value::Null);
                    }
                    let expected =
                        Value::String(format!("{}0{}", "[".repeat(depth), "]".repeat(depth)));
                    assert_eq!(partial?, expected);
                    if depth <= MAX_DEPTH {
                        assert_eq!(normal?, expected);
                        assert_eq!(throwing?, expected);
                    } else {
                        assert_eq!(normal?, Value::Bool(false));
                        assert!(matches!(throwing, Err(error) if error.code()=="model_json_depth"));
                    }
                }
                Ok(())
            })
            .map_err(Error::internal)?;
        thread
            .join()
            .map_err(|_| Error::custom("test_json_worker", "深层编码测试线程异常"))?
    }
}
