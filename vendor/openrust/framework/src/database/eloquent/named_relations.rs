use std::{any::TypeId, collections::HashMap, sync::Arc};

use indexmap::IndexMap;

use crate::database::{Connection, DatabaseError, DatabaseFuture, DatabaseResult, query};

use super::{
    EagerLoads, Model, RelatedModels,
    concerns::has_relationships::load,
    relations::{ExistenceRelation, NestedRelation, Relation},
};

type Prepare<M> =
    dyn Fn(EagerLoads<M>, &str, &[&str]) -> DatabaseResult<EagerLoads<M>> + Send + Sync;

type Existence = dyn Fn(
        &Connection,
        query::Builder<'static>,
        &str,
        &str,
        u64,
        Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<query::Builder<'static>>
    + Send
    + Sync;

type Discover = dyn for<'a> Fn(
        &'a Connection,
        &'a str,
        &'a str,
        &'a mut DiscoveredMorphs,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;

#[derive(Default)]
pub(super) struct DiscoveredMorphs(HashMap<(TypeId, String), Vec<&'static str>>);

impl DiscoveredMorphs {
    pub(super) fn get<M: Model>(&self, name: &str) -> Option<&[&'static str]> {
        self.0
            .get(&(TypeId::of::<M>(), name.to_owned()))
            .map(Vec::as_slice)
    }

    pub(super) fn insert<M: Model>(&mut self, name: &str, aliases: Vec<&'static str>) {
        self.0.insert((TypeId::of::<M>(), name.to_owned()), aliases);
    }
}

type LoadMissing<M> = dyn for<'a> Fn(&'a mut [M], &'a str, &'a [&'a str]) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;

struct NamedRelation<M: Model> {
    prepare: Box<Prepare<M>>,
    load_missing: Box<LoadMissing<M>>,
    existence: Option<Box<Existence>>,
    discover: Option<Box<Discover>>,
}

/// The relation factories available for eager loading and querying by name.
pub struct NamedRelations<M: Model>(IndexMap<&'static str, NamedRelation<M>>);

impl<M: Model> Default for NamedRelations<M> {
    fn default() -> Self {
        Self(IndexMap::new())
    }
}

impl<M: Model> NamedRelations<M> {
    /// Registers a relation with a single related model type for nested paths.
    #[must_use]
    pub fn nested<R: NestedRelation<Parent = M> + ExistenceRelation>(
        mut self,
        name: &'static str,
        factory: impl Fn() -> R + Send + Sync + 'static,
    ) -> Self
    where
        R::Output: RelatedModels<Model = R::Related>,
    {
        let factory = std::sync::Arc::new(factory);
        let eager = factory.clone();
        let missing = factory.clone();
        self.0.insert(
            name,
            NamedRelation {
                prepare: Box::new(move |loads, name, children| {
                    let relation = eager();
                    let relation = if children.is_empty() {
                        relation
                    } else {
                        relation.with_nested(NamedRelations::<R::Related>::prepare(children)?)
                    };
                    Ok(loads.with(name, relation))
                }),
                load_missing: Box::new(move |models, name, children| {
                    let relation = missing();
                    Box::pin(async move {
                        load(models, name, &relation, true).await?;
                        if children.is_empty() {
                            return Ok(());
                        }
                        load_missing_children::<M, R>(models, name, children).await
                    })
                }),
                existence: Some(Box::new(
                    move |connection, parent, child, operator, count, discovered| {
                        let mut related = factory()
                            .getRelationExistenceQuery(connection, parent.relation_qualifier())?;
                        if !child.is_empty() {
                            let terminal = (operator, count) == ("<", 1);
                            let (op, n) = if terminal {
                                (">=", 1)
                            } else {
                                (operator, count)
                            };
                            related = NamedRelations::<R::Related>::filter(
                                connection, related, child, op, n, discovered,
                            )?;
                            return if terminal {
                                parent.whereNotExists(&related)
                            } else {
                                parent.whereExists(&related)
                            };
                        }
                        match (operator, count) {
                            (">=", 1) => parent.whereExists(&related),
                            ("<", 1) => parent.whereNotExists(&related),
                            _ => {
                                let related = related.select_related_aggregate("count", "*")?;
                                parent.where_related_count(&related, operator, count)
                            }
                        }
                    },
                )),
                discover: Some(Box::new(|connection, _, child, found| {
                    NamedRelations::<R::Related>::discover(connection, child, found)
                })),
            },
        );
        self
    }

    /// Registers a relation that can appear at the end of a path.
    #[must_use]
    pub fn leaf<R: Relation<Parent = M>>(
        mut self,
        name: &'static str,
        factory: impl Fn() -> R + Send + Sync + 'static,
    ) -> Self {
        let factory = Arc::new(factory);
        let eager = factory.clone();
        self.0.insert(
            name,
            NamedRelation {
                prepare: Box::new(move |loads, name, children| {
                    if !children.is_empty() {
                        return Err(DatabaseError::InvalidQuery(
                            "relationship path cannot be nested",
                        ));
                    }
                    Ok(loads.with(name, eager()))
                }),
                load_missing: Box::new(move |models, name, children| {
                    let relation = factory();
                    Box::pin(async move {
                        if !children.is_empty() {
                            return Err(DatabaseError::InvalidQuery(
                                "relationship path cannot be nested",
                            ));
                        }
                        load(models, name, &relation, true).await
                    })
                }),
                existence: None,
                discover: None,
            },
        );
        self
    }

    /// Registers an inverse polymorphic relation and its typed target paths.
    #[must_use]
    #[allow(non_snake_case, reason = "Matches Laravel's Eloquent relationship API")]
    pub fn morphTo(
        mut self,
        name: &'static str,
        factory: impl Fn() -> super::relations::MorphTo<M> + Send + Sync + 'static,
    ) -> Self {
        let factory = std::sync::Arc::new(factory);
        let eager = factory.clone();
        let missing = factory.clone();
        let discover = factory.clone();
        self.0.insert(
            name,
            NamedRelation {
                prepare: Box::new(move |loads, name, children| {
                    let mut relation = eager();
                    relation.with_paths(children)?;
                    Ok(loads.with(name, relation))
                }),
                load_missing: Box::new(move |models, name, children| {
                    let relation = missing();
                    Box::pin(async move {
                        load(models, name, &relation, true).await?;
                        relation.load_missing_paths(models, name, children).await
                    })
                }),
                existence: Some(Box::new(
                    move |connection, parent, child, operator, count, discovered| {
                        factory().filter_path(
                            connection,
                            parent,
                            child,
                            operator,
                            count,
                            discovered.map(|found| (found, name)),
                        )
                    },
                )),
                discover: Some(Box::new(move |connection, name, child, found| {
                    let relation = discover();
                    Box::pin(
                        async move { relation.discover_path(connection, name, child, found).await },
                    )
                })),
            },
        );
        self
    }

    pub(super) fn filter(
        connection: &Connection,
        parent: query::Builder<'static>,
        path: &str,
        operator: &str,
        count: u64,
        discovered: Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<query::Builder<'static>> {
        let (name, child) = path.split_once('.').unwrap_or((path, ""));
        let registered = M::namedRelations();
        let relation = registered
            .0
            .get(name)
            .ok_or(DatabaseError::InvalidQuery("unknown relationship path"))?;
        let existence = relation
            .existence
            .as_ref()
            .ok_or(DatabaseError::InvalidQuery(
                "relationship path does not support existence queries",
            ))?;
        existence(connection, parent, child, operator, count, discovered)
    }

    pub(super) fn discover<'a>(
        connection: &'a Connection,
        path: &'a str,
        found: &'a mut DiscoveredMorphs,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if path.is_empty() {
                return Ok(());
            }
            let (name, child) = path.split_once('.').unwrap_or((path, ""));
            let registered = M::namedRelations();
            let relation = registered
                .0
                .get(name)
                .ok_or(DatabaseError::InvalidQuery("unknown relationship path"))?;
            let discover = relation
                .discover
                .as_ref()
                .ok_or(DatabaseError::InvalidQuery(
                    "relationship path does not support existence queries",
                ))?;
            discover(connection, name, child, found).await
        })
    }

    pub(super) fn load_missing<'a>(
        models: &'a mut [M],
        paths: &'a [&'a str],
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let grouped = group_paths(paths)?;
            let registered = M::namedRelations();
            for (name, children) in grouped {
                let relation = registered
                    .0
                    .get(name)
                    .ok_or(DatabaseError::InvalidQuery("unknown relationship path"))?;
                (relation.load_missing)(models, name, &children).await?;
            }
            Ok(())
        })
    }

    pub(super) fn prepare(paths: &[&str]) -> DatabaseResult<EagerLoads<M>> {
        let grouped = group_paths(paths)?;
        let registered = M::namedRelations();
        let mut loads = EagerLoads::default();
        for (name, children) in grouped {
            let prepare = registered
                .0
                .get(name)
                .ok_or(DatabaseError::InvalidQuery("unknown relationship path"))?;
            loads = (prepare.prepare)(loads, name, &children)?;
        }
        Ok(loads)
    }
}

fn group_paths<'a>(paths: &[&'a str]) -> DatabaseResult<IndexMap<&'a str, Vec<&'a str>>> {
    let mut grouped = IndexMap::<&str, Vec<&str>>::new();
    for path in paths {
        if path.split('.').any(str::is_empty) {
            return Err(DatabaseError::InvalidQuery("invalid relationship path"));
        }
        if let Some((name, child)) = path.split_once('.') {
            grouped.entry(name).or_default().push(child);
        } else {
            grouped.entry(path).or_default();
        }
    }

    Ok(grouped)
}

/// Loads children once per shared model instance and maps each original pointer to its replacement.
pub(super) async fn load_missing_models<'a, M: Model>(
    models: impl Iterator<Item = &'a M> + Send,
    children: &[&str],
) -> DatabaseResult<HashMap<usize, Arc<M>>> {
    let mut positions = IndexMap::new();
    for model in models {
        positions
            .entry(std::ptr::from_ref(model).addr())
            .or_insert_with(|| M::from_state(model.state().clone()));
    }
    let (keys, mut copies): (Vec<_>, Vec<_>) = positions.into_iter().unzip();
    NamedRelations::<M>::load_missing(&mut copies, children).await?;
    Ok(keys
        .into_iter()
        .zip(copies.into_iter().map(Arc::new))
        .collect())
}

async fn load_missing_children<M, R>(
    models: &mut [M],
    name: &str,
    children: &[&str],
) -> DatabaseResult<()>
where
    M: Model,
    R: NestedRelation<Parent = M>,
    R::Output: RelatedModels<Model = R::Related>,
{
    let outputs = models
        .iter()
        .map(|model| {
            model
                .getRelation::<R::Output>(name)
                .ok_or(DatabaseError::InvalidQuery("relationship output type"))
        })
        .collect::<DatabaseResult<Vec<_>>>()?;
    let replacements = load_missing_models(
        outputs
            .iter()
            .flat_map(|output| output.models())
            .map(Arc::as_ref),
        children,
    )
    .await?;
    let outputs = outputs
        .into_iter()
        .map(|output| {
            output.replace_models(|model| replacements[&Arc::as_ptr(model).addr()].clone())
        })
        .collect::<DatabaseResult<Vec<_>>>()?;
    for (model, output) in models.iter_mut().zip(outputs) {
        model
            .state_mut()
            .relations
            .insert(name.to_owned(), output, R::serialize);
    }
    Ok(())
}
