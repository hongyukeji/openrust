use std::sync::Arc;

use serde_json::{Map, Value};

use crate::database::{
    Value as AttributeValue,
    eloquent::{Model, RelationAggregate},
};

use super::ResourceResult;

/// Selects values lazily and keeps omitted fields distinct from explicit JSON nulls.
pub struct Conditional;

impl Conditional {
    pub fn when<T>(condition: bool, value: impl FnOnce() -> T) -> Option<T> {
        condition.then(value)
    }

    pub fn unless<T>(condition: bool, value: impl FnOnce() -> T) -> Option<T> {
        (!condition).then(value)
    }

    pub fn merge_when(
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
        Self::merge_when(fields, true, attributes);
    }

    /// Selects keys from the model's serialized attributes and relations.
    pub fn attributes(model: &impl Model, names: &[&str]) -> ResourceResult<Map<String, Value>> {
        let mut fields = model.to_array()?;
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

    pub fn merge_unless(
        fields: &mut Map<String, Value>,
        condition: bool,
        attributes: impl FnOnce() -> Map<String, Value>,
    ) {
        Self::merge_when(fields, !condition, attributes);
    }

    pub fn when_has<T>(
        model: &impl Model,
        attribute: &str,
        value: impl FnOnce() -> T,
    ) -> Option<T> {
        Self::when(
            model.state().get_attributes().contains_key(attribute),
            value,
        )
    }

    pub fn when_not_null<T>(value: &Value, present: impl FnOnce() -> T) -> Option<T> {
        Self::unless(value.is_null(), present)
    }

    pub fn when_null<T>(value: &Value, present: impl FnOnce() -> T) -> Option<T> {
        Self::when(value.is_null(), present)
    }

    pub fn when_appended<T>(
        model: &impl Model,
        attribute: &str,
        value: impl FnOnce() -> T,
    ) -> Option<T> {
        Self::when(model.has_appended(attribute), value)
    }

    pub fn when_loaded<'a, T: Send + Sync + 'static>(
        model: &'a impl Model,
        name: &str,
    ) -> Option<&'a T> {
        model.get_relation(name)
    }

    /// The outer option indicates whether the relationship was loaded.
    /// The inner option is absent when the loaded relationship has no model.
    pub fn when_loaded_one<M: Model, T>(
        model: &impl Model,
        name: &str,
        project: impl FnOnce(&M) -> T,
    ) -> Option<Option<T>> {
        Self::when_loaded::<Option<Arc<M>>>(model, name)
            .map(|related| related.as_deref().map(project))
    }

    pub fn when_counted<'a>(model: &'a impl Model, relation: &str) -> Option<&'a AttributeValue> {
        Self::when_aggregated(model, relation, RelationAggregate::Count)
    }

    pub fn when_aggregated<'a>(
        model: &'a impl Model,
        relation: &str,
        aggregate: RelationAggregate<'_>,
    ) -> Option<&'a AttributeValue> {
        model.get_attribute(&aggregate.alias(relation).ok()?)
    }

    pub fn when_exists_loaded<'a>(
        model: &'a impl Model,
        relation: &str,
    ) -> Option<&'a AttributeValue> {
        Self::when_aggregated(model, relation, RelationAggregate::Exists)
    }

    pub fn has_pivot_loaded(model: &impl Model, table: &str) -> bool {
        Self::has_pivot_loaded_as(model, "pivot", table)
    }

    pub fn has_pivot_loaded_as(model: &impl Model, accessor: &str, table: &str) -> bool {
        model.state().loaded_pivot_table(accessor) == Some(table)
    }

    pub fn when_pivot_loaded<T>(
        model: &impl Model,
        table: &str,
        value: impl FnOnce() -> T,
    ) -> Option<T> {
        Self::when_pivot_loaded_as(model, "pivot", table, value)
    }

    pub fn when_pivot_loaded_as<T>(
        model: &impl Model,
        accessor: &str,
        table: &str,
        value: impl FnOnce() -> T,
    ) -> Option<T> {
        Self::when(Self::has_pivot_loaded_as(model, accessor, table), value)
    }

    /// Matches a custom intermediate model by its Rust type.
    pub fn when_pivot_loaded_as_type<P: Model, T>(
        model: &impl Model,
        accessor: &str,
        project: impl FnOnce(&P) -> T,
    ) -> Option<T> {
        model.state().loaded_pivot_table(accessor)?;
        model.get_relation::<P>(accessor).map(project)
    }

    pub fn insert(fields: &mut Map<String, Value>, name: impl Into<String>, value: Option<Value>) {
        if let Some(value) = value {
            fields.insert(name.into(), value);
        }
    }
}
