use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use indexmap::IndexMap;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, EagerLoads, Model, NamedRelations,
        concerns::has_relationships::update_association_cache,
        named_relations::{DiscoveredMorphs, PathConstraint},
        touches::TouchedOwners,
    },
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
            query = query.withTrashed();
            let column = query.qualifyColumn(column);
            if self.require_live {
                query.whereNested(|conditions| conditions.whereNull(&column));
            }
            if self.require_deleted {
                query.whereNested(|conditions| conditions.whereNotNull(&column));
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

#[allow(non_snake_case, reason = "Matches Laravel's polymorphic query API")]
impl<P: Model> MorphTo<P> {
    /// Resolves the stored morph alias before comparing the owner key.
    pub fn is<R: Model>(&self, parent: &P, related: Option<&R>) -> bool {
        let Some(Value::Text(alias)) = parent.getAttribute(self.type_column) else {
            return false;
        };
        let Some(target) = self.targets.get(alias.as_str()) else {
            return false;
        };
        target.model_type() == TypeId::of::<R>()
            && super::BelongsTo::<P, R>::new(self.id_column, target.owner_key()).is(parent, related)
    }

    pub fn isNot<R: Model>(&self, parent: &P, related: Option<&R>) -> bool {
        !self.is(parent, related)
    }

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
        let alias = match parent.getAttribute(self.type_column) {
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

    pub fn identity<R: Model>(&self, alias: &str, model: &R) -> DatabaseResult<MorphIdentity> {
        self.validate_target::<R>(alias)?;
        let (&alias, target) = self
            .targets
            .get_key_value(alias)
            .ok_or(DatabaseError::InvalidQuery("unknown morph type"))?;
        let owner_key = target.owner_key();
        let key = model
            .getAttribute(owner_key)
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

    /// Names the cache entry updated by association and dissociation.
    #[must_use]
    pub fn named(mut self, name: &'static str) -> Self {
        self.relation_name = Some(name);
        self
    }

    /// Supplies an unsaved model when this registered target has no matching row.
    pub fn withDefault<R: Model>(
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
    pub fn defaultForMissingType(mut self, alias: &'static str) -> DatabaseResult<Self> {
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
    pub fn withTrashed(mut self) -> Self {
        self.soft_deletes.remove_scope = true;
        self
    }

    /// Requires targets with a soft-delete column to be live.
    #[must_use]
    pub fn withoutTrashed(mut self) -> Self {
        self.soft_deletes.require_live = true;
        self.withTrashed()
    }

    /// Requires targets with a soft-delete column to be deleted.
    #[must_use]
    pub fn onlyTrashed(mut self) -> Self {
        self.soft_deletes.require_deleted = true;
        self.withTrashed()
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
        self.tryConstrain::<R>(alias, owner_key, move |query| Ok(configure(query)))
    }

    #[must_use]
    pub fn tryConstrain<R: Model>(
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
    pub fn morphWith<R: Model>(
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
    pub fn morphWithPaths<R: Model>(self, alias: &str, paths: &[&str]) -> DatabaseResult<Self> {
        let loads = NamedRelations::<R>::prepare(paths)?;
        self.morphWith(alias, |_| loads)
    }

    #[must_use]
    pub fn morphWithCount<R: Model>(
        mut self,
        alias: &'static str,
        owner_key: &'static str,
        name: &'static str,
        relation: impl ExistenceRelation<Parent = R>,
    ) -> Self {
        let configure = move |query: Builder<'static, R>| query.withCount(name, &relation);
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
        children: &[&str],
    ) -> DatabaseResult<()> {
        if children.is_empty() {
            return Ok(());
        }
        let mut outputs = models
            .iter()
            .map(|model| {
                model
                    .getRelation::<Option<MorphTarget>>(name)
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
            .getAttribute(target.owner_key())
            .filter(|value| !matches!(value, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship owner key"))?;
        update_association_cache::<Self>(model, self.relation_name, || {
            Some(Some(MorphTarget {
                alias,
                model: Arc::new(R::from_state(related.state().clone())),
            }))
        });
        model
            .setAttribute(self.type_column, alias)
            .setAttribute(self.id_column, key.clone());
        Ok(())
    }

    pub fn dissociate(&self, model: &mut P) {
        update_association_cache::<Self>(model, self.relation_name, || Some(None));
        model
            .setAttribute(self.type_column, Value::Null)
            .setAttribute(self.id_column, Value::Null);
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

    async fn match_targets(
        &self,
        parents: &[&P],
        mut transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<Vec<Option<MorphTarget>>> {
        let mut groups = IndexMap::<&str, Vec<(usize, &P)>>::new();
        for (index, parent) in parents.iter().copied().enumerate() {
            if dictionary_key(parent.getAttribute(self.id_column))?.is_none() {
                continue;
            }
            match parent.getAttribute(self.type_column) {
                None | Some(Value::Null) => (),
                Some(Value::Text(alias)) if alias.is_empty() && self.untyped_default.is_some() => {}
                Some(Value::Text(alias)) => groups.entry(alias).or_default().push((index, parent)),
                _ => return Err(DatabaseError::InvalidQuery("morph type column")),
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
                if matched.is_some() {
                    continue;
                }
                let alias = match parent.getAttribute(self.type_column) {
                    None | Some(Value::Null) => self.untyped_default,
                    Some(Value::Text(alias)) if alias.is_empty() => self.untyped_default,
                    Some(Value::Text(alias)) => Some(alias.as_str()),
                    _ => None,
                };
                if let Some((alias, create)) =
                    alias.and_then(|alias| self.defaults.get_key_value(alias))
                {
                    if self
                        .path_aliases
                        .as_ref()
                        .is_some_and(|aliases| !aliases.contains(alias))
                    {
                        continue;
                    }
                    *matched = Some(MorphTarget {
                        alias,
                        model: create(parent)?,
                    });
                }
            }
        }
        Ok(matches)
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
        self.toArray().map(serde_json::Value::Object)
    }

    fn queueable_relations(&self) -> Vec<String> {
        self.getQueueableRelations()
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
        children: &'a [&'a str],
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
            let query = self.configure(R::queryOn(&connection))?;
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
        children: &'a [&'a str],
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
            Ok(query.whereNested(|conditions| constraint(conditions, &qualifier)))
        } else {
            let predicate = NamedRelations::<R>::filter(
                connection, &query, child, comparison, discovered, constraint,
            )?;
            Ok(query.and_expression(predicate))
        }
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

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model> Relation for MorphTo<P> {
    type Parent = P;
    type Output = Option<MorphTarget>;

    fn attachAutoload(outputs: &mut [Self::Output]) {
        let mut groups = IndexMap::<_, Vec<_>>::new();
        for target in outputs.iter_mut().flatten() {
            groups.entry(target.alias()).or_default().push(target);
        }
        for mut group in groups.into_values() {
            let representative = group[0].model.clone();
            representative.attach_autoload(&mut group);
        }
    }

    fn pushLoaded<'a>(
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

    fn autoloadOutput(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        output
            .as_ref()
            .map_or(Ok(serde_json::Value::Null), |target| target.model.project())
            .map(Some)
    }

    fn queueableRelations(output: &Self::Output) -> Vec<String> {
        output
            .as_ref()
            .map_or_else(Vec::new, |target| target.model.queueable_relations())
    }

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.match_results(parents, None)
    }

    fn matchResultsInTransaction<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        self.match_results(parents, Some(transaction))
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
