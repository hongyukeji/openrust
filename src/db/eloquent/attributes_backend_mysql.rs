//! MySQL 保留 PDO 数值/文本类别；使用同次结果的协议元数据，不猜 BOOLEAN 或 ZEROFILL。

use super::{Map, Result, Value, bytes, finite, invalid, unsupported};
use sea_orm::sqlx::{
    Column, MySql, Row, Type, ValueRef,
    mysql::{MySqlColumn, MySqlRow, MySqlValueFormat},
};
use std::fmt::Write;

pub(super) fn attributes(row: &MySqlRow) -> Result<Map<String, Value>> {
    let mut attributes = Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        let raw = row.try_get_raw(index).map_err(|_| invalid())?;
        // SQLx 把零日期也视为 null；直接 Decode<Vec<u8>> 可区分其有效 payload 与 SQL NULL。
        let payload = row.try_get_unchecked::<Vec<u8>, _>(index);
        let value = if raw.is_null() && payload.is_err() {
            Value::Null
        } else {
            let payload = payload.map_err(|_| invalid())?;
            decode(row, index, column, raw.format(), &payload)?
        };
        attributes.insert(column.name().to_owned(), value);
    }
    Ok(attributes)
}

fn decode(
    row: &MySqlRow,
    index: usize,
    column: &MySqlColumn,
    format: MySqlValueFormat,
    payload: &[u8],
) -> Result<Value> {
    let info = column.type_info();
    match info.wire_type_id() {
        1..=3 | 8 | 9 | 13 => {
            let value = if info.is_unsigned() {
                unsigned(
                    row.try_get_unchecked::<u64, _>(index)
                        .map_err(|_| invalid())?,
                )
            } else {
                Value::from(
                    row.try_get_unchecked::<i64, _>(index)
                        .map_err(|_| invalid())?,
                )
            };
            if info.is_zerofill() {
                zerofill(value, info.display_width().ok_or_else(unsupported)?)
            } else {
                Ok(value)
            }
        }
        4 => {
            if matches!(format, MySqlValueFormat::Text) {
                finite(
                    std::str::from_utf8(payload)
                        .map_err(|_| invalid())?
                        .parse()
                        .map_err(|_| invalid())?,
                )
            } else {
                mysql_float(
                    row.try_get_unchecked::<f32, _>(index)
                        .map_err(|_| invalid())?,
                    column.fractional_digits().ok_or_else(unsupported)?,
                )
            }
        }
        5 => finite(
            row.try_get_unchecked::<f64, _>(index)
                .map_err(|_| invalid())?,
        ),
        0 | 245..=248 => text(payload),
        7 | 10 | 11 | 12 => temporal(
            payload,
            info.wire_type_id(),
            column.fractional_digits().ok_or_else(unsupported)?,
            matches!(format, MySqlValueFormat::Binary),
        )
        .map(Value::from),
        16 => {
            if payload.len() > 8 {
                return Err(invalid());
            }
            let value = payload
                .iter()
                .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte));
            Ok(unsigned(value))
        }
        15 | 249..=254 => {
            // *_bin 排序规则仍可传 UTF-8 文本；不能只因 BINARY_FLAG 或名称就变为字节数组。
            if <String as Type<MySql>>::compatible(info) {
                text(payload)
            } else {
                Ok(bytes(payload.to_vec()))
            }
        }
        255 => Ok(bytes(payload.to_vec())),
        _ => Err(unsupported()),
    }
}

pub(super) fn unsigned(value: u64) -> Value {
    i64::try_from(value).map_or_else(|_| Value::from(value.to_string()), Value::from)
}

fn text(payload: &[u8]) -> Result<Value> {
    std::str::from_utf8(payload)
        .map(|value| Value::from(value.to_owned()))
        .map_err(|_| invalid())
}

fn zerofill(value: Value, width: u32) -> Result<Value> {
    // MySQL 数值显示宽度上限 255；异常元数据不得触发无界分配。
    if width > 255 {
        return Err(invalid());
    }
    let value = match value {
        Value::String(value) => value,
        Value::Number(value) => value.to_string(),
        _ => return Err(invalid()),
    };
    Ok(Value::from(format!(
        "{value:0>width$}",
        width = usize::try_from(width).map_err(|_| invalid())?
    )))
}

pub(super) fn mysql_float(value: f32, decimals: u8) -> Result<Value> {
    if !value.is_finite() {
        return finite(f64::from(value));
    }
    // mysqlnd mysql_float_to_double：普通 FLOAT 先按 FLT_DIG=6 位有效数字转文本；
    // 显式小数位则先按 decimals 格式化，再读取 double，而非直接扩大 f32 的二进制误差。
    let text = if decimals >= 31 {
        format!("{value:.5e}")
    } else {
        format!("{value:.precision$}", precision = usize::from(decimals))
    };
    finite(text.parse().map_err(|_| invalid())?)
}

pub(super) fn temporal(payload: &[u8], kind: u8, scale: u8, binary: bool) -> Result<String> {
    if !binary {
        return std::str::from_utf8(payload)
            .map(str::to_owned)
            .map_err(|_| invalid());
    }
    if scale > 6 || payload.is_empty() || usize::from(payload[0]) + 1 != payload.len() {
        return Err(invalid());
    }
    let data = &payload[1..];
    if kind == 11 {
        return time(data, scale);
    }
    if !matches!(data.len(), 0 | 4 | 7 | 11) || (kind == 10 && !matches!(data.len(), 0 | 4)) {
        return Err(invalid());
    }
    let mut year = 0;
    let (mut month, mut day, mut hour, mut minute, mut second, mut micros) = (0, 0, 0, 0, 0, 0);
    if data.len() >= 4 {
        year = u16::from_le_bytes([data[0], data[1]]);
        month = data[2];
        day = data[3];
    }
    if data.len() >= 7 {
        hour = data[4];
        minute = data[5];
        second = data[6];
    }
    if data.len() == 11 {
        micros = u32::from_le_bytes(data[7..11].try_into().map_err(|_| invalid())?);
    }
    if year > 9999 || month > 12 || day > 31 || hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }
    let mut text = format!("{year:04}-{month:02}-{day:02}");
    if kind != 10 {
        write!(text, " {hour:02}:{minute:02}:{second:02}").map_err(|_| invalid())?;
        fraction(&mut text, micros, scale)?;
    }
    Ok(text)
}

fn time(data: &[u8], scale: u8) -> Result<String> {
    if !matches!(data.len(), 0 | 8 | 12) {
        return Err(invalid());
    }
    let (mut negative, mut hours, mut minutes, mut seconds, mut micros) = (false, 0_u32, 0, 0, 0);
    if data.len() >= 8 {
        if data[0] > 1 || data[5] > 23 || data[6] > 59 || data[7] > 59 {
            return Err(invalid());
        }
        negative = data[0] == 1;
        hours = u32::from_le_bytes(data[1..5].try_into().map_err(|_| invalid())?)
            .checked_mul(24)
            .and_then(|days| days.checked_add(u32::from(data[5])))
            .ok_or_else(invalid)?;
        minutes = data[6];
        seconds = data[7];
    }
    if data.len() == 12 {
        micros = u32::from_le_bytes(data[8..12].try_into().map_err(|_| invalid())?);
    }
    if hours > 838 {
        return Err(invalid());
    }
    let mut text = format!(
        "{}{hours:02}:{minutes:02}:{seconds:02}",
        if negative { "-" } else { "" }
    );
    fraction(&mut text, micros, scale)?;
    Ok(text)
}

fn fraction(text: &mut String, micros: u32, scale: u8) -> Result<()> {
    if micros >= 1_000_000 || !micros.is_multiple_of(10_u32.pow(u32::from(6 - scale))) {
        return Err(invalid());
    }
    if scale != 0 {
        text.push('.');
        let value = format!("{micros:06}");
        text.push_str(&value[..usize::from(scale)]);
    }
    Ok(())
}
