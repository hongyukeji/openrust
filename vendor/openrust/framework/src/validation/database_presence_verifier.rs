use std::{collections::HashSet, sync::Arc};

use crate::database::{
    ConnectionResolverInterface, DatabaseError, Value as Binding, query::Builder,
};

use super::{
    Data, ValidationError, ValidationResult, Value,
    rules::{DatabaseRule, Exclusion, Unique, prepare_id},
};

/// Queries the application's write connections for database validation rules.
#[derive(Clone)]
pub struct DatabasePresenceVerifier {
    resolver: Arc<dyn ConnectionResolverInterface>,
}

impl DatabasePresenceVerifier {
    pub fn new(resolver: Arc<dyn ConnectionResolverInterface>) -> Self {
        Self { resolver }
    }

    pub(super) async fn exists(
        &self,
        rule: &DatabaseRule,
        column: &str,
        value: &Value,
    ) -> ValidationResult<bool> {
        let elements = match value {
            Value::Array(values) => Some(values.iter().collect::<Vec<_>>()),
            Value::Object(values) => Some(values.values().collect()),
            _ => None,
        };
        if let Some(elements) = elements {
            let Some(bindings) = elements
                .iter()
                .map(|value| binding(value))
                .collect::<Option<Vec<_>>>()
            else {
                return Ok(false);
            };
            let expected = elements
                .into_iter()
                .map(distinct_input)
                .collect::<HashSet<_>>()
                .len() as u64;
            if expected == 0 {
                return Ok(true);
            }
            let query = self.table(rule)?.where_in(column, bindings).distinct();
            return rule
                .apply(query)
                .count(column)
                .await
                .map(|count| count >= expected)
                .map_err(execution);
        }
        let Some(binding) = binding(value) else {
            return Ok(false);
        };
        let query = equal(self.table(rule)?, column, binding);
        rule.apply(query)
            .count("*")
            .await
            .map(|count| count > 0)
            .map_err(execution)
    }

    pub(super) async fn unique(
        &self,
        rule: &Unique,
        column: &str,
        value: &Value,
        data: &Data,
    ) -> ValidationResult<bool> {
        let Some(value) = binding(value) else {
            return Ok(false);
        };
        let mut query = equal(self.table(&rule.target)?, column, value);
        let excluded = match &rule.exclusion {
            Some(Exclusion::Value(column, id)) => Some((column, id.clone())),
            Some(Exclusion::Input(column, path)) => data
                .attributes(path)?
                .into_iter()
                .next()
                .and_then(|(_, value)| value)
                .filter(|value| !matches!(value, Value::Null | Value::Object(_) | Value::Array(_)))
                .and_then(|value| prepare_id(&distinct_input(value)))
                .map(|id| (column, id)),
            None => None,
        };
        if let Some((column, id)) = excluded.filter(|(_, id)| !matches!(id, Binding::Null)) {
            query = query.r#where(column, "<>", id);
        }
        rule.target
            .apply(query)
            .count("*")
            .await
            .map(|count| count == 0)
            .map_err(execution)
    }

    fn table(&self, rule: &DatabaseRule) -> ValidationResult<Builder<'static>> {
        Ok(self
            .resolver
            .connection(rule.connection.as_deref())
            .map_err(execution)?
            .table(&rule.table)
            .useWritePdo())
    }
}

fn equal(query: Builder<'static>, column: &str, value: Binding) -> Builder<'static> {
    if value == Binding::Null {
        query.where_null(column)
    } else {
        query.r#where(column, "=", value)
    }
}

fn execution(source: DatabaseError) -> ValidationError {
    ValidationError::Execution {
        source: Box::new(source),
    }
}

fn binding(value: &Value) -> Option<Binding> {
    Some(match value {
        Value::Null => Binding::Null,
        Value::Bool(value) => Binding::Bool(*value),
        Value::String(value) => Binding::Text(value.clone()),
        Value::Number(value) => value
            .as_i64()
            .map(Binding::Integer)
            .or_else(|| value.as_u64().map(Binding::Unsigned))
            .or_else(|| value.as_f64().map(Binding::Real))?,
        _ => return None,
    })
}

fn distinct_input(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(false) => String::new(),
        Value::Bool(true) => "1".into(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value
            .as_f64()
            .filter(|_| value.is_f64())
            .map_or_else(|| value.to_string(), php_float),
        _ => unreachable!("non-scalar values are rejected before comparison"),
    }
}

fn php_float(value: f64) -> String {
    // PHP's default string precision is 14 significant digits.
    let scientific = format!("{value:.13e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("scientific float");
    let exponent = exponent.parse::<i32>().expect("scientific exponent");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    if !(-4..14).contains(&exponent) {
        let decimal = if mantissa.contains('.') { "" } else { ".0" };
        return format!("{mantissa}{decimal}E{exponent:+}");
    }
    let precision = (13 - exponent) as usize;
    let fixed = format!("{value:.precision$}");
    if fixed.contains('.') {
        fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        fixed
    }
}
