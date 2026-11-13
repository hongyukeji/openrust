use std::{any::Any, collections::HashSet, sync::Arc};

use tokio::sync::Mutex;

use crate::database::DatabaseResult;

use super::{
    Model, ModelState,
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
    peers: Mutex<Vec<M>>,
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
        peers: Mutex::new(peers),
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
        return load_on_access(model, name, relation).await;
    };
    let Some(batch) = link.batch.downcast_ref::<Batch<M>>() else {
        return load_on_access(model, name, relation).await;
    };
    let loaded = {
        let mut peers = batch.peers.lock().await;
        if link.invalidated.contains(name) || !same_source(model.state(), peers[link.index].state())
        {
            None
        } else {
            load(&mut peers, name, relation, true).await?;
            Some(peers[link.index].state().relations.clone())
        }
    };
    if let Some(loaded) = loaded {
        let local: &mut LoadedRelations = &mut model.state_mut().relations;
        local.copy_from(name, &loaded);
        Ok(())
    } else {
        load(std::slice::from_mut(model), name, relation, true).await
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
