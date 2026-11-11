use std::{cell::RefCell, collections::HashSet, sync::Arc};

use serde_json::{Map, Value as Json};

use crate::database::{DatabaseError, DatabaseResult, Value};

use super::{Model, ModelMetadata, casts};

/// A boolean or a predicate evaluated against the current model.
pub trait VisibilityCondition<M> {
    fn evaluate(self, model: &M) -> bool;
}

impl<M> VisibilityCondition<M> for bool {
    fn evaluate(self, _: &M) -> bool {
        self
    }
}

impl<M, F: FnOnce(&M) -> bool> VisibilityCondition<M> for F {
    fn evaluate(self, model: &M) -> bool {
        self(model)
    }
}

#[derive(Default)]
pub(super) struct SerializationOptions {
    pub(super) hidden: Option<Vec<String>>,
    pub(super) visible: Option<Vec<String>>,
    pub(super) appends: Option<Vec<String>>,
}

pub(super) fn names<'a>(
    local: Option<&'a [String]>,
    defaults: &'static [&'static str],
) -> impl Iterator<Item = &'a str> {
    local.into_iter().flatten().map(String::as_str).chain(
        local
            .is_none()
            .then_some(defaults)
            .into_iter()
            .flatten()
            .copied(),
    )
}

pub(super) fn local_names<'a>(
    local: &'a mut Option<Vec<String>>,
    defaults: &[&str],
) -> &'a mut Vec<String> {
    local.get_or_insert_with(|| defaults.iter().map(|name| (*name).into()).collect())
}

pub(super) fn merge(list: &mut Vec<String>, attributes: &[&str]) {
    for name in attributes {
        if !list.iter().any(|current| current == name) {
            list.push((*name).into());
        }
    }
}

impl SerializationOptions {
    pub(super) const EMPTY: Self = Self {
        hidden: None,
        visible: None,
        appends: None,
    };

    pub(super) fn includes(&self, metadata: &ModelMetadata, name: &str) -> bool {
        let mut visible = names(self.visible.as_deref(), metadata.visible).peekable();
        (visible.peek().is_none() || visible.any(|attribute| attribute == name))
            && !names(self.hidden.as_deref(), metadata.hidden).any(|attribute| attribute == name)
    }
}

pub(super) fn attributes<M: Model>(model: &M) -> DatabaseResult<Map<String, Json>> {
    let state = model.state();
    let metadata = M::metadata();
    let options = state.serialization();
    let mut output = Map::new();
    for name in state
        .attributes
        .keys()
        .filter(|name| options.includes(metadata, name))
    {
        if let Some(value) = model.getAttributeValue(name)? {
            let value = casts::serialize_attribute(model, name, value)?;
            output.insert(name.clone(), json_value(value)?);
        }
    }
    for name in names(options.appends.as_deref(), metadata.appends)
        .filter(|name| options.includes(metadata, name))
    {
        let getter = M::attribute(name)
            .and_then(|attribute| attribute.get)
            .ok_or(DatabaseError::InvalidQuery("appended model accessor"))?;
        output.insert(name.into(), json_value(getter(None, &state.attributes)?)?);
    }
    Ok(output)
}

pub(super) fn json_value(value: Value) -> DatabaseResult<Json> {
    match value {
        Value::Bytes(bytes) => String::from_utf8(bytes)
            .map(Json::String)
            .map_err(|_| DatabaseError::InvalidQuery("model JSON UTF-8")),
        value => casts::json_value(value),
    }
}

thread_local! {
    static PROJECTING: RefCell<HashSet<usize>> = RefCell::default();
}

struct ProjectionGuard(usize);

impl Drop for ProjectionGuard {
    fn drop(&mut self) {
        PROJECTING.with(|models| models.borrow_mut().remove(&self.0));
    }
}

pub(super) fn model<M: Model>(model: &M) -> DatabaseResult<Map<String, Json>> {
    let state = model.state();
    let identity = std::ptr::from_ref(state).addr();
    if !PROJECTING.with(|models| models.borrow_mut().insert(identity)) {
        return model.attributesToArray();
    }
    let _guard = ProjectionGuard(identity);
    let mut output = model.attributesToArray()?;
    output.extend(model.relationsToArray()?);
    Ok(output)
}

pub(super) fn one<M: Model>(related: &Option<Arc<M>>) -> DatabaseResult<Option<Json>> {
    related
        .as_deref()
        .map_or(Ok(Json::Null), |model| model.toArray().map(Json::Object))
        .map(Some)
}

pub(super) fn many<M: Model>(related: &[Arc<M>]) -> DatabaseResult<Option<Json>> {
    related
        .iter()
        .map(|model| model.toArray().map(Json::Object))
        .collect::<DatabaseResult<Vec<_>>>()
        .map(|models| Some(Json::Array(models)))
}

pub(super) fn snake_relation(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_uppercase() && !result.is_empty() && !result.ends_with('_') {
            result.push('_');
        }
        result.extend(character.to_lowercase());
    }
    result
}
