use std::{
    any::{TypeId, type_name},
    sync::RwLock,
};

use indexmap::IndexMap;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{Connection, DatabaseError, DatabaseResult, eloquent::Model},
    foundation::Application,
};

/// A stored discriminator and its compiled model type.
#[derive(Clone, Copy)]
pub struct MorphMapEntry {
    alias: &'static str,
    model: &'static str,
    model_type: TypeId,
}

impl MorphMapEntry {
    pub fn model<M: Model>(alias: &'static str) -> Self {
        Self {
            alias,
            model: type_name::<M>(),
            model_type: TypeId::of::<M>(),
        }
    }
}

#[derive(Default)]
struct Mapping {
    entries: IndexMap<&'static str, MorphMapEntry>,
    required: bool,
}

/// The polymorphic type aliases owned by one application.
#[derive(Default)]
pub struct MorphMap(RwLock<Mapping>);

#[allow(non_snake_case, reason = "Matches Laravel's polymorphic mapping API")]
impl MorphMap {
    pub fn morphMap(
        app: &Application,
        entries: impl IntoIterator<Item = MorphMapEntry>,
        merge: bool,
    ) -> DatabaseResult<()> {
        Self::register(app, entries, merge, false)
    }

    pub fn enforceMorphMap(
        app: &Application,
        entries: impl IntoIterator<Item = MorphMapEntry>,
        merge: bool,
    ) -> DatabaseResult<()> {
        Self::register(app, entries, merge, true)
    }

    fn register(
        app: &Application,
        entries: impl IntoIterator<Item = MorphMapEntry>,
        merge: bool,
        required: bool,
    ) -> DatabaseResult<()> {
        let mut incoming = IndexMap::new();
        for entry in entries {
            if entry.alias.is_empty() {
                return Err(DatabaseError::InvalidQuery("empty morph alias"));
            }
            incoming.insert(entry.alias, entry);
        }

        let map = app
            .container()
            .make::<Self>()
            .map_err(DatabaseError::Unavailable)?;
        let mut map = map.0.write().unwrap();
        if merge {
            for (alias, entry) in &map.entries {
                incoming.entry(alias).or_insert(*entry);
            }
        }
        map.entries = incoming;
        map.required |= required;
        Ok(())
    }

    pub fn getMorphedModel(&self, alias: &str) -> Option<&'static str> {
        self.0
            .read()
            .unwrap()
            .entries
            .get(alias)
            .map(|entry| entry.model)
    }

    pub fn getMorphAlias<M: Model>(&self) -> &'static str {
        self.0
            .read()
            .unwrap()
            .entries
            .values()
            .find(|entry| entry.model_type == TypeId::of::<M>())
            .map_or_else(type_name::<M>, |entry| entry.alias)
    }

    fn aliases<M: Model>(&self) -> Vec<&'static str> {
        self.0
            .read()
            .unwrap()
            .entries
            .values()
            .filter(|entry| entry.model_type == TypeId::of::<M>())
            .map(|entry| entry.alias)
            .collect()
    }

    pub(super) fn aliases_for<M: Model>(
        connection: &Connection,
    ) -> DatabaseResult<Vec<&'static str>> {
        if let Some(container) = Connection::model_container(Some(connection)) {
            let map = container
                .make::<Self>()
                .map_err(DatabaseError::Unavailable)?;
            let aliases = map.aliases::<M>();
            if !aliases.is_empty() {
                return Ok(aliases);
            }
        }
        Ok(vec![Self::class_for::<M>(Some(connection))?])
    }

    pub(in crate::database::eloquent) fn class_for<M: Model>(
        connection: Option<&Connection>,
    ) -> DatabaseResult<&'static str> {
        let Some(container) = Connection::model_container(connection) else {
            return Ok(type_name::<M>());
        };
        let map = container
            .make::<Self>()
            .map_err(DatabaseError::Unavailable)?;
        let map = map.0.read().unwrap();
        if let Some(entry) = map
            .entries
            .values()
            .find(|entry| entry.model_type == TypeId::of::<M>())
        {
            return Ok(entry.alias);
        }
        if map.required {
            return Err(DatabaseError::InvalidQuery(
                "model requires a morph map entry",
            ));
        }
        Ok(type_name::<M>())
    }
}
