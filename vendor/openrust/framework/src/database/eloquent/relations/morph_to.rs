use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use indexmap::IndexMap;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, Model, NamedRelations, named_relations::DiscoveredMorphs,
        touches::TouchedOwners,
    },
    query,
};

use super::{
    ExistenceRelation, Relation,
    relation::{RelationKeys, dictionary_key},
};

/// An inverse polymorphic relationship whose aliases resolve to registered model types.
pub struct MorphTo<P: Model> {
    type_column: &'static str,
    id_column: &'static str,
    targets: IndexMap<&'static str, Box<dyn Target<P>>>,
}

/// A polymorphic model loaded from a registered alias.
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
    pub fn alias(&self) -> &'static str {
        self.alias
    }

    pub fn downcast_ref<M: Model>(&self) -> Option<&M> {
        self.model.as_ref().as_any().downcast_ref()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's polymorphic query API")]
impl<P: Model> MorphTo<P> {
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
            targets: IndexMap::new(),
        }
    }

    #[must_use]
    pub fn target<R: Model>(self, alias: &'static str, owner_key: &'static str) -> Self {
        self.constrain::<R>(alias, owner_key, |query| query)
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
        let mut keys = RelationKeys::new(self.id_column, owner_key);
        keys.try_constrain(configure);
        self.targets.insert(alias, Box::new(keys));
        self
    }

    #[must_use]
    pub fn morphWithCount<R: Model>(
        self,
        alias: &'static str,
        owner_key: &'static str,
        name: &'static str,
        relation: impl ExistenceRelation<Parent = R>,
    ) -> Self {
        self.tryConstrain::<R>(alias, owner_key, move |query| {
            query.withCount(name, &relation)
        })
    }

    pub(in crate::database::eloquent) fn with_paths(
        &mut self,
        children: &[&str],
    ) -> DatabaseResult<()> {
        if !children.is_empty() {
            for target in self.targets.values_mut() {
                target.with_paths(children)?;
            }
        }
        Ok(())
    }

    pub(in crate::database::eloquent) fn filter_path(
        &self,
        connection: &Connection,
        parent: query::Builder<'static>,
        child: &str,
        operator: &str,
        count: u64,
        discovered: Option<(&DiscoveredMorphs, &str)>,
    ) -> DatabaseResult<query::Builder<'static>> {
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
                    op,
                    n,
                    discovered.map(|(found, _)| found),
                )?;
                Ok((alias, query))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        if absent {
            parent.where_not_morph_path(&column, queries)
        } else if child.is_empty() {
            parent.where_morph_count(&column, queries, op, n, false)
        } else {
            parent.where_morph_count(&column, queries, ">=", 1, false)
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
        let target = self
            .targets
            .get(alias)
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
        model
            .setAttribute(self.type_column, alias)
            .setAttribute(self.id_column, key.clone());
        model.state_mut().relations.clear();
        Ok(())
    }

    pub fn dissociate(&self, model: &mut P) {
        model
            .setAttribute(self.type_column, Value::Null)
            .setAttribute(self.id_column, Value::Null);
        model.state_mut().relations.clear();
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

    async fn match_targets(&self, parents: &[&P]) -> DatabaseResult<Vec<Option<MorphTarget>>> {
        let mut groups = IndexMap::<&str, Vec<(usize, &P)>>::new();
        for (index, parent) in parents.iter().copied().enumerate() {
            if dictionary_key(parent.getAttribute(self.id_column))?.is_none() {
                continue;
            }
            match parent.getAttribute(self.type_column) {
                None | Some(Value::Null) => (),
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
            let (indexes, parents): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
            let loaded = target.load(&parents).await?;
            for (index, model) in indexes.into_iter().zip(loaded) {
                matches[index] = model.map(|model| MorphTarget { alias, model });
            }
        }
        Ok(matches)
    }
}

type ErasedModel = Arc<dyn ProjectableModel>;

trait ProjectableModel: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn project(&self) -> DatabaseResult<serde_json::Value>;
}

impl<M: Model> ProjectableModel for M {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn project(&self) -> DatabaseResult<serde_json::Value> {
        self.toArray().map(serde_json::Value::Object)
    }
}

trait Target<P: Model>: Send + Sync {
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
    fn with_paths(&mut self, children: &[&str]) -> DatabaseResult<()>;
    fn path_query(
        &self,
        connection: &Connection,
        parent: &str,
        child: &str,
        operator: &str,
        count: u64,
        discovered: Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<query::Builder<'static>>;
    fn load<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Option<ErasedModel>>>>;
    fn touch<'a>(
        &'a self,
        model: &'a P,
        attributes: &'a Attributes,
        visited: &'a mut TouchedOwners,
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

impl<P: Model, R: Model> Target<P> for RelationKeys<P, R> {
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

    fn with_paths(&mut self, children: &[&str]) -> DatabaseResult<()> {
        self.with_nested(NamedRelations::<R>::prepare(children)?);
        Ok(())
    }

    fn path_query(
        &self,
        connection: &Connection,
        parent: &str,
        child: &str,
        operator: &str,
        count: u64,
        discovered: Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<query::Builder<'static>> {
        let query = self.existence_query(connection, parent)?;
        if child.is_empty() {
            Ok(query)
        } else {
            NamedRelations::<R>::filter(connection, query, child, operator, count, discovered)
        }
    }

    fn load<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Option<ErasedModel>>>> {
        Box::pin(async move {
            Ok(self
                .match_many(parents)
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

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        output
            .as_ref()
            .map_or(Ok(serde_json::Value::Null), |target| target.model.project())
            .map(Some)
    }

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(self.match_targets(parents))
    }
}
