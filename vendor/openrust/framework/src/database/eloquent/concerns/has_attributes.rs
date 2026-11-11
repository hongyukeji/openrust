use crate::database::{Connection, Value};

use super::super::Attributes;

/// Raw attributes and snapshots for one model instance.
#[derive(Default)]
pub struct ModelState {
    pub(in crate::database::eloquent) attributes: Attributes,
    pub(in crate::database::eloquent) original: Attributes,
    pub(in crate::database::eloquent) changes: Attributes,
    pub(in crate::database::eloquent) connection: Option<Connection>,
    pub(in crate::database::eloquent) exists: bool,
    pub(in crate::database::eloquent) was_recently_created: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's model attribute API")]
impl ModelState {
    pub fn getAttribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name)
    }

    pub fn setAttribute(&mut self, name: impl Into<String>, value: impl Into<Value>) {
        self.attributes.insert(name.into(), value.into());
    }

    pub(in crate::database::eloquent) fn apply_attributes(&mut self, attributes: Attributes) {
        self.attributes.extend(attributes);
    }

    pub fn getAttributes(&self) -> &Attributes {
        &self.attributes
    }

    pub fn getOriginal(&self) -> &Attributes {
        &self.original
    }

    pub fn getDirty(&self) -> Attributes {
        self.attributes
            .iter()
            .filter(|(key, value)| self.original.get(*key) != Some(*value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    pub fn isDirty(&self) -> bool {
        self.attributes != self.original
    }

    pub fn getChanges(&self) -> &Attributes {
        &self.changes
    }

    pub fn exists(&self) -> bool {
        self.exists
    }

    pub fn wasRecentlyCreated(&self) -> bool {
        self.was_recently_created
    }

    pub(in crate::database::eloquent) fn hydrated_attributes(
        attributes: Attributes,
        connection: Connection,
    ) -> Self {
        Self {
            original: attributes.clone(),
            attributes,
            connection: Some(connection),
            exists: true,
            ..Self::default()
        }
    }
}
