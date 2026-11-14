use std::{
    any::Any,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::database::DatabaseResult;

use super::super::Model;

/// Tracks model snapshots while saving one loaded relationship graph.
#[doc(hidden)]
#[derive(Default)]
pub struct PushContext {
    active: HashSet<usize>,
    saved: HashMap<usize, Box<dyn Any + Send + Sync>>,
}

impl PushContext {
    pub(in crate::database::eloquent) async fn push<M: Model>(
        &mut self,
        model: &mut M,
    ) -> DatabaseResult<bool> {
        let state = model.state_mut();
        if let Some(link) = state.detached_autoload.take() {
            state.relations = link.visible_relations(state, |_| true);
        }
        if !model.save().await? {
            return Ok(false);
        }
        model.state_mut().relations.push(self).await
    }

    /// Saves and replaces one shared model snapshot.
    pub async fn push_model<M: Model>(&mut self, model: &mut Arc<M>) -> DatabaseResult<bool> {
        let address = Arc::as_ptr(model).addr();
        if let Some((_, saved)) = self
            .saved
            .get(&address)
            .and_then(|entry| entry.downcast_ref::<(Arc<M>, Arc<M>)>())
        {
            *model = saved.clone();
            return Ok(true);
        }
        if !self.active.insert(address) {
            return Ok(true);
        }
        let mut snapshot = M::from_state(model.state().clone());
        let result = Box::pin(self.push(&mut snapshot)).await;
        self.active.remove(&address);
        let replacement = Arc::new(snapshot);
        if matches!(result, Ok(true)) {
            self.saved
                .insert(address, Box::new((model.clone(), replacement.clone())));
        }
        *model = replacement;
        result
    }

    pub(super) async fn push_many<M: Model>(
        &mut self,
        models: &[Arc<M>],
    ) -> (Option<Vec<Arc<M>>>, DatabaseResult<bool>) {
        let mut models = models.to_vec();
        for model in &mut models {
            match self.push_model(model).await {
                Ok(true) => (),
                result => return (Some(models), result),
            }
        }
        (Some(models), Ok(true))
    }

    pub(in crate::database::eloquent) async fn push_one<M: Model>(
        &mut self,
        model: &Option<Arc<M>>,
    ) -> (Option<Option<Arc<M>>>, DatabaseResult<bool>) {
        let Some(mut model) = model.clone() else {
            return (None, Ok(true));
        };
        let result = self.push_model(&mut model).await;
        (Some(Some(model)), result)
    }
}
