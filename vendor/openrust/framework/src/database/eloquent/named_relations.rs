use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};

use indexmap::IndexMap;

use crate::database::{Connection, DatabaseError, DatabaseFuture, DatabaseResult, query};

use super::{
    EagerLoads, Model, RelatedModels,
    concerns::has_relationships::load,
    relations::{ExistenceRelation, NestedRelation, Relation},
};

pub(super) type PathConstraint =
    Arc<dyn Fn(query::Conditions, &str) -> query::Conditions + Send + Sync>;

/// A registered relationship path and its optional terminal predicate.
pub struct MissingRelation<'a> {
    path: &'a str,
    constraint: Option<PathConstraint>,
}

impl<'a> MissingRelation<'a> {
    pub fn new(path: &'a str) -> Self {
        Self {
            path,
            constraint: None,
        }
    }

    /// Applies grouped conditions using the final relationship's table qualifier.
    #[must_use]
    pub fn constrain(
        mut self,
        constraint: impl Fn(query::Conditions, &str) -> query::Conditions + Send + Sync + 'static,
    ) -> Self {
        self.constraint = Some(Arc::new(constraint));
        self
    }
}

pub(super) type MissingPaths<'a> = IndexMap<&'a str, MissingPath<'a>>;

#[derive(Default)]
pub(super) struct MissingPath<'a> {
    columns: Option<Vec<String>>,
    constraint: Option<PathConstraint>,
    children: MissingPaths<'a>,
}

impl<'a> MissingPath<'a> {
    fn parse(
        paths: impl IntoIterator<Item = MissingRelation<'a>>,
    ) -> DatabaseResult<MissingPaths<'a>> {
        let mut roots = MissingPaths::new();
        for path in paths {
            let (names, columns) = split_path(path.path)?;
            let mut children = &mut roots;
            let mut names = names.split('.').peekable();
            while let Some(name) = names.next() {
                let node = children.entry(name).or_default();
                if names.peek().is_some() {
                    children = &mut node.children;
                    continue;
                }
                if let Some(columns) = columns {
                    node.columns.get_or_insert(parse_columns(columns)?);
                }
                if let Some(next) = path.constraint {
                    node.constraint = Some(match node.constraint.take() {
                        Some(previous) => Arc::new(move |conditions, qualifier| {
                            previous(next(conditions, qualifier), qualifier)
                        }),
                        None => next,
                    });
                }
                break;
            }
        }
        Ok(roots)
    }
}

type Prepare<M> = dyn Fn(
        EagerLoads<M>,
        &str,
        &[&str],
        Option<&[String]>,
        Option<&PathConstraint>,
        Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<EagerLoads<M>>
    + Send
    + Sync;

type Existence = dyn Fn(
        &Connection,
        &query::Builder<'static>,
        &str,
        &str,
        u64,
        Option<&DiscoveredMorphs>,
        Option<&PathConstraint>,
    ) -> DatabaseResult<query::Expression>
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

type LoadMissing<M> = dyn for<'a> Fn(&'a mut [M], &'a str, &'a MissingPath<'a>) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;

type Descriptor = dyn Fn() -> Box<dyn Any + Send + Sync> + Send + Sync;

struct NamedRelation<M: Model> {
    descriptor: Box<Descriptor>,
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
    pub(super) fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    pub(super) fn factory_descriptor<R: Relation<Parent = M>>(
        &self,
        name: &str,
    ) -> DatabaseResult<R> {
        let entry = self
            .0
            .get(name)
            .ok_or(DatabaseError::InvalidQuery("unknown factory relationship"))?;
        (entry.descriptor)()
            .downcast::<R>()
            .map(|relation| *relation)
            .map_err(|_| DatabaseError::InvalidQuery("factory relationship type mismatch"))
    }

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
        let descriptor = factory.clone();
        self.0.insert(
            name,
            NamedRelation {
                descriptor: Box::new(move || Box::new(descriptor())),
                prepare: Box::new(
                    move |loads, name, children, columns, constraint, discovered| {
                        let relation = select_columns(eager(), columns)?;
                        let relation = if children.is_empty() {
                            constrain_leaf(relation, constraint)?
                        } else {
                            relation.with_nested(NamedRelations::<R::Related>::prepare_constrained(
                                children, constraint, discovered,
                            )?)
                        };
                        Ok(loads.with(name, relation))
                    },
                ),
                load_missing: Box::new(move |models, name, path| {
                    let relation = missing();
                    Box::pin(async move {
                        let relation = select_columns(relation, path.columns.as_deref())?;
                        let relation = constrain_leaf(relation, path.constraint.as_ref())?;
                        load(models, name, &relation, true).await?;
                        if path.children.is_empty() {
                            return Ok(());
                        }
                        load_missing_children::<M, R>(models, name, &path.children).await
                    })
                }),
                existence: Some(Box::new(
                    move |connection, parent, child, operator, count, discovered, constraint| {
                        let mut related = factory()
                            .getRelationExistenceQuery(connection, parent.relation_qualifier())?;
                        if !child.is_empty() {
                            let terminal = (operator, count) == ("<", 1);
                            let (op, n) = if terminal {
                                (">=", 1)
                            } else {
                                (operator, count)
                            };
                            let predicate = NamedRelations::<R::Related>::filter(
                                connection,
                                &related,
                                child,
                                (op, n),
                                discovered,
                                constraint,
                            )?;
                            related = related.and_expression(predicate);
                            return parent.exists_expression(&related, terminal);
                        }
                        if let Some(constraint) = constraint {
                            let qualifier = related.relation_qualifier().to_owned();
                            related = related
                                .whereNested(|conditions| constraint(conditions, &qualifier));
                        }
                        match (operator, count) {
                            (">=", 1) => parent.exists_expression(&related, false),
                            ("<", 1) => parent.exists_expression(&related, true),
                            _ => {
                                let related = related.select_related_aggregate("count", "*")?;
                                parent.related_count_expression(&related, operator, count)
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
        let descriptor = factory.clone();
        self.0.insert(
            name,
            NamedRelation {
                descriptor: Box::new(move || Box::new(descriptor())),
                prepare: Box::new(move |loads, name, children, columns, constraint, _| {
                    if constraint.is_some() || columns.is_some() {
                        return Err(DatabaseError::InvalidQuery(
                            "relationship path does not support constraints",
                        ));
                    }
                    if !children.is_empty() {
                        return Err(DatabaseError::InvalidQuery(
                            "relationship path cannot be nested",
                        ));
                    }
                    Ok(loads.with(name, eager()))
                }),
                load_missing: Box::new(move |models, name, path| {
                    let relation = factory();
                    Box::pin(async move {
                        if path.constraint.is_some() {
                            return Err(DatabaseError::InvalidQuery(
                                "relationship path does not support constraints",
                            ));
                        }
                        if path.columns.is_some() {
                            return Err(DatabaseError::InvalidQuery(
                                "relationship path does not support column selection",
                            ));
                        }
                        if !path.children.is_empty() {
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
        let descriptor = factory.clone();
        self.0.insert(
            name,
            NamedRelation {
                descriptor: Box::new(move || Box::new(descriptor())),
                prepare: Box::new(
                    move |loads, name, children, columns, constraint, discovered| {
                        let mut relation = eager();
                        relation.select_path_columns(columns);
                        relation.with_constrained_paths(name, children, constraint, discovered)?;
                        Ok(loads.with(name, relation))
                    },
                ),
                load_missing: Box::new(move |models, name, path| {
                    let mut relation = missing();
                    relation.select_path_columns(path.columns.as_deref());
                    Box::pin(async move {
                        if let Some(constraint) = &path.constraint {
                            relation.with_constrained_paths(name, &[], Some(constraint), None)?;
                        }
                        load(models, name, &relation, true).await?;
                        relation
                            .load_missing_paths(models, name, &path.children)
                            .await
                    })
                }),
                existence: Some(Box::new(
                    move |connection, parent, child, operator, count, discovered, constraint| {
                        factory().filter_path(
                            connection,
                            parent,
                            child,
                            (operator, count),
                            discovered.map(|found| (found, name)),
                            constraint,
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
        parent: &query::Builder<'static>,
        path: &str,
        (operator, count): (&str, u64),
        discovered: Option<&DiscoveredMorphs>,
        constraint: Option<&PathConstraint>,
    ) -> DatabaseResult<query::Expression> {
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
        existence(
            connection, parent, child, operator, count, discovered, constraint,
        )
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

    pub(super) async fn load_missing(models: &mut [M], paths: &[&str]) -> DatabaseResult<()> {
        let paths = MissingPath::parse(paths.iter().map(|path| MissingRelation::new(path)))?;
        Self::load_missing_nodes(models, &paths).await
    }

    pub(super) async fn load_missing_with<'a>(
        models: &mut [M],
        paths: impl IntoIterator<Item = MissingRelation<'a>> + Send,
    ) -> DatabaseResult<()> {
        let paths = MissingPath::parse(paths)?;
        Self::load_missing_nodes(models, &paths).await
    }

    fn load_missing_nodes<'a>(
        models: &'a mut [M],
        paths: &'a MissingPaths<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let registered = M::namedRelations();
            for (name, path) in paths {
                let relation = registered
                    .0
                    .get(name)
                    .ok_or(DatabaseError::InvalidQuery("unknown relationship path"))?;
                (relation.load_missing)(models, name, path).await?;
            }
            Ok(())
        })
    }

    pub(super) fn prepare(paths: &[&str]) -> DatabaseResult<EagerLoads<M>> {
        Self::prepare_constrained(paths, None, None)
    }

    pub(super) fn prepare_constrained(
        paths: &[&str],
        constraint: Option<&PathConstraint>,
        discovered: Option<&DiscoveredMorphs>,
    ) -> DatabaseResult<EagerLoads<M>> {
        let grouped = group_paths(paths)?;
        let registered = M::namedRelations();
        let mut loads = EagerLoads::default();
        for (name, path) in grouped {
            let prepare = registered
                .0
                .get(name)
                .ok_or(DatabaseError::InvalidQuery("unknown relationship path"))?;
            loads = (prepare.prepare)(
                loads,
                name,
                &path.children,
                path.columns.as_deref(),
                constraint,
                discovered,
            )?;
        }
        Ok(loads)
    }
}

fn constrain_leaf<R: NestedRelation>(
    relation: R,
    constraint: Option<&PathConstraint>,
) -> DatabaseResult<R> {
    if let Some(constraint) = constraint.cloned() {
        relation
            .with_path_constraint(move |conditions, qualifier| constraint(conditions, qualifier))
    } else {
        Ok(relation)
    }
}

fn select_columns<R: NestedRelation>(relation: R, columns: Option<&[String]>) -> DatabaseResult<R> {
    match columns {
        Some(columns) => relation.with_path_columns(columns.to_vec()),
        None => Ok(relation),
    }
}

#[derive(Default)]
struct EagerPath<'a> {
    columns: Option<Vec<String>>,
    children: Vec<&'a str>,
}

fn group_paths<'a>(paths: &[&'a str]) -> DatabaseResult<IndexMap<&'a str, EagerPath<'a>>> {
    let mut grouped = IndexMap::<&str, EagerPath<'_>>::new();
    for path in paths {
        let (names, columns) = split_path(path)?;
        if let Some((name, _)) = names.split_once('.') {
            grouped
                .entry(name)
                .or_default()
                .children
                .push(&path[name.len() + 1..]);
        } else {
            let entry = grouped.entry(names).or_default();
            if let Some(columns) = columns {
                // Laravel evaluates the first projection last when a path is repeated.
                entry.columns.get_or_insert(parse_columns(columns)?);
            }
        }
    }

    Ok(grouped)
}

fn split_path(path: &str) -> DatabaseResult<(&str, Option<&str>)> {
    let (names, columns) = path
        .split_once(':')
        .map_or((path, None), |(name, columns)| (name, Some(columns)));
    if names.split('.').any(str::is_empty) {
        return Err(DatabaseError::InvalidQuery("invalid relationship path"));
    }
    Ok((names, columns))
}

fn parse_columns(columns: &str) -> DatabaseResult<Vec<String>> {
    let columns = columns.split(',').map(str::trim).collect::<Vec<_>>();
    if columns
        .iter()
        .any(|column| column.is_empty() || column.contains(':'))
    {
        return Err(DatabaseError::InvalidQuery("invalid relationship columns"));
    }
    Ok(columns.into_iter().map(str::to_owned).collect())
}

/// Loads children once per shared model instance and maps each original pointer to its replacement.
pub(super) async fn load_missing_models<'a, M: Model>(
    models: impl Iterator<Item = &'a M> + Send,
    children: &MissingPaths<'_>,
) -> DatabaseResult<HashMap<usize, Arc<M>>> {
    let mut positions = IndexMap::new();
    for model in models {
        positions
            .entry(std::ptr::from_ref(model).addr())
            .or_insert_with(|| M::from_state(model.state().clone()));
    }
    let (keys, mut copies): (Vec<_>, Vec<_>) = positions.into_iter().unzip();
    NamedRelations::<M>::load_missing_nodes(&mut copies, children).await?;
    Ok(keys
        .into_iter()
        .zip(copies.into_iter().map(Arc::new))
        .collect())
}

async fn load_missing_children<M, R>(
    models: &mut [M],
    name: &str,
    children: &MissingPaths<'_>,
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
            .insert_relation::<R>(name.to_owned(), output);
    }
    Ok(())
}
