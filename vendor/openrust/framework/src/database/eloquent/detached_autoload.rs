use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::Arc,
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
    index: usize,
    invalidated: HashSet<String>,
}

struct Batch<M> {
    state: Mutex<BatchState<M>>,
}

struct BatchState<M> {
    peers: Vec<M>,
    propagated: HashMap<String, Vec<LoadedRelations>>,
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
    let batch = Arc::new(Batch {
        state: Mutex::new(BatchState {
            peers,
            propagated: HashMap::new(),
        }),
    });
    for (index, model) in models.iter_mut().enumerate() {
        model.state_mut().detached_autoload.get_or_insert_with(|| {
            Box::new(DetachedAutoload {
                batch: batch.clone(),
                index,
                invalidated: HashSet::new(),
            })
        });
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
}

pub(super) async fn load_on_model_access<M: Model, R: Relation<Parent = M>>(
    model: &mut M,
    name: &str,
    relation: &R,
) -> DatabaseResult<()> {
    let Some(link) = model.state().detached_autoload.as_ref() else {
        return if model.relationLoaded(name) {
            Ok(())
        } else {
            load_on_access(model, name, relation).await
        };
    };
    let Some(batch) = link.batch.downcast_ref::<Batch<M>>() else {
        return if model.relationLoaded(name) {
            Ok(())
        } else {
            load_on_access(model, name, relation).await
        };
    };
    let local_value = model.state().relations.only(name);
    let loaded = {
        let mut state = batch.state.lock().await;
        if link.invalidated.contains(name)
            || (local_value.contains(name)
                && !local_value.same_value(name, &state.peers[link.index].state().relations)
                && !state
                    .propagated
                    .get(name)
                    .is_some_and(|original| local_value.same_value(name, &original[link.index])))
            || !same_source(model.state(), state.peers[link.index].state())
        {
            None
        } else {
            load(&mut state.peers, name, relation, true).await?;
            if state.peers[link.index]
                .getRelation::<R::Output>(name)
                .is_some()
                && !state.propagated.contains_key(name)
            {
                let original = state
                    .peers
                    .iter()
                    .map(|peer| peer.state().relations.only(name))
                    .collect();
                propagate::<M, R>(&mut state.peers, name);
                state.propagated.insert(name.to_owned(), original);
            }
            Some(state.peers[link.index].state().relations.only(name))
        }
    };
    if let Some(loaded) = loaded {
        let local: &mut LoadedRelations = &mut model.state_mut().relations;
        local.copy_from(name, &loaded);
        Ok(())
    } else {
        let models = std::slice::from_mut(model);
        load(models, name, relation, true).await?;
        Ok(())
    }
}

fn propagate<M: Model, R: Relation<Parent = M>>(models: &mut [M], name: &str) {
    let (positions, mut outputs): (Vec<_>, Vec<_>) = models
        .iter()
        .enumerate()
        .filter_map(|(index, model)| {
            Some((
                index,
                R::autoloadOutput(model.getRelation::<R::Output>(name)?)?,
            ))
        })
        .unzip();
    R::attachAutoload(&mut outputs);
    for (index, output) in positions.into_iter().zip(outputs) {
        models[index]
            .state_mut()
            .relations
            .insert(name.to_owned(), output, R::serialize);
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
