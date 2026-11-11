use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};

use crate::database::{
    DatabaseError, DatabaseResult, Value,
    query::{Record, grammars::Grammar},
};

pub(super) fn compile_assignments(
    grammar: &Grammar,
    values: Record,
) -> DatabaseResult<(Vec<String>, Vec<Value>)> {
    let mut assignments = Vec::new();
    let mut bindings = Vec::new();
    let mut json = BTreeMap::<String, Vec<(String, Json)>>::new();
    for (column, value) in values {
        if let Some((field, path)) = column.split_once("->") {
            if field.is_empty() || path.split("->").any(str::is_empty) {
                return Err(DatabaseError::InvalidQuery("JSON update path"));
            }
            json.entry(field.to_owned())
                .or_default()
                .push((path.to_owned(), json_value(value)?));
        } else {
            let column = write_column(grammar, &column)?;
            assignments.push(format!("{column} = ?"));
            bindings.push(value);
        }
    }

    for (field, changes) in json {
        let column = write_column(grammar, &field)?;
        if grammar.is_mysql() {
            let pairs = vec!["?, cast(? as json)"; changes.len()].join(", ");
            assignments.push(format!("{column} = json_set({column}, {pairs})"));
            for (path, value) in changes {
                bindings.push(mysql_path(&path)?.into());
                bindings.push(value.to_string().into());
            }
        } else {
            assignments.push(format!(
                "{column} = json_patch(ifnull({column}, json('{{}}')), json(?))"
            ));
            let mut patch = Json::Object(Map::new());
            for (path, value) in changes {
                let segments: Vec<_> = path.split("->").flat_map(|key| key.split('.')).collect();
                set_patch(&mut patch, &segments, value)?;
            }
            bindings.push(patch.to_string().into());
        }
    }
    Ok((assignments, bindings))
}

fn write_column(grammar: &Grammar, column: &str) -> DatabaseResult<String> {
    let column = if grammar.is_mysql() {
        column
    } else {
        column.rsplit('.').next().expect("at least one segment")
    };
    grammar.column(column)
}

fn json_value(value: Value) -> DatabaseResult<Json> {
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
