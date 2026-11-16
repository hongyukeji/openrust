use std::sync::Arc;

use serde_json::{Map, Value};

use crate::database::{
    Value as AttributeValue,
    eloquent::{Model, RelationAggregate},
};
use crate::http::HttpError;

use super::ResourceResult;

/// Selects values lazily and keeps omitted fields distinct from explicit JSON nulls.
pub struct Conditional;

#[allow(
    non_snake_case,
    reason = "Matches Laravel's conditional resource helpers"
)]
impl Conditional {
    pub fn when<T>(condition: bool, value: impl FnOnce() -> T) -> Option<T> {
        condition.then(value)
    }

    pub fn unless<T>(condition: bool, value: impl FnOnce() -> T) -> Option<T> {
        (!condition).then(value)
    }

    pub fn mergeWhen(
        fields: &mut Map<String, Value>,
        condition: bool,
        attributes: impl FnOnce() -> Map<String, Value>,
    ) {
        if condition {
            for (name, value) in attributes() {
                fields.entry(name).or_insert(value);
            }
        }
    }

    pub fn merge(fields: &mut Map<String, Value>, attributes: impl FnOnce() -> Map<String, Value>) {
        Self::mergeWhen(fields, true, attributes);
    }

    /// Selects keys from the model's serialized attributes and relations.
    pub fn attributes(model: &impl Model, names: &[&str]) -> ResourceResult<Map<String, Value>> {
        let mut fields = model.toArray().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        fields.retain(|name, _| names.contains(&name.as_str()));
        Ok(fields)
    }

    /// Transforms a non-blank JSON value.
    pub fn transform<T>(value: &Value, project: impl FnOnce(&Value) -> T) -> Option<T> {
        let blank = match value {
            Value::Null => true,
            Value::String(text) => text
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
                .is_empty(),
            Value::Array(values) => values.is_empty(),
            Value::Object(values) => values.is_empty(),
            Value::Bool(_) | Value::Number(_) => false,
        };
        Self::unless(blank, || project(value))
    }

    pub fn mergeUnless(
        fields: &mut Map<String, Value>,
        condition: bool,
        attributes: impl FnOnce() -> Map<String, Value>,
    ) {
        Self::mergeWhen(fields, !condition, attributes);
    }

    pub fn whenHas<T>(model: &impl Model, attribute: &str, value: impl FnOnce() -> T) -> Option<T> {
        Self::when(model.state().getAttributes().contains_key(attribute), value)
    }

    pub fn whenNotNull<T>(value: &Value, present: impl FnOnce() -> T) -> Option<T> {
        Self::unless(value.is_null(), present)
    }

    pub fn whenNull<T>(value: &Value, present: impl FnOnce() -> T) -> Option<T> {
        Self::when(value.is_null(), present)
    }

    pub fn whenAppended<T>(
        model: &impl Model,
        attribute: &str,
        value: impl FnOnce() -> T,
    ) -> Option<T> {
        Self::when(model.hasAppended(attribute), value)
    }

    pub fn whenLoaded<'a, T: Send + Sync + 'static>(
        model: &'a impl Model,
        name: &str,
    ) -> Option<&'a T> {
        model.getRelation(name)
    }

    /// The outer option indicates whether the relationship was loaded.
    /// The inner option is absent when the loaded relationship has no model.
    pub fn whenLoadedOne<M: Model, T>(
        model: &impl Model,
        name: &str,
        project: impl FnOnce(&M) -> T,
    ) -> Option<Option<T>> {
        Self::whenLoaded::<Option<Arc<M>>>(model, name)
            .map(|related| related.as_deref().map(project))
    }

    pub fn whenCounted<'a>(model: &'a impl Model, relation: &str) -> Option<&'a AttributeValue> {
        Self::whenAggregated(model, relation, RelationAggregate::Count)
    }

    pub fn whenAggregated<'a>(
        model: &'a impl Model,
        relation: &str,
        aggregate: RelationAggregate<'_>,
    ) -> Option<&'a AttributeValue> {
        model.getAttribute(&aggregate.alias(relation).ok()?)
    }

    pub fn whenExistsLoaded<'a>(
        model: &'a impl Model,
        relation: &str,
    ) -> Option<&'a AttributeValue> {
        Self::whenAggregated(model, relation, RelationAggregate::Exists)
    }

    pub fn hasPivotLoaded(model: &impl Model, table: &str) -> bool {
        Self::hasPivotLoadedAs(model, "pivot", table)
    }

    pub fn hasPivotLoadedAs(model: &impl Model, accessor: &str, table: &str) -> bool {
        model.state().loaded_pivot_table(accessor) == Some(table)
    }

    pub fn whenPivotLoaded<T>(
        model: &impl Model,
        table: &str,
        value: impl FnOnce() -> T,
    ) -> Option<T> {
        Self::whenPivotLoadedAs(model, "pivot", table, value)
    }

    pub fn whenPivotLoadedAs<T>(
        model: &impl Model,
        accessor: &str,
        table: &str,
        value: impl FnOnce() -> T,
    ) -> Option<T> {
        Self::when(Self::hasPivotLoadedAs(model, accessor, table), value)
    }

    /// Matches a custom intermediate model by its Rust type.
    pub fn whenPivotLoadedAsType<P: Model, T>(
        model: &impl Model,
        accessor: &str,
        project: impl FnOnce(&P) -> T,
    ) -> Option<T> {
        model.state().loaded_pivot_table(accessor)?;
        model.getRelation::<P>(accessor).map(project)
    }

    pub fn insert(fields: &mut Map<String, Value>, name: impl Into<String>, value: Option<Value>) {
        if let Some(value) = value {
            fields.insert(name.into(), value);
        }
    }
}
