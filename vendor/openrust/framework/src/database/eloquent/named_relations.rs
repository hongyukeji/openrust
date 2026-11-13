use indexmap::IndexMap;

use crate::database::{DatabaseError, DatabaseResult};

use super::{
    EagerLoads, Model,
    relations::{NestedRelation, Relation},
};

type Prepare<M> =
    dyn Fn(EagerLoads<M>, &str, &[&str]) -> DatabaseResult<EagerLoads<M>> + Send + Sync;

/// The relation factories available for eager loading by name.
pub struct NamedRelations<M: Model>(IndexMap<&'static str, Box<Prepare<M>>>);

impl<M: Model> Default for NamedRelations<M> {
    fn default() -> Self {
        Self(IndexMap::new())
    }
}

impl<M: Model> NamedRelations<M> {
    /// Registers a relation with a single related model type for nested paths.
    #[must_use]
    pub fn nested<R: NestedRelation<Parent = M>>(
        mut self,
        name: &'static str,
        factory: impl Fn() -> R + Send + Sync + 'static,
    ) -> Self {
        self.0.insert(
            name,
            Box::new(move |loads, name, children| {
                let relation = factory();
                let relation = if children.is_empty() {
                    relation
                } else {
                    relation.with_nested(NamedRelations::<R::Related>::prepare(children)?)
                };
                Ok(loads.with(name, relation))
            }),
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
        self.0.insert(
            name,
            Box::new(move |loads, name, children| {
                if !children.is_empty() {
                    return Err(DatabaseError::InvalidQuery(
                        "relationship path cannot be nested",
                    ));
                }
                Ok(loads.with(name, factory()))
            }),
        );
        self
    }

    pub(super) fn prepare(paths: &[&str]) -> DatabaseResult<EagerLoads<M>> {
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

        let registered = M::namedRelations();
        let mut loads = EagerLoads::default();
        for (name, children) in grouped {
            let prepare = registered
                .0
                .get(name)
                .ok_or(DatabaseError::InvalidQuery("unknown relationship path"))?;
            loads = prepare(loads, name, &children)?;
        }
        Ok(loads)
    }
}
