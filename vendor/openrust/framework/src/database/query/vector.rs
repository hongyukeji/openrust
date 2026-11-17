use crate::database::{DatabaseError, DatabaseResult, Value};

use super::{Column, embeddings, grammars::Grammar};

pub(in crate::database) fn similarity_max_distance(similarity: f64) -> DatabaseResult<f64> {
    if !(0.0..=1.0).contains(&similarity) {
        return Err(DatabaseError::InvalidQuery(
            "vector similarity must be between 0 and 1",
        ));
    }
    Ok(1.0 - similarity)
}

pub(super) fn distance(
    grammar: &Grammar,
    column: &Column,
    vector: &[f64],
    bindings: &mut Vec<Value>,
) -> DatabaseResult<String> {
    if !embeddings::valid(vector) {
        return Err(DatabaseError::InvalidQuery(
            "vector must contain finite values",
        ));
    }
    let mut sql = String::new();
    column.compile(grammar, &mut sql, bindings, false)?;
    let sql = grammar.vector_distance(&sql)?;
    let vector = serde_json::to_string(vector)
        .map_err(|_| DatabaseError::InvalidQuery("vector parameter"))?;
    bindings.push(Value::Text(vector));
    Ok(sql)
}

pub(super) fn append_max_distance(
    sql: &mut String,
    bindings: &mut Vec<Value>,
    max_distance: f64,
) -> DatabaseResult<()> {
    validate_max_distance(max_distance)?;
    sql.push_str(" <= ?");
    bindings.push(Value::Real(max_distance));
    Ok(())
}

pub(in crate::database) fn validate_max_distance(max_distance: f64) -> DatabaseResult<()> {
    if !max_distance.is_finite() {
        return Err(DatabaseError::InvalidQuery(
            "vector distance must be finite",
        ));
    }
    Ok(())
}
