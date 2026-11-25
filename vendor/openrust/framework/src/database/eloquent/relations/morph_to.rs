use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use futures_util::TryStreamExt as _;
use indexmap::IndexMap;

use crate::{
    pagination::{CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, Paginator},
    support::Collection,
};

use crate::database::{
    Connection, DatabaseCursor, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, EagerLoads, Model, NamedRelations,
        concerns::has_relationships::update_association_cache,
        named_relations::{DiscoveredMorphs, MissingPaths, PathConstraint},
        touches::TouchedOwners,
    },
    events::with_query_caller,
    query,
};

use super::{
    ExistenceRelation, Relation,
    default_model::{ConfigureDefault, new_default},
    relation::{RelationKeys, dictionary_key},
};

/// An inverse polymorphic relationship whose aliases resolve to registered model types.
pub struct MorphTo<P: Model> {
    type_column: &'static str,
    id_column: &'static str,
    relation_name: Option<&'static str>,
    targets: IndexMap<&'static str, Box<dyn Target<P>>>,
    path_aliases: Option<Vec<&'static str>>,
    soft_deletes: SoftDeleteMacros,
    defaults: IndexMap<&'static str, Box<DefaultTarget<P>>>,
    untyped_default: Option<&'static str>,
}

type DefaultTarget<P> = dyn Fn(&P) -> DatabaseResult<ErasedModel> + Send + Sync;

#[derive(Clone, Copy, Default)]
struct SoftDeleteMacros {
    remove_scope: bool,
    require_live: bool,
    require_deleted: bool,
}

impl SoftDeleteMacros {
    fn apply<R: Model>(self, mut query: Builder<'static, R>) -> Builder<'static, R> {
        if let Some(column) = R::metadata().deleted_at
            && self.remove_scope
        {
            query = query.with_trashed();
            let column = query.qualify_column(column);
            if self.require_live {
                query.where_nested(|conditions| conditions.where_null(&column));
            }
            if self.require_deleted {
                query.where_nested(|conditions| conditions.where_not_null(&column));
            }
        }
        query
    }
}

/// A polymorphic model loaded from a registered alias.
#[derive(Clone)]
pub struct MorphTarget {
    alias: &'static str,
    model: ErasedModel,
}

/// The alias and owner key of a model for a morph identity query.
pub struct MorphIdentity {
    pub(in crate::database::eloquent) alias: &'static str,
    pub(in crate::database::eloquent) key: Value,
    pub(in crate::database::eloquent) model_type: TypeId,
    pub(in crate::database::eloquent) owner_key: &'static str,
}

impl MorphTarget {
    pub(super) fn from_model<M: Model>(alias: &'static str, model: Arc<M>) -> Self {
        Self { alias, model }
    }

    pub fn alias(&self) -> &'static str {
        self.alias
    }

    pub fn downcast_ref<M: Model>(&self) -> Option<&M> {
        self.model.as_ref().as_any().downcast_ref()
    }

    pub(in crate::database::eloquent) fn replace_model<M: Model>(&mut self, model: Arc<M>) {
        self.model = model;
    }
}

impl<P: Model> MorphTo<P> {
    /// Resolves the stored morph alias before comparing the owner key.
    pub fn is<R: Model>(&self, parent: &P, related: Option<&R>) -> bool {
        let Some(Value::Text(alias)) = parent.get_attribute(self.type_column) else {
            return false;
        };
        let Some(target) = self.targets.get(alias.as_str()) else {
            return false;
        };
        target.model_type() == TypeId::of::<R>()
            && super::BelongsTo::<P, R>::new(self.id_column, target.owner_key()).is(parent, related)
    }

    pub fn is_not<R: Model>(&self, parent: &P, related: Option<&R>) -> bool {
        !self.is(parent, related)
    }

    query_callers! {
    /// Writes to the owner table selected by the declaring model's stored morph alias.
    pub async fn upsert(
        &self,
        parent: &P,
        rows: Vec<Attributes>,
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let alias = match parent.get_attribute(self.type_column) {
            Some(Value::Text(alias)) if !alias.is_empty() => alias,
            None | Some(Value::Null | Value::Text(_)) => {
                return Err(DatabaseError::InvalidQuery("missing morph type"));
            }
            _ => return Err(DatabaseError::InvalidQuery("morph type column")),
        };
        self.targets
            .get(alias.as_str())
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?
            .upsert(parent, rows, unique_by, update, self.soft_deletes)
            .await
    }
    }

    pub fn identity<R: Model>(&self, alias: &str, model: &R) -> DatabaseResult<MorphIdentity> {
        self.validate_target::<R>(alias)?;
        let (&alias, target) = self
            .targets
            .get_key_value(alias)
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
        let owner_key = target.owner_key();
        let key = model
            .get_attribute(owner_key)
            .cloned()
            .unwrap_or(Value::Null);
        Ok(MorphIdentity {
            alias,
            key,
            model_type: TypeId::of::<R>(),
            owner_key,
        })
    }

    pub(in crate::database::eloquent) fn validate_identity(
        &self,
        identity: &MorphIdentity,
    ) -> DatabaseResult<()> {
        let target = self
            .targets
            .get(identity.alias)
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
        if target.model_type() != identity.model_type || target.owner_key() != identity.owner_key {
            return Err(DatabaseError::InvalidQuery(
                "morph identity does not match relation",
            ));
        }
        Ok(())
    }

    pub(in crate::database::eloquent) fn id_column(&self) -> &str {
        self.id_column
    }

    pub(in crate::database::eloquent) fn validate_target<R: Model>(
        &self,
        alias: &str,
    ) -> DatabaseResult<()> {
        let target = self
            .targets
            .get(alias)
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
        if target.model_type() != TypeId::of::<R>() {
            return Err(DatabaseError::InvalidQuery(
                "morph type does not match model",
            ));
        }
        Ok(())
    }

    pub fn new(type_column: &'static str, id_column: &'static str) -> Self {
        Self {
            type_column,
            id_column,
            relation_name: None,
            targets: IndexMap::new(),
            path_aliases: None,
            soft_deletes: SoftDeleteMacros::default(),
            defaults: IndexMap::new(),
            untyped_default: None,
        }
    }

    #[track_caller]
    pub fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<MorphTarget>>> {
        self.paginate_with(
            parent,
            PageOptions {
                per_page: Some(per_page),
                ..Default::default()
            },
        )
    }

    #[track_caller]
    pub fn paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<MorphTarget>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            let Some((alias, target)) = self.selected_target(parent)? else {
                return super::pagination::empty_length(options);
            };
            target
                .paginate(parent, alias, self.soft_deletes, options)
                .await
        })
    }

    #[track_caller]
    pub fn simple_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<Paginator<MorphTarget>>> {
        self.simple_paginate_with(
            parent,
            PageOptions {
                per_page: Some(per_page),
                ..Default::default()
            },
        )
    }

    #[track_caller]
    pub fn simple_paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<Paginator<MorphTarget>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            let Some((alias, target)) = self.selected_target(parent)? else {
                return super::pagination::empty_simple(options);
            };
            target
                .simple_paginate(parent, alias, self.soft_deletes, options)
                .await
        })
    }

    #[track_caller]
    pub fn cursor_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<MorphTarget>>> {
        self.cursor_paginate_with(
            parent,
            CursorOptions {
                per_page: Some(per_page),
                ..Default::default()
            },
        )
    }

    #[track_caller]
    pub fn cursor_paginate_with(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<MorphTarget>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            let Some((alias, target)) = self.selected_target(parent)? else {
                return super::pagination::empty_cursor(options);
            };
            target
                .cursor_paginate(parent, alias, self.soft_deletes, options)
                .await
        })
    }

    super::key_chunks::lazy_chunks!(MorphTarget);

    fn lazy_relation<'a>(
        &'a self,
        parent: &'a P,
        chunk_size: u64,
        key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
    ) -> DatabaseCursor<'a, MorphTarget> {
        query::flatten_batches(
            self.morph_batches(parent, chunk_size, key, true),
            |models| models.into_iter().map(|(_, model)| model),
        )
    }

    pub async fn each<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(MorphTarget, crate::support::ArrayKey) -> Fut,
        count: u64,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let batches = self.morph_batches(parent, count, None, false);
        query::visit_batches(batches, |_, key| Ok(key), callback).await
    }

    pub async fn each_by_id<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(MorphTarget, i64) -> Fut,
        count: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let batches = self.morph_batches(parent, count, Some((column, alias, false)), false);
        query::visit_batches(
            batches,
            |batch, key| query::collection_chunk_index(batch, count, key),
            callback,
        )
        .await
    }

    fn morph_batches<'a>(
        &'a self,
        parent: &'a P,
        count: u64,
        key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
        lazy: bool,
    ) -> DatabaseCursor<'a, Collection<MorphTarget>> {
        Box::pin(async_stream::try_stream! {
            if count == 0 {
                Err(DatabaseError::InvalidQuery("chunk size must be positive"))?;
            }
            if let Some((alias, target)) = self.selected_target(parent)? {
                let mut batches = target.chunks(parent, alias, self.soft_deletes, count, key, lazy);
                while let Some(batch) = batches.try_next().await? {
                    yield batch;
                }
            }
        })
    }

    /// Processes persisted rows from the registered morph target in ordered batches.
    pub async fn chunk<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(Collection<MorphTarget>, u64) -> Fut,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.chunk_with(parent, count, callback, None).await
    }

    super::key_chunks::key_chunks!(Collection<MorphTarget>);

    pub async fn ordered_chunk_by_id<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(Collection<MorphTarget>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.chunk_with(parent, count, callback, Some((column, alias, descending)))
            .await
    }

    async fn chunk_with<Fut>(
        &self,
        parent: &P,
        count: u64,
        mut callback: impl FnMut(Collection<MorphTarget>, u64) -> Fut,
        key: Option<(Option<&str>, Option<&str>, bool)>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let mut batches = self.morph_batches(parent, count, key, false);
        let mut page = 1;
        while let Some(models) = batches.try_next().await? {
            if !callback(models, page).await? {
                return Ok(false);
            }
            page += 1;
        }
        Ok(true)
    }

    /// Streams rows from the registered target selected by the parent's morph type.
    #[track_caller]
    pub fn cursor<'a>(&'a self, parent: &'a P) -> DatabaseCursor<'a, MorphTarget> {
        Box::pin(crate::database::events::stream_with_query_caller(
            std::panic::Location::caller(),
            async_stream::try_stream! {
                if let Some((alias, target)) = self.selected_target(parent)? {
                    let mut targets = target.cursor(parent, self.soft_deletes);
                    while let Some(model) = targets.try_next().await? {
                        yield MorphTarget { alias, model };
                    }
                }
            },
        ))
    }

    fn selected_target(
        &self,
        parent: &P,
    ) -> DatabaseResult<Option<(&'static str, &dyn Target<P>)>> {
        let Some(alias) = self.query_alias(parent)? else {
            return Ok(None);
        };
        let (&alias, target) = self
            .targets
            .get_key_value(alias)
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
        Ok(self
            .path_aliases
            .as_ref()
            .is_none_or(|aliases| aliases.contains(&alias))
            .then_some((alias, target.as_ref())))
    }

    /// Names the cache entry updated by association and dissociation.
    #[must_use]
    pub fn named(mut self, name: &'static str) -> Self {
        self.relation_name = Some(name);
        self
    }

    /// Supplies an unsaved model when this registered target has no matching row.
    pub fn with_default<R: Model>(
        mut self,
        alias: &'static str,
        configure: impl Fn(&mut R, &P) + Send + Sync + 'static,
    ) -> DatabaseResult<Self> {
        self.validate_target::<R>(alias)?;
        let keys = RelationKeys::<P, R>::new(self.id_column, self.targets[alias].owner_key());
        let configure: Box<ConfigureDefault<P, R>> = Box::new(configure);
        self.defaults.insert(
            alias,
            Box::new(move |parent| {
                Ok(
                    new_default(parent, configure.as_ref(), &keys, Attributes::new())?
                        as ErasedModel,
                )
            }),
        );
        Ok(self)
    }

    /// Selects a configured default for parents without a morph type.
    pub fn default_for_missing_type(mut self, alias: &'static str) -> DatabaseResult<Self> {
        if !self.defaults.contains_key(alias) {
            return Err(DatabaseError::InvalidQuery(
                "morph type has no default model",
            ));
        }
        self.untyped_default = Some(alias);
        Ok(self)
    }

    fn clear_default(&mut self, alias: &str) {
        self.defaults.shift_remove(alias);
        if self.untyped_default == Some(alias) {
            self.untyped_default = None;
        }
    }

    /// Includes soft-deleted targets when loading this relation.
    #[must_use]
    pub fn with_trashed(mut self) -> Self {
        self.soft_deletes.remove_scope = true;
        self
    }

    /// Requires targets with a soft-delete column to be live.
    #[must_use]
    pub fn without_trashed(mut self) -> Self {
        self.soft_deletes.require_live = true;
        self.with_trashed()
    }

    /// Requires targets with a soft-delete column to be deleted.
    #[must_use]
    pub fn only_trashed(mut self) -> Self {
        self.soft_deletes.require_deleted = true;
        self.with_trashed()
    }

    /// Registers a compiled target under the alias selected by this connection's morph map.
    pub fn target_mapped<R: Model>(
        mut self,
        connection: &Connection,
        owner_key: &'static str,
    ) -> DatabaseResult<Self> {
        for alias in super::MorphMap::aliases_for::<R>(connection)? {
            self = self.target::<R>(alias, owner_key);
        }
        Ok(self)
    }

    #[must_use]
    pub fn target<R: Model>(mut self, alias: &'static str, owner_key: &'static str) -> Self {
        self.clear_default(alias);
        self.targets.insert(
            alias,
            Box::new(RelationKeys::<P, R>::new(self.id_column, owner_key)),
        );
        self
    }

    #[must_use]
    pub fn constrain<R: Model>(
        self,
        alias: &'static str,
        owner_key: &'static str,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.try_constrain::<R>(alias, owner_key, move |query| Ok(configure(query)))
    }

    #[must_use]
    pub fn try_constrain<R: Model>(
        mut self,
        alias: &'static str,
        owner_key: &'static str,
        configure: impl Fn(Builder<'static, R>) -> DatabaseResult<Builder<'static, R>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        if let Some(keys) = self
            .targets
            .get_mut(alias)
            .and_then(|target| target.as_any_mut().downcast_mut::<RelationKeys<P, R>>())
            .filter(|keys| keys.related == owner_key)
        {
            keys.morph_constraint = Some(Box::new(configure));
        } else {
            self.clear_default(alias);
            let mut keys = RelationKeys::<P, R>::new(self.id_column, owner_key);
            keys.morph_constraint = Some(Box::new(configure));
            self.targets.insert(alias, Box::new(keys));
        }
        self
    }

    /// Adds typed eager loads to an already registered morph target.
    pub fn morph_with<R: Model>(
        mut self,
        alias: &str,
        configure: impl FnOnce(EagerLoads<R>) -> EagerLoads<R>,
    ) -> DatabaseResult<Self> {
        let target = self
            .targets
            .get_mut(alias)
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
        let keys = target
            .as_any_mut()
            .downcast_mut::<RelationKeys<P, R>>()
            .ok_or(DatabaseError::InvalidQuery(
                "morph type does not match model",
            ))?;
        keys.with_nested(configure(EagerLoads::default()));
        Ok(self)
    }

    /// Adds registered relationship paths to an already registered morph target.
    pub fn morph_with_paths<R: Model>(self, alias: &str, paths: &[&str]) -> DatabaseResult<Self> {
        let loads = NamedRelations::<R>::prepare(paths)?;
        self.morph_with(alias, |_| loads)
    }

    #[must_use]
    pub fn morph_with_count<R: Model>(
        mut self,
        alias: &'static str,
        owner_key: &'static str,
        name: &'static str,
        relation: impl ExistenceRelation<Parent = R>,
    ) -> Self {
        let configure = move |query: Builder<'static, R>| query.with_count(name, &relation);
        if let Some(keys) = self
            .targets
            .get_mut(alias)
            .and_then(|target| target.as_any_mut().downcast_mut::<RelationKeys<P, R>>())
            .filter(|keys| keys.related == owner_key)
        {
            keys.append_query_configuration(configure);
            self
        } else {
            self.clear_default(alias);
            let mut keys = RelationKeys::<P, R>::new(self.id_column, owner_key);
            keys.try_constrain(configure);
            self.targets.insert(alias, Box::new(keys));
            self
        }
    }

    pub(in crate::database::eloquent) fn select_path_columns(
        &mut self,
        columns: Option<&[String]>,
    ) {
        if let Some(columns) = columns {
            for target in self.targets.values_mut() {
                target.select_path_columns(columns.to_vec());
            }
        }
    }

    pub(in crate::database::eloquent) fn with_constrained_paths(
        &mut self,
        name: &str,
        children: &[&str],
        constraint: Option<&PathConstraint>,
        discovered: Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<()> {
        self.path_aliases = discovered
            .and_then(|found| found.get::<P>(name))
            .map(<[_]>::to_vec);
        if !children.is_empty() || constraint.is_some() {
            for (alias, target) in &mut self.targets {
                if self
                    .path_aliases
                    .as_ref()
                    .is_none_or(|aliases| aliases.contains(alias))
                {
                    target.with_paths(children, constraint, discovered)?;
                }
            }
        }
        Ok(())
    }

    pub(in crate::database::eloquent) async fn load_missing_paths(
        &self,
        models: &mut [P],
        name: &str,
        children: &MissingPaths<'_>,
    ) -> DatabaseResult<()> {
        if children.is_empty() {
            return Ok(());
        }
        let mut outputs = models
            .iter()
            .map(|model| {
                model
                    .get_relation::<Option<MorphTarget>>(name)
                    .cloned()
                    .ok_or(DatabaseError::InvalidQuery("relationship output type"))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        let mut groups = IndexMap::<_, Vec<_>>::new();
        for target in outputs.iter_mut().flatten() {
            groups
                .entry(target.alias)
                .or_default()
                .push(&mut target.model);
        }
        for (alias, mut targets) in groups {
            self.targets
                .get(alias)
                .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?
                .load_missing_paths(&mut targets, children)
                .await?;
        }
        for (model, output) in models.iter_mut().zip(outputs) {
            model
                .state_mut()
                .relations
                .insert_relation::<Self>(name.to_owned(), output);
        }
        Ok(())
    }

    pub(in crate::database::eloquent) fn filter_path(
        &self,
        connection: &Connection,
        parent: &query::Builder<'static>,
        child: &str,
        (operator, count): (&str, u64),
        discovered: Option<(&DiscoveredMorphs, &str)>,
        constraint: Option<&PathConstraint>,
    ) -> DatabaseResult<query::Expression> {
        let qualifier = parent.relation_qualifier();
        let column = format!("{qualifier}.{}", self.type_column);
        let absent = (operator, count) == ("<", 1);
        let (op, n) = if absent { (">=", 1) } else { (operator, count) };
        let aliases = discovered.and_then(|(found, name)| found.get::<P>(name));
        let queries = self
            .targets
            .iter()
            .filter(|(alias, _)| aliases.is_none_or(|aliases| aliases.contains(alias)))
            .map(|(&alias, target)| {
                let query = target.path_query(
                    connection,
                    qualifier,
                    child,
                    (op, n),
                    discovered.map(|(found, _)| found),
                    constraint,
                )?;
                Ok((alias, query))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        if absent {
            parent.morph_count_expression(&column, queries, ">=", 1, false, true)
        } else if child.is_empty() {
            parent.morph_count_expression(&column, queries, op, n, false, false)
        } else {
            parent.morph_count_expression(&column, queries, ">=", 1, false, false)
        }
    }

    pub(in crate::database::eloquent) async fn discover_path(
        &self,
        connection: &Connection,
        name: &str,
        child: &str,
        found: &mut DiscoveredMorphs,
    ) -> DatabaseResult<()> {
        let aliases = match found.get::<P>(name) {
            Some(aliases) => aliases.to_vec(),
            None => {
                let aliases = self.discover_types(connection).await?;
                found.insert::<P>(name, aliases.clone());
                aliases
            }
        };
        if !child.is_empty() {
            for alias in aliases {
                self.targets[alias]
                    .discover_path(connection, child, found)
                    .await?;
            }
        }
        Ok(())
    }

    pub(in crate::database::eloquent) fn existence_queries(
        &self,
        connection: &Connection,
        parent_table: &str,
        aliases: &[&str],
    ) -> DatabaseResult<Vec<(&'static str, query::Builder<'static>)>> {
        let mut queries = IndexMap::new();
        for alias in aliases {
            let (&alias, target) = self
                .targets
                .get_key_value(*alias)
                .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
            if let indexmap::map::Entry::Vacant(entry) = queries.entry(alias) {
                entry.insert(target.existence_query(connection, parent_table)?);
            }
        }
        Ok(queries.into_iter().collect())
    }

    pub(in crate::database::eloquent) async fn discover_types(
        &self,
        connection: &Connection,
    ) -> DatabaseResult<Vec<&'static str>> {
        let values = connection
            .table(P::metadata().table)
            .distinct()
            .pluck(self.type_column)
            .await?;
        let mut aliases = Vec::with_capacity(values.len());
        for value in values {
            match value {
                Value::Null => (),
                Value::Text(alias) if alias.is_empty() || alias == "0" => (),
                Value::Text(alias) => {
                    let (&alias, _) = self
                        .targets
                        .get_key_value(alias.as_str())
                        .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
                    aliases.push(alias);
                }
                _ => return Err(DatabaseError::InvalidQuery("morph type column")),
            }
        }
        Ok(aliases)
    }

    pub(in crate::database::eloquent) fn type_column(&self) -> &str {
        self.type_column
    }

    /// Associates a model using its mapped alias.
    pub fn associate_mapped<R: Model>(&self, parent: &mut P, related: &R) -> DatabaseResult<()> {
        self.associate(parent, related.get_morph_class()?, related)
    }

    pub fn associate<R: Model>(
        &self,
        model: &mut P,
        alias: &str,
        related: &R,
    ) -> DatabaseResult<()> {
        let (&alias, target) = self
            .targets
            .get_key_value(alias)
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
        if alias.is_empty() {
            return Err(DatabaseError::InvalidQuery("empty morph type"));
        }
        if target.model_type() != TypeId::of::<R>() {
            return Err(DatabaseError::InvalidQuery(
                "morph type does not match model",
            ));
        }
        if let (Some(parent), Some(owner)) =
            (&model.state().connection, &related.state().connection)
            && !parent.same_query_source(owner)
            && R::metadata().connection.is_none()
        {
            return Err(DatabaseError::InvalidQuery(
                "mixed relationship connections",
            ));
        }
        let key = related
            .get_attribute(target.owner_key())
            .filter(|value| !matches!(value, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship owner key"))?;
        dictionary_key(Some(key))?;
        update_association_cache::<Self>(model, self.relation_name, || {
            Some(Some(MorphTarget {
                alias,
                model: Arc::new(R::from_state(related.state().clone())),
            }))
        });
        model
            .set_attribute(self.type_column, alias)
            .set_attribute(self.id_column, key.clone());
        Ok(())
    }

    pub fn dissociate(&self, model: &mut P) {
        update_association_cache::<Self>(model, self.relation_name, || Some(None));
        model
            .set_attribute(self.type_column, Value::Null)
            .set_attribute(self.id_column, Value::Null);
    }

    pub(in crate::database::eloquent) async fn touch_owner(
        &self,
        model: &P,
        attributes: &Attributes,
        visited: &mut TouchedOwners,
        transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        if dictionary_key(attributes.get(self.id_column))?.is_none() {
            return Ok(());
        }
        let alias = match attributes.get(self.type_column) {
            None | Some(Value::Null) => return Ok(()),
            Some(Value::Text(alias)) => alias,
            _ => return Err(DatabaseError::InvalidQuery("morph type column")),
        };
        self.targets
            .get(alias.as_str())
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?
            .touch(model, attributes, visited, transaction)
            .await
    }

    fn query_alias<'a>(&self, parent: &'a P) -> DatabaseResult<Option<&'a str>> {
        if dictionary_key(parent.get_attribute(self.id_column))?.is_none() {
            return Ok(None);
        }
        match parent.get_attribute(self.type_column) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Text(alias)) if alias.is_empty() => Ok(None),
            Some(Value::Text(alias)) => Ok(Some(alias)),
            _ => Err(DatabaseError::InvalidQuery("morph type column")),
        }
    }

    async fn match_targets(
        &self,
        parents: &[&P],
        mut transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<Vec<Option<MorphTarget>>> {
        let mut groups = IndexMap::<&str, Vec<(usize, &P)>>::new();
        for (index, parent) in parents.iter().copied().enumerate() {
            if let Some(alias) = self.query_alias(parent)? {
                groups.entry(alias).or_default().push((index, parent));
            }
        }
        let mut matches: Vec<_> = (0..parents.len()).map(|_| None).collect();
        for (alias, rows) in groups {
            let (&alias, target) = self
                .targets
                .get_key_value(alias)
                .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
            if self
                .path_aliases
                .as_ref()
                .is_some_and(|aliases| !aliases.contains(&alias))
            {
                continue;
            }
            let (indexes, parents): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
            let loaded = target
                .load(&parents, transaction.as_deref_mut(), self.soft_deletes)
                .await?;
            for (index, model) in indexes.into_iter().zip(loaded) {
                matches[index] = model.map(|model| MorphTarget { alias, model });
            }
        }
        if !self.defaults.is_empty() {
            for (parent, matched) in parents.iter().zip(&mut matches) {
                if matched.is_none() {
                    *matched = self.default_target(parent)?;
                }
            }
        }
        Ok(matches)
    }

    fn default_target(&self, parent: &P) -> DatabaseResult<Option<MorphTarget>> {
        let alias = match parent.get_attribute(self.type_column) {
            None | Some(Value::Null) => self.untyped_default,
            Some(Value::Text(alias)) if alias.is_empty() => self.untyped_default,
            Some(Value::Text(alias)) => Some(alias.as_str()),
            _ => None,
        };
        let Some((alias, create)) = alias.and_then(|alias| self.defaults.get_key_value(alias))
        else {
            return Ok(None);
        };
        if self
            .path_aliases
            .as_ref()
            .is_some_and(|aliases| !aliases.contains(alias))
        {
            return Ok(None);
        }
        Ok(Some(MorphTarget {
            alias,
            model: create(parent)?,
        }))
    }
}

type ErasedModel = Arc<dyn ProjectableModel>;

trait ProjectableModel: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn project(&self) -> DatabaseResult<serde_json::Value>;
    fn queueable_relations(&self) -> Vec<String>;
    fn push_snapshot<'a>(
        self: Arc<Self>,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (ErasedModel, DatabaseResult<bool>)>;
    fn attach_autoload(&self, group: &mut [&mut MorphTarget]);
}

impl<M: Model> ProjectableModel for M {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn push_snapshot<'a>(
        mut self: Arc<Self>,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (ErasedModel, DatabaseResult<bool>)> {
        Box::pin(async move {
            let result = context.push_model(&mut self).await;
            (self as ErasedModel, result)
        })
    }

    fn attach_autoload(&self, group: &mut [&mut MorphTarget]) {
        let mut models = IndexMap::new();
        for target in group.iter() {
            if let Some(model) = target.downcast_ref::<M>() {
                models
                    .entry(std::ptr::from_ref(model).addr())
                    .or_insert_with(|| M::from_state(model.state().clone()));
            }
        }
        let (addresses, mut models): (Vec<_>, Vec<_>) = models.into_iter().unzip();
        super::super::detached_autoload::attach(&mut models);
        let replacements: IndexMap<_, _> = addresses
            .into_iter()
            .zip(models.into_iter().map(Arc::new))
            .collect();
        for target in group {
            if let Some(model) = target.downcast_ref::<M>()
                && let Some(replacement) = replacements.get(&std::ptr::from_ref(model).addr())
            {
                target.replace_model(replacement.clone());
            }
        }
    }

    fn project(&self) -> DatabaseResult<serde_json::Value> {
        self.to_array().map(serde_json::Value::Object)
    }

    fn queueable_relations(&self) -> Vec<String> {
        self.get_queueable_relations()
    }
}

trait Target<P: Model>: Send + Sync {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn model_type(&self) -> TypeId;
    fn owner_key(&self) -> &'static str;
    fn existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>>;
    fn discover_path<'a>(
        &self,
        connection: &'a Connection,
        child: &'a str,
        found: &'a mut DiscoveredMorphs,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
    fn load_missing_paths<'a>(
        &'a self,
        targets: &'a mut [&mut ErasedModel],
        children: &'a MissingPaths<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
    fn select_path_columns(&mut self, columns: Vec<String>);
    fn with_paths(
        &mut self,
        children: &[&str],
        constraint: Option<&PathConstraint>,
        discovered: Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<()>;
    fn path_query(
        &self,
        connection: &Connection,
        parent: &str,
        child: &str,
        comparison: (&str, u64),
        discovered: Option<&DiscoveredMorphs>,
        constraint: Option<&PathConstraint>,
    ) -> DatabaseResult<query::Builder<'static>>;
    fn paginate<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        options: PageOptions<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<LengthAwarePaginator<MorphTarget>>>;
    fn simple_paginate<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        options: PageOptions<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<Paginator<MorphTarget>>>;
    fn cursor_paginate<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        options: CursorOptions<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<CursorPaginator<MorphTarget>>>;
    fn cursor<'a>(
        &'a self,
        parent: &'a P,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseCursor<'a, ErasedModel>;
    fn chunks<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        count: u64,
        key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
        lazy: bool,
    ) -> DatabaseCursor<'a, Collection<MorphTarget>>;
    fn load_first<'a>(
        &'a self,
        parent: &'a P,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<ErasedModel>>>;
    fn load<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Option<ErasedModel>>>>;
    fn upsert<'a>(
        &'a self,
        parent: &'a P,
        rows: Vec<Attributes>,
        unique_by: &'a [&'a str],
        update: Option<&'a [&'a str]>,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>>;
    fn touch<'a>(
        &'a self,
        model: &'a P,
        attributes: &'a Attributes,
        visited: &'a mut TouchedOwners,
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

impl<P: Model, R: Model> Target<P> for RelationKeys<P, R> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn upsert<'a>(
        &'a self,
        parent: &'a P,
        rows: Vec<Attributes>,
        unique_by: &'a [&'a str],
        update: Option<&'a [&'a str]>,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(async move {
            let connection = self.connection(parent)?;
            let query = self.configure(R::query_on(&connection))?;
            soft_deletes
                .apply(query)
                .upsert(rows, unique_by, update)
                .await
        })
    }

    fn touch<'a>(
        &'a self,
        model: &'a P,
        attributes: &'a Attributes,
        visited: &'a mut TouchedOwners,
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(self.touch_owner(model, attributes, visited, transaction))
    }

    fn model_type(&self) -> TypeId {
        TypeId::of::<R>()
    }

    fn owner_key(&self) -> &'static str {
        self.related
    }

    fn existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        RelationKeys::existence_query(self, connection, parent_table)
    }

    fn discover_path<'a>(
        &self,
        connection: &'a Connection,
        child: &'a str,
        found: &'a mut DiscoveredMorphs,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        NamedRelations::<R>::discover(connection, child, found)
    }

    fn load_missing_paths<'a>(
        &'a self,
        targets: &'a mut [&mut ErasedModel],
        children: &'a MissingPaths<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let models =
                targets
                    .iter()
                    .map(|target| {
                        target.as_ref().as_any().downcast_ref::<R>().ok_or(
                            DatabaseError::InvalidQuery("morph type does not match model"),
                        )
                    })
                    .collect::<DatabaseResult<Vec<_>>>()?;
            let replacements = super::super::named_relations::load_missing_models(
                models.iter().copied(),
                children,
            )
            .await?;
            let keys = models
                .into_iter()
                .map(|model| std::ptr::from_ref(model).addr())
                .collect::<Vec<_>>();
            for (target, key) in targets.iter_mut().zip(keys) {
                **target = replacements[&key].clone();
            }
            Ok(())
        })
    }

    fn select_path_columns(&mut self, columns: Vec<String>) {
        self.append_constraint(move |query| query.select_path_columns(&columns));
    }

    fn with_paths(
        &mut self,
        children: &[&str],
        constraint: Option<&PathConstraint>,
        discovered: Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<()> {
        if children.is_empty() {
            if let Some(constraint) = constraint {
                let constraint = constraint.clone();
                self.append_constraint(move |query| {
                    query.with_path_constraint(constraint.as_ref())
                });
            }
        } else {
            self.with_nested(NamedRelations::<R>::prepare_constrained(
                children, constraint, discovered,
            )?);
        }
        Ok(())
    }

    fn path_query(
        &self,
        connection: &Connection,
        parent: &str,
        child: &str,
        comparison: (&str, u64),
        discovered: Option<&DiscoveredMorphs>,
        constraint: Option<&PathConstraint>,
    ) -> DatabaseResult<query::Builder<'static>> {
        let query = self.existence_query(connection, parent)?;
        if child.is_empty() {
            let Some(constraint) = constraint else {
                return Ok(query);
            };
            let qualifier = query.relation_qualifier().to_owned();
            Ok(query.where_nested(|conditions| constraint(conditions, &qualifier)))
        } else {
            let predicate = NamedRelations::<R>::filter(
                connection, &query, child, comparison, discovered, constraint,
            )?;
            Ok(query.and_expression(predicate))
        }
    }

    fn paginate<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        options: PageOptions<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<LengthAwarePaginator<MorphTarget>>> {
        Box::pin(async move {
            let Some(query) = self.owner_query(parent)? else {
                return super::pagination::empty_length(PageOptions {
                    per_page: options.per_page.or(Some(R::metadata().per_page)),
                    ..options
                });
            };
            let page = soft_deletes.apply(query).paginate_with(options).await?;
            Ok(page.map(|model| MorphTarget::from_model(alias, Arc::new(model))))
        })
    }

    fn simple_paginate<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        options: PageOptions<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<Paginator<MorphTarget>>> {
        Box::pin(async move {
            let Some(query) = self.owner_query(parent)? else {
                return super::pagination::empty_simple(PageOptions {
                    per_page: options.per_page.or(Some(R::metadata().per_page)),
                    ..options
                });
            };
            let page = soft_deletes
                .apply(query)
                .simple_paginate_with(options)
                .await?;
            Ok(page.map(|model| MorphTarget::from_model(alias, Arc::new(model))))
        })
    }

    fn cursor_paginate<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        options: CursorOptions<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<CursorPaginator<MorphTarget>>> {
        Box::pin(async move {
            let Some(query) = self.owner_query(parent)? else {
                return super::pagination::empty_cursor(CursorOptions {
                    per_page: options.per_page.or(Some(R::metadata().per_page)),
                    ..options
                });
            };
            let page = soft_deletes
                .apply(query)
                .cursor_paginate_with(options)
                .await?;
            Ok(page.map(|model| MorphTarget::from_model(alias, Arc::new(model))))
        })
    }

    fn cursor<'a>(
        &'a self,
        parent: &'a P,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseCursor<'a, ErasedModel> {
        Box::pin(async_stream::try_stream! {
            if let Some(query) = self.owner_query(parent)? {
                let mut models = soft_deletes.apply(query).cursor();
                while let Some(model) = models.try_next().await? {
                    yield Arc::new(model) as ErasedModel;
                }
            }
        })
    }

    fn chunks<'a>(
        &'a self,
        parent: &'a P,
        alias: &'static str,
        soft_deletes: SoftDeleteMacros,
        count: u64,
        key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
        lazy: bool,
    ) -> DatabaseCursor<'a, Collection<MorphTarget>> {
        Box::pin(async_stream::try_stream! {
            if let Some(query) = self.owner_query(parent)? {
                let query = soft_deletes.apply(query);
                let key = key.map(|(column, alias, descending)| {
                    let mut key = query.chunk_key(column, alias, descending);
                    key.lazy = lazy;
                    key
                });
                let mut batches = query.chunk_collections(count, key, |_, _| Ok(()));
                while let Some(models) = batches.try_next().await? {
                    let targets = models.to_base().into_iter().map(|(key, model)| {
                        (key, MorphTarget::from_model(alias, Arc::new(model)))
                    });
                    yield Collection::from_entries(targets);
                }
            }
        })
    }

    fn load_first<'a>(
        &'a self,
        parent: &'a P,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<ErasedModel>>> {
        Box::pin(async move {
            let Some(query) = self.owner_query(parent)? else {
                return Ok(None);
            };
            Ok(soft_deletes
                .apply(query)
                .first()
                .await?
                .map(|model| Arc::new(model) as ErasedModel))
        })
    }

    fn load<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
        soft_deletes: SoftDeleteMacros,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Option<ErasedModel>>>> {
        Box::pin(async move {
            Ok(self
                .match_many_with(parents, transaction, |query| soft_deletes.apply(query))
                .await?
                .into_iter()
                .map(|models| models.into_iter().last().map(|model| model as ErasedModel))
                .collect())
        })
    }
}

impl<P: Model> Relation for MorphTo<P> {
    type Parent = P;
    type Output = Option<MorphTarget>;

    fn attach_autoload(outputs: &mut [Self::Output]) {
        let mut groups = IndexMap::<_, Vec<_>>::new();
        for target in outputs.iter_mut().flatten() {
            groups.entry(target.alias()).or_default().push(target);
        }
        for mut group in groups.into_values() {
            let representative = group[0].model.clone();
            representative.attach_autoload(&mut group);
        }
    }

    fn push_loaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(async move {
            let Some(target) = output else {
                return (None, Ok(true));
            };
            let (model, result) = target.model.clone().push_snapshot(context).await;
            (
                Some(Some(MorphTarget {
                    alias: target.alias,
                    model,
                })),
                result,
            )
        })
    }

    fn autoload_output(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        output
            .as_ref()
            .map_or(Ok(serde_json::Value::Null), |target| target.model.project())
            .map(Some)
    }

    fn queueable_relations(output: &Self::Output) -> Vec<String> {
        output
            .as_ref()
            .map_or_else(Vec::new, |target| target.model.queueable_relations())
    }

    fn get_results<'a>(
        &'a self,
        parent: &'a P,
    ) -> DatabaseFuture<'a, DatabaseResult<Self::Output>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            async move {
                if let Some((alias, target)) = self.selected_target(parent)?
                    && let Some(model) = target.load_first(parent, self.soft_deletes).await?
                {
                    return Ok(Some(MorphTarget { alias, model }));
                }
                self.default_target(parent)
            },
        ))
    }

    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, None),
        ))
    }

    fn match_results_in_transaction<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, Some(transaction)),
        ))
    }
}

impl<P: Model> MorphTo<P> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        Box::pin(self.match_targets(parents, transaction))
    }
}
