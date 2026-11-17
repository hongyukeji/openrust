use std::{collections::BTreeMap, fmt::Write as _};

use serde_json::{Map, Value as Json};

use crate::database::{
    DatabaseError, DatabaseResult, Value,
    query::{ValueOperand, grammars::Grammar},
};

pub(super) fn compile_assignments<V: Into<ValueOperand>>(
    grammar: &Grammar,
    values: BTreeMap<String, V>,
    target: Option<&str>,
) -> DatabaseResult<(Vec<String>, Vec<Value>)> {
    let mut assignments = Vec::new();
    let mut bindings = Vec::new();
    let mut json = BTreeMap::<String, Vec<(String, ValueOperand)>>::new();
    for (column, value) in values {
        let value = value.into();
        if let Some((field, path)) = column.split_once("->") {
            if grammar.is_sql_server() {
                return Err(DatabaseError::UnsupportedOperation(
                    "SQL Server JSON path update",
                ));
            }
            if field.is_empty() || path.split("->").any(str::is_empty) {
                return Err(DatabaseError::InvalidQuery("JSON update path"));
            }
            let field = if grammar.is_postgres() {
                unqualified(field)
            } else {
                field
            };
            json.entry(field.to_owned())
                .or_default()
                .push((path.to_owned(), value));
        } else {
            let column = write_column(grammar, &column)?;
            match value {
                ValueOperand::Value(value) => {
                    assignments.push(format!("{column} = ?"));
                    bindings.push(value);
                }
                ValueOperand::Expression(expression) => {
                    assignments.push(format!("{column} = {}", expression.sql));
                    bindings.extend(expression.bindings);
                }
            }
        }
    }

    for (field, changes) in json {
        let column = write_column(grammar, &field)?;
        if grammar.is_mysql() {
            let pair = if grammar.is_mariadb() {
                "?, json_extract(?, '$')"
            } else {
                "?, cast(? as json)"
            };
            let pairs = vec![pair; changes.len()].join(", ");
            assignments.push(format!("{column} = json_set({column}, {pairs})"));
            for (path, value) in changes {
                bindings.push(mysql_path(&path)?.into());
                bindings.push(json_value(value)?.to_string().into());
            }
        } else if grammar.is_postgres() {
            assignments.push(postgres_assignment(
                &column,
                target,
                changes,
                &mut bindings,
            )?);
        } else {
            assignments.push(format!(
                "{column} = json_patch(ifnull({column}, json('{{}}')), json(?))"
            ));
            let mut patch = Json::Object(Map::new());
            for (path, value) in changes {
                let segments: Vec<_> = path.split("->").flat_map(|key| key.split('.')).collect();
                set_patch(&mut patch, &segments, json_value(value)?)?;
            }
            bindings.push(patch.to_string().into());
        }
    }
    Ok((assignments, bindings))
}

fn postgres_assignment(
    column: &str,
    target: Option<&str>,
    changes: Vec<(String, ValueOperand)>,
    bindings: &mut Vec<Value>,
) -> DatabaseResult<String> {
    let mut sql = format!("{column} = {}", "jsonb_set(".repeat(changes.len()));
    if let Some(target) = target {
        sql.push_str(target);
        sql.push('.');
    }
    sql.push_str(column);
    sql.push_str("::jsonb");
    for (path, value) in changes {
        let path = Grammar::postgres_json_path(&path)?;
        let placeholders = vec!["?"; path.len()].join(", ");
        write!(sql, ", array[{placeholders}]::text[], ")
            .expect("writing to a String is infallible");
        bindings.extend(path.into_iter().map(Value::from));
        match value {
            ValueOperand::Expression(expression) => {
                write!(sql, "({})::jsonb)", expression.sql)
                    .expect("writing to a String is infallible");
                bindings.extend(expression.bindings);
            }
            value => {
                sql.push_str("?::jsonb)");
                bindings.push(Value::Json(json_value(value)?));
            }
        }
    }
    Ok(sql)
}

pub(super) fn write_column(grammar: &Grammar, column: &str) -> DatabaseResult<String> {
    let column = if grammar.is_mysql() {
        column
    } else {
        unqualified(column)
    };
    grammar.column(column)
}

fn unqualified(column: &str) -> &str {
    column.rsplit_once('.').map_or(column, |(_, field)| field)
}

fn json_value(value: ValueOperand) -> DatabaseResult<Json> {
    let ValueOperand::Value(value) = value else {
        return Err(DatabaseError::InvalidQuery("JSON update value"));
    };
    Ok(match value {
        Value::Null => Json::Null,
        Value::Bool(value) => Json::Bool(value),
        Value::Integer(value) => value.into(),
        Value::Unsigned(value) => value.into(),
        Value::Real(value) => serde_json::Number::from_f64(value)
            .map(Json::Number)
            .ok_or(DatabaseError::InvalidQuery("JSON update number"))?,
        Value::Text(value) => value.into(),
        Value::Json(value) => value,
        Value::Bytes(_) => return Err(DatabaseError::InvalidQuery("JSON update value")),
    })
}

fn set_patch(mut target: &mut Json, path: &[&str], value: Json) -> DatabaseResult<()> {
    for key in path {
        if key.is_empty() {
            return Err(DatabaseError::InvalidQuery("JSON update path"));
        }
        if !target.is_object() {
            *target = Json::Object(Map::new());
        }
        target = target
            .as_object_mut()
            .expect("JSON object")
            .entry(*key)
            .or_insert(Json::Null);
    }
    *target = value;
    Ok(())
}

fn mysql_path(path: &str) -> DatabaseResult<String> {
    let mut result = "$".to_owned();
    for segment in path.split("->") {
        let (key, mut indexes) = segment
            .find('[')
            .map_or((segment, ""), |index| segment.split_at(index));
        if !key.is_empty() {
            result.push('.');
            result.push_str(&Json::from(key).to_string());
        }
        while let Some(indexes_start) = indexes.strip_prefix('[') {
            let (index, next) = indexes_start
                .split_once(']')
                .ok_or(DatabaseError::InvalidQuery("JSON array index"))?;
            if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(DatabaseError::InvalidQuery("JSON array index"));
            }
            result.push('[');
            result.push_str(index);
            result.push(']');
            indexes = next;
        }
        if !indexes.is_empty() {
            return Err(DatabaseError::InvalidQuery("JSON array index"));
        }
    }
    Ok(result)
}
