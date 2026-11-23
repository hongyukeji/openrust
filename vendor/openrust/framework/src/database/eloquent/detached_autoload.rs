use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

use indexmap::IndexMap;
use tokio::sync::Mutex;

use crate::database::DatabaseResult;

use super::{
    Model, ModelState, RelatedModels,
    concerns::has_relationships::{LoadedRelations, load, load_on_access},
    relations::Relation,
};

#[derive(Clone)]
pub(super) struct DetachedAutoload {
    batch: Arc<dyn Any + Send + Sync>,
    published: Arc<OnceLock<Box<PublishedRelation>>>,
    snapshot: Arc<ModelState>,
    index: usize,
    invalidated: HashSet<String>,
}

struct Batch<M> {
    peers: Mutex<Vec<M>>,
}

// Immutable entries keep borrowed relationship values valid while another model
// publishes a different relationship. The batch mutex serializes publication.
struct PublishedRelation {
    name: String,
    values: Vec<LoadedRelations>,
    next: OnceLock<Box<PublishedRelation>>,
}

pub(super) fn attach<M: Model>(models: &mut [M]) {
    if models.is_empty()
        || models
            .iter()
            .all(|model| model.state().detached_autoload.is_some())
    {
        return;
    }
    let peers = models
        .iter()
        .map(|model| {
            let mut state = model.state().clone();
            state.detached_autoload = None;
            M::from_state(state)
        })
        .collect();
    let published = Arc::new(OnceLock::new());
    let batch = Arc::new(Batch {
        peers: Mutex::new(peers),
    });
    for (index, model) in models.iter_mut().enumerate() {
        if model.state().detached_autoload.is_none() {
            let snapshot = Arc::new(model.state().clone());
            model.state_mut().detached_autoload = Some(Box::new(DetachedAutoload {
                batch: batch.clone(),
                published: published.clone(),
                snapshot,
                index,
                invalidated: HashSet::new(),
            }));
        }
    }
}

/// Groups child models by shared instance before attaching the next batch.
pub(in crate::database::eloquent) fn attach_outputs<O: RelatedModels>(outputs: &mut [O]) {
    let mut models = IndexMap::new();
    for child in outputs.iter().flat_map(RelatedModels::models) {
        models
            .entry(Arc::as_ptr(child).addr())
            .or_insert_with(|| O::Model::from_state(child.state().clone()));
    }
    let (addresses, mut models): (Vec<_>, Vec<_>) = models.into_iter().unzip();
    attach(&mut models);
    let replacements: HashMap<_, _> = addresses
        .into_iter()
        .zip(models.into_iter().map(Arc::new))
        .collect();
    for output in outputs {
        if let Ok(replacement) =
            output.replace_models(|model| replacements[&Arc::as_ptr(model).addr()].clone())
        {
            *output = replacement;
        }
    }
}

impl DetachedAutoload {
    pub(super) fn invalidate(&mut self, name: &str) {
        self.invalidated.insert(name.to_owned());
    }

    fn entries(&self) -> impl Iterator<Item = &PublishedRelation> {
        std::iter::successors(self.published.get().map(Box::as_ref), |entry| {
            entry.next.get().map(Box::as_ref)
        })
    }

    fn can_share(&self, state: &ModelState, name: &str) -> bool {
        !self.invalidated.contains(name)
            && same_source(state, &self.snapshot)
            && (!state.relations.contains(name)
                || state.relations.same_value(name, &self.snapshot.relations))
    }

    pub(super) fn value<'a>(
        &'a self,
        state: &ModelState,
        name: &str,
    ) -> Option<&'a LoadedRelations> {
        let entry = self.entries().find(|entry| entry.name == name)?;
        self.can_share(state, name)
            .then_some(&entry.values[self.index])
    }

    pub(super) fn project(
        &self,
        state: &ModelState,
        include: impl Fn(&str) -> bool,
    ) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        self.visible_relations(state, &include).project(include)
    }

    pub(super) fn visible_relations(
        &self,
        state: &ModelState,
        include: impl Fn(&str) -> bool,
    ) -> LoadedRelations {
        let mut relations = state.relations.clone();
        for entry in self.entries() {
            if include(&entry.name) && self.can_share(state, &entry.name) {
                relations.copy_from(&entry.name, &entry.values[self.index]);
            }
        }
        relations
    }
}

pub(super) async fn load_on_model_access<M: Model, R: Relation<Parent = M>>(
    model: &mut M,
    name: &str,
    relation: &R,
) -> DatabaseResult<()> {
    let Some(link) = model.state().detached_autoload.as_ref() else {
        return if model.relation_loaded(name) {
            Ok(())
        } else {
            load_on_access(model, name, relation).await
        };
    };
    let Some(batch) = link.batch.downcast_ref::<Batch<M>>() else {
        return if model.relation_loaded(name) {
            Ok(())
        } else {
            load_on_access(model, name, relation).await
        };
    };
    if !link.can_share(model.state(), name) {
        return load(std::slice::from_mut(model), name, relation, true).await;
    }
    let mut peers = batch.peers.lock().await;
    let mut tail = link.published.as_ref();
    for entry in link.entries() {
        if entry.name == name {
            return Ok(());
        }
        tail = &entry.next;
    }
    load(&mut peers, name, relation, true).await?;
    if peers[link.index].get_relation::<R::Output>(name).is_some() {
        propagate::<M, R>(&mut peers, name);
        let values = peers
            .iter()
            .map(|peer| peer.state().relations.only(name))
            .collect();
        assert!(
            tail.set(Box::new(PublishedRelation {
                name: name.to_owned(),
                values,
                next: OnceLock::new()
            }))
            .is_ok()
        );
    }
    Ok(())
}

fn propagate<M: Model, R: Relation<Parent = M>>(models: &mut [M], name: &str) {
    let (positions, mut outputs): (Vec<_>, Vec<_>) = models
        .iter()
        .enumerate()
        .filter_map(|(index, model)| {
            Some((
                index,
                R::autoload_output(model.get_relation::<R::Output>(name)?)?,
            ))
        })
        .unzip();
    R::attach_autoload(&mut outputs);
    for (index, output) in positions.into_iter().zip(outputs) {
        models[index]
            .state_mut()
            .relations
            .insert_relation::<R>(name.to_owned(), output);
    }
}

fn same_source(left: &ModelState, right: &ModelState) -> bool {
    left.attributes == right.attributes
        && match (&left.connection, &right.connection) {
            (Some(left), Some(right)) => left.same_query_source(right),
            (None, None) => true,
            _ => false,
        }
}
