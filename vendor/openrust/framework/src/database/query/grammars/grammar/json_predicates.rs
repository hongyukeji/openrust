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
            Dialect::SqlServer => {
                bindings.push(json_scalar(value, self.dialect)?);
                let rows = self.sql_server_json_rows(selector, bindings)?;
                Ok(format!("? in (select [value] from {rows})"))
            }
            Dialect::Sqlite => {
                let (column, path) = self.json_selector(selector)?;
                let path_parameter = if path.is_some() { ", ?" } else { "" };
                let value = json_scalar(value, self.dialect)?;
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
            Dialect::SqlServer => {
                let (parent, key) = sql_server_json_key(selector)?;
                bindings.push(key.into());
                let rows = self.sql_server_json_rows(parent, bindings)?;
                Ok(format!("? in (select [key] from {rows})"))
            }
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
                let rows = self.sql_server_json_rows(selector, bindings)?;
                return Ok(format!("(select count(*) from {rows})"));
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

    fn sql_server_json_rows(
        &self,
        selector: &str,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        let (column, path) = self.json_selector(selector)?;
        let path_parameter = if path.is_some() { ", ?" } else { "" };
        bindings.extend(path.map(Value::from));
        Ok(format!("openjson({column}{path_parameter})"))
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
        let attributes = Self::postgres_json_path(path)?;
        let mut expression = self.column(column)?;
        let last = attributes.len() - 1;
        for (index, attribute) in attributes.into_iter().enumerate() {
            expression.push_str(if text && index == last { "->>" } else { "->" });
            expression.push_str(&postgres_json_attribute(attribute));
        }
        Ok(expression)
    }

    pub(in crate::database::query) fn postgres_json_path(path: &str) -> DatabaseResult<Vec<&str>> {
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
        Ok(attributes)
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

fn sql_server_json_key(selector: &str) -> DatabaseResult<(&str, &str)> {
    let (parent, key) = selector
        .rsplit_once("->")
        .ok_or(DatabaseError::InvalidQuery("JSON key path is required"))?;
    if key.contains('\0') {
        return Err(DatabaseError::InvalidQuery("JSON path"));
    }
    if let Some((stem, index)) = key.strip_suffix(']').and_then(|key| key.rsplit_once('['))
        && !index.is_empty()
        && index.bytes().all(|byte| byte.is_ascii_digit())
    {
        let parent = if stem.is_empty() {
            parent
        } else {
            &selector[..parent.len() + 2 + stem.len()]
        };
        let index = index.trim_start_matches('0');
        return Ok((parent, if index.is_empty() { "0" } else { index }));
    }
    Ok((parent, key))
}

fn json_scalar(value: &JsonValue, dialect: Dialect) -> DatabaseResult<Value> {
    Ok(match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) if dialect == Dialect::SqlServer => value.to_string().into(),
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::String(value) => value.clone().into(),
        JsonValue::Number(value) => match value.as_i64() {
            Some(integer) => integer.into(),
            None if dialect == Dialect::SqlServer && value.is_u64() => {
                Value::Unsigned(value.as_u64().expect("unsigned JSON number"))
            }
            None => Value::Real(
                value
                    .as_f64()
                    .ok_or(DatabaseError::InvalidQuery("JSON numeric value"))?,
            ),
        },
        _ => {
            return Err(DatabaseError::InvalidQuery(
                if dialect == Dialect::SqlServer {
                    "SQL Server JSON containment requires a scalar"
                } else {
                    "SQLite JSON containment requires a scalar"
                },
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
