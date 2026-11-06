//! PostgreSQL 原始属性；NUMERIC 直接解释同次结果的 base-10000 wire，不经 Decimal。

use super::{Map, Result, Value, bytes, finite, invalid, unsupported};
use chrono::Datelike;
use sea_orm::sqlx::{
    Column, Row, TypeInfo, ValueRef,
    postgres::{PgRow, PgTypeInfo, PgTypeKind, PgValueFormat, PgValueRef, types::Oid},
};
use std::fmt::Write;

pub(super) fn attributes(row: &PgRow) -> Result<Map<String, Value>> {
    let mut attributes = Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        let raw = row.try_get_raw(index).map_err(|_| invalid())?;
        let value = if raw.is_null() {
            Value::Null
        } else {
            decode(row, index, &raw, column.type_info())?
        };
        attributes.insert(column.name().to_owned(), value);
    }
    Ok(attributes)
}

fn decode(row: &PgRow, index: usize, raw: &PgValueRef<'_>, info: &PgTypeInfo) -> Result<Value> {
    let info = base_type(info);
    macro_rules! native {
        ($type:ty) => {
            row.try_get_unchecked::<$type, _>(index)
                .map(Value::from)
                .map_err(|_| invalid())
        };
    }
    match info.name() {
        "BOOL" => native!(bool),
        "INT2" => native!(i16),
        "INT4" => native!(i32),
        "INT8" => native!(i64),
        "OID" => row
            .try_get_unchecked::<Oid, _>(index)
            .map(|value| Value::from(value.0))
            .map_err(|_| invalid()),
        "FLOAT4" => finite(f64::from(
            row.try_get_unchecked::<f32, _>(index)
                .map_err(|_| invalid())?,
        )),
        "FLOAT8" => finite(
            row.try_get_unchecked::<f64, _>(index)
                .map_err(|_| invalid())?,
        ),
        "NUMERIC" => match raw.format() {
            PgValueFormat::Text => text(raw.as_bytes().map_err(|_| invalid())?),
            PgValueFormat::Binary => {
                numeric(raw.as_bytes().map_err(|_| invalid())?).map(Value::from)
            }
        },
        "JSON" => text(raw.as_bytes().map_err(|_| invalid())?),
        "JSONB" => {
            let payload = raw.as_bytes().map_err(|_| invalid())?;
            match raw.format() {
                PgValueFormat::Text => text(payload),
                PgValueFormat::Binary if payload.first() == Some(&1) => text(&payload[1..]),
                PgValueFormat::Binary => Err(invalid()),
            }
        }
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" | "UNKNOWN" | "CITEXT" | "CHAR" => {
            text(raw.as_bytes().map_err(|_| invalid())?)
        }
        "UUID" => row
            .try_get_unchecked::<sea_orm::prelude::Uuid, _>(index)
            .map(|value| Value::from(value.to_string()))
            .map_err(|_| invalid()),
        "BYTEA" => row
            .try_get_unchecked::<Vec<u8>, _>(index)
            .map(bytes)
            .map_err(|_| invalid()),
        "DATE" | "TIME" | "TIMESTAMP" | "TIMESTAMPTZ" => temporal(raw, info.name()),
        _ if matches!(info.kind(), PgTypeKind::Enum(_)) => {
            text(raw.as_bytes().map_err(|_| invalid())?)
        }
        _ => Err(unsupported()),
    }
}

fn base_type(mut info: &PgTypeInfo) -> &PgTypeInfo {
    while let PgTypeKind::Domain(base) = info.kind() {
        info = base;
    }
    info
}

fn text(value: &[u8]) -> Result<Value> {
    std::str::from_utf8(value)
        .map(|value| Value::from(value.to_owned()))
        .map_err(|_| invalid())
}

fn temporal(raw: &PgValueRef<'_>, kind: &str) -> Result<Value> {
    if raw.format() == PgValueFormat::Text {
        return text(raw.as_bytes().map_err(|_| invalid())?);
    }
    let payload = raw.as_bytes().map_err(|_| invalid())?;
    let epoch = chrono::NaiveDate::from_ymd_opt(2000, 1, 1).ok_or_else(invalid)?;
    let value = if kind == "DATE" {
        let days = i32::from_be_bytes(payload.try_into().map_err(|_| invalid())?);
        match days {
            i32::MAX => "infinity".to_owned(),
            i32::MIN => "-infinity".to_owned(),
            days => {
                let date = epoch
                    .checked_add_signed(chrono::TimeDelta::days(i64::from(days)))
                    .ok_or_else(unsupported)?;
                let (mut text, bc) = format_date(date);
                if bc {
                    text.push_str(" BC");
                }
                text
            }
        }
    } else {
        let micros = i64::from_be_bytes(payload.try_into().map_err(|_| invalid())?);
        if kind == "TIME" {
            if !(0..=86_400_000_000).contains(&micros) {
                return Err(invalid());
            }
            format_time(micros)
        } else {
            match micros {
                i64::MAX => "infinity".to_owned(),
                i64::MIN => "-infinity".to_owned(),
                micros => {
                    let timestamp = epoch
                        .and_hms_opt(0, 0, 0)
                        .ok_or_else(invalid)?
                        .checked_add_signed(chrono::TimeDelta::microseconds(micros))
                        .ok_or_else(unsupported)?;
                    let (mut text, bc) = format_date(timestamp.date());
                    write!(text, " {}", timestamp.format("%H:%M:%S")).map_err(|_| invalid())?;
                    let fraction = timestamp.and_utc().timestamp_subsec_micros();
                    if fraction != 0 {
                        text.push('.');
                        text.push_str(format!("{fraction:06}").trim_end_matches('0'));
                    }
                    if kind == "TIMESTAMPTZ" {
                        // 二进制只含绝对瞬间；沿用 Rust 驱动 UTC，不猜会话时区。
                        text.push_str("+00");
                    }
                    if bc {
                        text.push_str(" BC");
                    }
                    text
                }
            }
        }
    };
    Ok(Value::from(value))
}

fn format_date(date: chrono::NaiveDate) -> (String, bool) {
    let bc = date.year() <= 0;
    let year = if bc { 1 - date.year() } else { date.year() };
    (
        format!("{year:04}-{:02}-{:02}", date.month(), date.day()),
        bc,
    )
}

fn format_time(micros: i64) -> String {
    let seconds = micros / 1_000_000;
    let fraction = micros % 1_000_000;
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if fraction != 0 {
        text.push('.');
        text.push_str(format!("{fraction:06}").trim_end_matches('0'));
    }
    text
}

use super::numeric::numeric;
