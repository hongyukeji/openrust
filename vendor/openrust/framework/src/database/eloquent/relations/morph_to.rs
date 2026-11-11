use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use indexmap::IndexMap;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Value,
    eloquent::{Builder, Model},
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
        self.model.as_ref().downcast_ref()
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

type ErasedModel = Arc<dyn Any + Send + Sync>;

trait Target<P: Model>: Send + Sync {
    fn model_type(&self) -> TypeId;
    fn owner_key(&self) -> &'static str;
    fn existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>>;
    fn load<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Option<ErasedModel>>>>;
}

impl<P: Model, R: Model> Target<P> for RelationKeys<P, R> {
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

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(self.match_targets(parents))
    }
}
