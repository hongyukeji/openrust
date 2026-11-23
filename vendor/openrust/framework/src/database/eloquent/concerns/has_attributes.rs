use crate::database::{Connection, Value};

use super::{
    super::{
        Attributes,
        casts::{CastOverrides, ValueObjects},
        serialization::SerializationOptions,
    },
    has_relationships::LoadedRelations,
};

/// Raw attributes and snapshots for one model instance.
#[derive(Clone, Default)]
pub struct ModelState {
    pub(in crate::database::eloquent) relations: LoadedRelations,
    pub(in crate::database::eloquent) detached_autoload:
        Option<Box<super::super::detached_autoload::DetachedAutoload>>,
    pub(in crate::database::eloquent) attributes: Attributes,
    pub(in crate::database::eloquent) original: Attributes,
    pub(in crate::database::eloquent) changes: Attributes,
    pub(in crate::database::eloquent) casts: CastOverrides,
    value_objects: Option<Box<ValueObjects>>,
    serialization: Option<Box<SerializationOptions>>,
    pub(in crate::database::eloquent) connection: Option<Connection>,
    pub(in crate::database::eloquent) pivot_identity:
        Option<super::super::relations::custom_pivot::PivotIdentity>,
    pub(in crate::database::eloquent) exists: bool,
    pub(in crate::database::eloquent) was_recently_created: bool,
    pub(in crate::database::eloquent) prevent_lazy_loading: bool,
}

impl ModelState {
    pub(crate) fn loaded_pivot_table(&self, name: &str) -> Option<&'static str> {
        self.relations.pivot_table(name)
    }

    pub(in crate::database::eloquent) fn serialization(&self) -> &SerializationOptions {
        self.serialization
            .as_deref()
            .unwrap_or(&SerializationOptions::EMPTY)
    }

    pub(in crate::database::eloquent) fn serialization_mut(&mut self) -> &mut SerializationOptions {
        self.serialization.get_or_insert_with(Box::default)
    }

    pub fn get_attribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name)
    }

    pub fn set_attribute(&mut self, name: impl Into<String>, value: impl Into<Value>) {
        self.attributes.insert(name.into(), value.into());
        self.invalidate_value_objects();
    }

    pub(in crate::database::eloquent) fn invalidate_value_objects(&mut self) {
        self.value_objects = None;
    }

    pub(in crate::database::eloquent) fn value_objects(&self) -> &ValueObjects {
        static EMPTY: ValueObjects = ValueObjects::EMPTY;
        self.value_objects.as_deref().unwrap_or(&EMPTY)
    }

    pub(in crate::database::eloquent) fn value_objects_mut(&mut self) -> &mut ValueObjects {
        self.value_objects.get_or_insert_with(Box::default)
    }

    pub(in crate::database::eloquent) fn apply_attributes(&mut self, attributes: Attributes) {
        if attributes
            .iter()
            .any(|(name, value)| self.attributes.get(name) != Some(value))
        {
            self.invalidate_value_objects();
        }
        self.attributes.extend(attributes);
    }

    pub(in crate::database::eloquent) fn replace_attributes(&mut self, attributes: Attributes) {
        if self.attributes != attributes {
            self.invalidate_value_objects();
            self.attributes = attributes;
        }
    }

    pub fn get_attributes(&self) -> &Attributes {
        &self.attributes
    }

    pub fn get_original(&self) -> &Attributes {
        &self.original
    }

    pub fn get_dirty(&self) -> Attributes {
        self.attributes
            .iter()
            .filter(|(key, value)| self.original.get(*key) != Some(*value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    pub fn is_dirty(&self) -> bool {
        self.attributes != self.original
    }

    pub fn get_changes(&self) -> &Attributes {
        &self.changes
    }

    pub fn exists(&self) -> bool {
        self.exists
    }

    pub fn was_recently_created(&self) -> bool {
        self.was_recently_created
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn clear_recently_created(&mut self) {
        self.was_recently_created = false;
    }

    pub(in crate::database::eloquent) fn replica(&self, attributes: Attributes) -> Self {
        Self {
            attributes,
            relations: self.relations.clone(),
            connection: self.connection.clone(),
            original: self
                .pivot_identity
                .map_or_else(Attributes::new, |identity| {
                    identity
                        .columns()
                        .filter_map(|name| {
                            self.original
                                .get(name)
                                .map(|value| (name.into(), value.clone()))
                        })
                        .collect()
                }),
            pivot_identity: self.pivot_identity,
            ..Self::default()
        }
    }

    pub(in crate::database::eloquent) fn hydrated_attributes(
        attributes: Attributes,
        connection: Connection,
        casts: CastOverrides,
    ) -> Self {
        Self {
            original: attributes.clone(),
            attributes,
            connection: Some(connection),
            casts,
            exists: true,
            ..Self::default()
        }
    }
}
