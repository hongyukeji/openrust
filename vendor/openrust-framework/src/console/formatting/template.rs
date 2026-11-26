use std::{collections::TryReserveError, error::Error, fmt, string::FromUtf8Error};
/// 单值格式模板的配置、编码与容量错误。
#[derive(Debug)]
pub enum ConsoleTemplateError {
    InvalidFormat(String),
    InvalidEncoding(FromUtf8Error),
    Allocation(TryReserveError),
}
impl fmt::Display for ConsoleTemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(message) => write!(f, "控制台格式模板无效：{message}"),
            Self::InvalidEncoding(error) => {
                write!(f, "控制台格式化结果不是有效的UTF-8文本：{error}")
            }
            Self::Allocation(error) => write!(f, "控制台格式化容量分配失败：{error}"),
        }
    }
}
impl Error for ConsoleTemplateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEncoding(error) => Some(error),
            Self::Allocation(error) => Some(error),
            _ => None,
        }
    }
}
fn configuration(message: &str) -> ConsoleTemplateError {
    ConsoleTemplateError::InvalidFormat(message.to_owned())
}
use serde_json::Value;

fn argument(value: &Value, index: usize) -> Result<&Value, ConsoleTemplateError> {
    if index == 1 {
        Ok(value)
    } else {
        Err(configuration(&format!(
            "错误模板需要第 {index} 个回答参数，当前仅提供一个"
        )))
    }
}
fn number(bytes: &[u8], cursor: &mut usize) -> Result<Option<usize>, ConsoleTemplateError> {
    let start = *cursor;
    let mut value = 0usize;
    while let Some(byte) = bytes.get(*cursor).filter(|byte| byte.is_ascii_digit()) {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(usize::from(*byte - b'0')))
            .ok_or_else(|| configuration("错误模板中的数值超出范围"))?;
        *cursor += 1;
    }
    Ok((*cursor > start).then_some(value))
}
fn reference(bytes: &[u8], cursor: &mut usize) -> Result<Option<usize>, ConsoleTemplateError> {
    let start = *cursor;
    let index = number(bytes, cursor)?;
    if index.is_some() && bytes.get(*cursor) == Some(&b'$') {
        *cursor += 1;
        Ok(index)
    } else {
        *cursor = start;
        Ok(None)
    }
}
fn parameter(
    bytes: &[u8],
    cursor: &mut usize,
    value: &Value,
    next: &mut usize,
) -> Result<usize, ConsoleTemplateError> {
    if bytes.get(*cursor) != Some(&b'*') {
        return Ok(number(bytes, cursor)?.unwrap_or(0));
    }
    *cursor += 1;
    let index = match reference(bytes, cursor)? {
        Some(index) => index,
        None => {
            let index = *next;
            *next += 1;
            index
        }
    };
    let integer = argument(value, index)?
        .as_i64()
        .ok_or_else(|| configuration("错误模板的动态宽度和精度必须为整数"))?;
    usize::try_from(integer).map_err(|_| configuration("错误模板的宽度和精度不能为负数"))
}
fn scientific(value: f64, precision: usize, upper: bool) -> (String, i32) {
    let text = format!("{value:.precision$e}");
    let (mantissa, exponent) = text.split_once('e').expect("标准科学计数包含指数");
    let exponent = exponent.parse::<i32>().expect("浮点指数在整数范围内");
    (
        format!("{mantissa}{}{exponent:+}", if upper { 'E' } else { 'e' }),
        exponent,
    )
}
fn trim_fraction(value: &str) -> &str {
    if value.contains('.') {
        value.trim_end_matches('0').trim_end_matches('.')
    } else {
        value
    }
}
fn float(value: &Value, kind: u8, precision: Option<usize>, plus: bool) -> String {
    let value = crate::support::input_scalars::float(value);
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-INF"
        } else {
            "INF"
        }
        .into();
    }
    if value.is_nan() {
        return "NaN".into();
    }
    let general = matches!(kind, b'g' | b'G' | b'h' | b'H');
    let negative = value < 0.0 || (general && value.is_sign_negative());
    let absolute = value.abs();
    let precision = precision.unwrap_or(6);
    let body = match kind {
        b'f' | b'F' => format!("{absolute:.precision$}"),
        b'e' | b'E' => scientific(absolute, precision, kind == b'E').0,
        _ => {
            let significant = precision.max(1);
            let (sci, exponent) =
                scientific(absolute, significant - 1, matches!(kind, b'G' | b'H'));
            if exponent < -4
                || usize::try_from(exponent).is_ok_and(|exponent| exponent >= significant)
            {
                let marker = if matches!(kind, b'G' | b'H') {
                    'E'
                } else {
                    'e'
                };
                let (mantissa, power) = sci.split_once(marker).expect("科学计数包含指数");
                let mantissa = trim_fraction(mantissa);
                let mantissa = if mantissa.contains('.') {
                    mantissa.to_owned()
                } else {
                    format!("{mantissa}.0")
                };
                format!("{mantissa}{marker}{power}")
            } else {
                let fraction = if exponent >= 0 {
                    significant.saturating_sub(usize::try_from(exponent).expect("非负指数") + 1)
                } else {
                    significant + usize::try_from(-exponent - 1).expect("小数位数非负")
                };
                let fixed = format!("{absolute:.fraction$}");
                trim_fraction(&fixed).to_owned()
            }
        }
    };
    if negative {
        format!("-{body}")
    } else if plus {
        format!("+{body}")
    } else {
        body
    }
}
fn padded(
    output: &mut Vec<u8>,
    content: &[u8],
    width: usize,
    mut pad: u8,
    left: bool,
    numeric: bool,
    signed: bool,
) -> Result<(), ConsoleTemplateError> {
    if left && numeric && pad == b'0' {
        pad = b' ';
    }
    let padding = width.saturating_sub(content.len());
    output
        .try_reserve(content.len().saturating_add(padding))
        .map_err(ConsoleTemplateError::Allocation)?;
    if left {
        output.extend_from_slice(content);
        output.resize(output.len() + padding, pad);
    } else if signed
        && pad == b'0'
        && content
            .first()
            .is_some_and(|first| matches!(first, b'+' | b'-'))
    {
        output.push(content[0]);
        output.resize(output.len() + padding, pad);
        output.extend_from_slice(&content[1..]);
    } else {
        output.resize(output.len() + padding, pad);
        output.extend_from_slice(content);
    }
    Ok(())
}
/// 此调用点只传一个回答参数；格式器保留显式位置、动态字段和标量转换。
pub(super) fn format(template: &str, value: &Value) -> Result<String, ConsoleTemplateError> {
    let bytes = template.as_bytes();
    let mut cursor = 0;
    let mut next = 1;
    let mut output = Vec::new();
    while cursor < bytes.len() {
        if bytes[cursor] != b'%' {
            output.push(bytes[cursor]);
            cursor += 1;
            continue;
        }
        cursor += 1;
        let index = reference(bytes, &mut cursor)?;
        let mut left = false;
        let mut plus = false;
        let mut pad = b' ';
        loop {
            match bytes.get(cursor) {
                Some(b'-') => left = true,
                Some(b'+') => plus = true,
                Some(b' ') => {}
                Some(b'0') => pad = b'0',
                Some(b'\'') => {
                    cursor += 1;
                    pad = *bytes
                        .get(cursor)
                        .ok_or_else(|| configuration("错误模板缺少填充字符"))?;
                }
                _ => break,
            }
            cursor += 1;
        }
        let width = parameter(bytes, &mut cursor, value, &mut next)?;
        let precision = if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            if bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'*')
            {
                Some(parameter(bytes, &mut cursor, value, &mut next)?)
            } else {
                None
            }
        } else {
            None
        };
        let kind = *bytes
            .get(cursor)
            .ok_or_else(|| configuration("错误模板缺少转换类型"))?;
        cursor += 1;
        if kind == b'%' {
            output.push(b'%');
            continue;
        }
        if !b"bcdeEfFgGhHosuxX".contains(&kind) {
            return Err(configuration(&format!(
                "错误模板在字节 {} 处包含未知转换类型：0x{kind:02X}",
                cursor - 1
            )));
        }
        let index = match index {
            Some(index) => index,
            None => {
                let index = next;
                next += 1;
                index
            }
        };
        let value = argument(value, index)?;
        let signed = kind == b'd' || b"eEfFgGhH".contains(&kind);
        let numeric = kind != b's';
        let content = match kind {
            b's' => {
                let text = crate::support::input_scalars::string(value.clone());
                let limit = precision.unwrap_or(text.len()).min(text.len());
                text.as_bytes()[..limit].to_vec()
            }
            b'c' => vec![crate::support::php_integer_cast(value).to_le_bytes()[0]],
            b'd' => {
                let integer = crate::support::php_integer_cast(value);
                if plus && integer >= 0 {
                    format!("+{integer}")
                } else {
                    integer.to_string()
                }
                .into_bytes()
            }
            b'b' | b'o' | b'u' | b'x' | b'X' => {
                let integer =
                    u64::from_ne_bytes(crate::support::php_integer_cast(value).to_ne_bytes());
                match kind {
                    b'b' => format!("{integer:b}"),
                    b'o' => format!("{integer:o}"),
                    b'x' => format!("{integer:x}"),
                    b'X' => format!("{integer:X}"),
                    _ => integer.to_string(),
                }
                .into_bytes()
            }
            _ => float(value, kind, precision, plus).into_bytes(),
        };
        if kind == b'c' {
            output.extend_from_slice(&content);
        } else {
            padded(&mut output, &content, width, pad, left, numeric, signed)?;
        }
    }
    String::from_utf8(output).map_err(ConsoleTemplateError::InvalidEncoding)
}
