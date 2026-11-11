use serde_json::{Map, Value};

use crate::database::{
    Value as AttributeValue,
    eloquent::{Model, RelationAggregate},
};

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
            fields.extend(attributes());
        }
    }

    pub fn whenHas<T>(model: &impl Model, attribute: &str, value: impl FnOnce() -> T) -> Option<T> {
        Self::when(model.state().getAttributes().contains_key(attribute), value)
    }

    pub fn whenNotNull<T>(value: &Value, present: impl FnOnce() -> T) -> Option<T> {
        Self::unless(value.is_null(), present)
    }

    pub fn whenLoaded<'a, T: Send + Sync + 'static>(
        model: &'a impl Model,
        name: &str,
    ) -> Option<&'a T> {
        model.getRelation(name)
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

    pub fn insert(fields: &mut Map<String, Value>, name: impl Into<String>, value: Option<Value>) {
        if let Some(value) = value {
            fields.insert(name.into(), value);
        }
    }
}
