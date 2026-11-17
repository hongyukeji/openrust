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
        match self.dialect {
            Dialect::MySql => {
                let (column, path) = self.json_selector(selector)?;
                let path_parameter = if path.is_some() { ", ?" } else { "" };
                bindings.push(Value::Json(value.clone()));
                bindings.extend(path.map(Value::from));
                Ok(format!("json_contains({column}, ?{path_parameter})"))
            }
            Dialect::Postgres => {
                let document = self.postgres_json_document(selector)?;
                bindings.push(Value::Json(value.clone()));
                Ok(format!("{document} @> ?::jsonb"))
            }
            Dialect::SqlServer => Err(DatabaseError::UnsupportedOperation(
                "SQL Server JSON predicate",
            )),
            Dialect::Sqlite => {
                let (column, path) = self.json_selector(selector)?;
                let path_parameter = if path.is_some() { ", ?" } else { "" };
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
        match self.dialect {
            Dialect::MySql => {
                let (column, path) = self.json_selector(selector)?;
                let path = path.ok_or(DatabaseError::InvalidQuery("JSON key path is required"))?;
                bindings.push(path.into());
                Ok(format!("ifnull(json_contains_path({column}, 'one', ?), 0)"))
            }
            Dialect::Postgres => self.postgres_json_contains_key(selector, bindings),
            Dialect::SqlServer => Err(DatabaseError::UnsupportedOperation(
                "SQL Server JSON predicate",
            )),
            Dialect::Sqlite => {
                let (column, path) = self.json_selector(selector)?;
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
        let function = match self.dialect {
            Dialect::MySql => "json_length",
            Dialect::Sqlite => "json_array_length",
            Dialect::Postgres => {
                let document = self.postgres_json_document(selector)?;
                return Ok(format!("jsonb_array_length({document})"));
            }
            Dialect::SqlServer => {
                return Err(DatabaseError::UnsupportedOperation(
                    "SQL Server JSON length",
                ));
            }
        };
        let (column, path) = self.json_selector(selector)?;
        let path_parameter = if path.is_some() { ", ?" } else { "" };
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

    pub(in crate::database) fn postgres_json_selector(
        &self,
        selector: &str,
    ) -> DatabaseResult<String> {
        self.postgres_json_expression(selector, true)
    }

    fn postgres_json_document(&self, selector: &str) -> DatabaseResult<String> {
        let expression = self.postgres_json_expression(selector, false)?;
        Ok(format!("({expression})::jsonb"))
    }

    fn postgres_json_expression(&self, selector: &str, text: bool) -> DatabaseResult<String> {
        let Some((column, path)) = selector.split_once("->") else {
            return self.column(selector);
        };
        if path.contains('\0') {
            return Err(DatabaseError::InvalidQuery("JSON path"));
        }
        let mut attributes = Vec::new();
        for segment in path.split("->") {
            let (key, indices) = path_segment(segment);
            if !key.is_empty() || indices.is_empty() {
                attributes.push(key);
            }
            attributes.extend(indices.split_terminator(']').map(|index| &index[1..]));
        }
        let mut expression = self.column(column)?;
        let last = attributes.len() - 1;
        for (index, attribute) in attributes.into_iter().enumerate() {
            expression.push_str(if text && index == last { "->>" } else { "->" });
            expression.push_str(&postgres_json_attribute(attribute));
        }
        Ok(expression)
    }

    fn postgres_json_contains_key(
        &self,
        selector: &str,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        let (parent, key) = selector
            .rsplit_once("->")
            .ok_or(DatabaseError::InvalidQuery("JSON key path is required"))?;
        if key.contains('\0') {
            return Err(DatabaseError::InvalidQuery("JSON path"));
        }
        let array = postgres_json_index(key)
            .map(|index| (parent, index.unsigned_abs() + u64::from(index >= 0)))
            .or_else(|| {
                let (stem, bound) = postgres_bracket_bound(key)?;
                let parent = if stem.is_empty() {
                    parent
                } else {
                    &selector[..parent.len() + 2 + stem.len()]
                };
                Some((parent, bound))
            });
        if let Some((parent, bound)) = array {
            let document = self.postgres_json_document(parent)?;
            let bound = bound.min(i32::MAX as u64 + 1);
            return Ok(format!(
                "case when jsonb_typeof({document}) = 'array' then jsonb_array_length({document}) >= {bound} else false end"
            ));
        }
        let document = self.postgres_json_document(parent)?;
        bindings.push(key.into());
        // Double the operator's question mark so bind rewriting preserves it.
        Ok(format!("coalesce({document} ?? ?, false)"))
    }
}

fn postgres_json_index(attribute: &str) -> Option<i64> {
    let value = attribute.trim_matches([' ', '\t', '\n', '\r', '\u{b}']);
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits != "0" && digits.starts_with('0') {
        return None;
    }
    value.parse().ok()
}

fn postgres_json_attribute(attribute: &str) -> String {
    if let Some(index) = postgres_json_index(attribute) {
        return index.to_string();
    }
    format!("E'{}'", attribute.replace('\\', "\\\\").replace('\'', "''"))
}

fn postgres_bracket_bound(key: &str) -> Option<(&str, u64)> {
    let (stem, index) = key
        .strip_suffix(']')
        .or_else(|| key.strip_suffix("]\n"))?
        .rsplit_once('[')?;
    let digits = index.strip_prefix('-').unwrap_or(index);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.parse::<u64>().unwrap_or(u64::MAX);
    Some((
        stem,
        magnitude.saturating_add(u64::from(!index.starts_with('-') || magnitude == 0)),
    ))
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
