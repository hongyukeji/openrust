use std::fmt::Write as _;

use serde_json::Value as JsonValue;

use crate::database::{DatabaseError, DatabaseResult, Value, query::JsonOperand};

use super::{Dialect, Grammar};

impl Grammar {
    pub(in crate::database::query) fn json_contains(
        &self,
        selector: &str,
        value: &JsonValue,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        let (column, path) = self.json_selector(selector)?;
        let path_parameter = if path.is_some() { ", ?" } else { "" };
        match self.dialect {
            Dialect::MySql => {
                bindings.push(Value::Json(value.clone()));
                bindings.extend(path.map(Value::from));
                Ok(format!("json_contains({column}, ?{path_parameter})"))
            }
            Dialect::Postgres => Err(DatabaseError::UnsupportedOperation(
                "PostgreSQL JSON predicate",
            )),
            Dialect::Sqlite => {
                let value = sqlite_json_scalar(value)?;
                bindings.extend(path.map(Value::from));
                bindings.push(value);
                Ok(format!(
                    r#"exists (select 1 from json_each({column}{path_parameter}) where "json_each"."value" is ?)"#
                ))
            }
        }
    }

    pub(in crate::database::query) fn json_overlaps(
        &self,
        selector: &str,
        value: &JsonOperand,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        if !self.is_mysql() {
            return Err(DatabaseError::UnsupportedOperation("JSON overlaps"));
        }
        let (mut column, path) = self.json_selector(selector)?;
        if let Some(path) = path {
            bindings.push(path.into());
            column = format!("json_extract({column}, ?)");
        }
        let operand = match value {
            JsonOperand::Value(value) => {
                bindings.push(Value::Json(value.clone()));
                "?"
            }
            JsonOperand::Expression(expression) => {
                bindings.extend(expression.bindings.iter().cloned());
                &expression.sql
            }
        };
        Ok(format!("json_overlaps({column}, {operand})"))
    }

    pub(in crate::database::query) fn json_contains_key(
        &self,
        selector: &str,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        let (column, path) = self.json_selector(selector)?;
        match self.dialect {
            Dialect::MySql => {
                let path = path.ok_or(DatabaseError::InvalidQuery("JSON key path is required"))?;
                bindings.push(path.into());
                Ok(format!("ifnull(json_contains_path({column}, 'one', ?), 0)"))
            }
            Dialect::Postgres => Err(DatabaseError::UnsupportedOperation(
                "PostgreSQL JSON predicate",
            )),
            Dialect::Sqlite => {
                let path_parameter = if path.is_some() { ", ?" } else { "" };
                bindings.extend(path.map(Value::from));
                Ok(format!("json_type({column}{path_parameter}) is not null"))
            }
        }
    }

    pub(in crate::database::query) fn json_length(
        &self,
        selector: &str,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        let (column, path) = self.json_selector(selector)?;
        let path_parameter = if path.is_some() { ", ?" } else { "" };
        let function = match self.dialect {
            Dialect::MySql => "json_length",
            Dialect::Sqlite => "json_array_length",
            Dialect::Postgres => {
                return Err(DatabaseError::UnsupportedOperation(
                    "PostgreSQL JSON length",
                ));
            }
        };
        bindings.extend(path.map(Value::from));
        Ok(format!("{function}({column}{path_parameter})"))
    }

    pub(in crate::database) fn json_selector(
        &self,
        selector: &str,
    ) -> DatabaseResult<(String, Option<String>)> {
        let (column, path) = selector
            .split_once("->")
            .map_or((selector, None), |(column, path)| (column, Some(path)));
        Ok((self.column(column)?, path.map(json_path).transpose()?))
    }
}

fn sqlite_json_scalar(value: &JsonValue) -> DatabaseResult<Value> {
    Ok(match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::String(value) => value.clone().into(),
        JsonValue::Number(value) => match value.as_i64() {
            Some(integer) => integer.into(),
            None => Value::Real(
                value
                    .as_f64()
                    .ok_or(DatabaseError::InvalidQuery("SQLite JSON numeric value"))?,
            ),
        },
        _ => {
            return Err(DatabaseError::InvalidQuery(
                "SQLite JSON containment requires a scalar",
            ));
        }
    })
}

fn json_path(path: &str) -> DatabaseResult<String> {
    if path.contains('\0') {
        return Err(DatabaseError::InvalidQuery("JSON path"));
    }
    let mut result = String::from("$");
    for segment in path.split("->") {
        let (key, indices) = path_segment(segment);
        if !key.is_empty() || indices.is_empty() {
            // JSON-escape each key, then bind the complete path as an SQL value.
            let key = JsonValue::String(key.to_owned());
            write!(result, ".{key}").expect("formatting a string is infallible");
        }
        result.push_str(indices);
    }
    Ok(result)
}

fn path_segment(segment: &str) -> (&str, &str) {
    for (start, _) in segment.match_indices('[') {
        let mut rest = &segment[start..];
        while let Some(index) = rest.strip_prefix('[') {
            let Some(end) = index.find(']') else {
                break;
            };
            if end == 0 {
                break;
            }
            rest = &index[end + 1..];
        }
        if rest.is_empty() {
            return (&segment[..start], &segment[start..]);
        }
    }
    (segment, "")
}
