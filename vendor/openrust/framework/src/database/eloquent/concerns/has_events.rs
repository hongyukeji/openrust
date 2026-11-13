use std::{
    any::{Any, TypeId},
    collections::HashMap,
    future::Future,
    sync::{Arc, RwLock},
};

use crate::{
    contracts::container::Container as _,
    database::{Connection, DatabaseError, DatabaseFuture, DatabaseResult},
};

use super::super::Model;

/// An Eloquent lifecycle event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelEvent {
    Retrieved,
    Creating,
    Created,
    Updating,
    Updated,
    Saving,
    Saved,
    Restoring,
    Restored,
    Replicating,
    Trashed,
    Deleting,
    Deleted,
    ForceDeleting,
    ForceDeleted,
}

/// A listener's response to a model event.
pub enum ModelEventResponse {
    Continue,
    Stop,
    Cancel,
}

/// Handles model lifecycle events with access to the current operation's connection.
pub trait ModelEventListener<M: Model>: Send + Sync {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>>;
}

type Registrations = HashMap<(TypeId, ModelEvent), Vec<Arc<dyn Any + Send + Sync>>>;

#[derive(Default)]
pub(crate) struct ModelEvents(RwLock<Registrations>);

impl ModelEvents {
    pub(crate) fn listen<M: Model>(
        &self,
        event: ModelEvent,
        listener: impl ModelEventListener<M> + 'static,
    ) {
        self.listen_to::<M>(&[event], listener);
    }

    pub(crate) fn listen_to<M: Model>(
        &self,
        events: &[ModelEvent],
        listener: impl ModelEventListener<M> + 'static,
    ) {
        let listener: Arc<dyn ModelEventListener<M>> = Arc::new(listener);
        let listener: Arc<dyn Any + Send + Sync> = Arc::new(listener);
        let mut registrations = self.0.write().unwrap();
        for event in events
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
        {
            registrations
                .entry((TypeId::of::<M>(), event))
                .or_default()
                .push(Arc::clone(&listener));
        }
    }

    fn listeners<M: Model>(&self, event: ModelEvent) -> Vec<Arc<dyn Any + Send + Sync>> {
        self.0
            .read()
            .unwrap()
            .get(&(TypeId::of::<M>(), event))
            .cloned()
            .unwrap_or_default()
    }
}

tokio::task_local! {
    static SUPPRESSED: ();
}

pub(in crate::database::eloquent) async fn without_events<F: Future>(work: F) -> F::Output {
    SUPPRESSED.scope((), work).await
}

pub(in crate::database::eloquent) async fn fire<M: Model>(
    model: &mut M,
    connection: &Connection,
    event: ModelEvent,
    halt: bool,
) -> DatabaseResult<bool> {
    if SUPPRESSED.try_with(|()| ()).is_ok() {
        return Ok(true);
    }
    let Some(container) = Connection::model_container(Some(connection)) else {
        return Ok(true);
    };
    let events = container
        .make::<ModelEvents>()
        .map_err(DatabaseError::Unavailable)?;
    for listener in events.listeners::<M>(event) {
        let listener = listener
            .downcast_ref::<Arc<dyn ModelEventListener<M>>>()
            .expect("model event listeners are registered by model type");
        match listener.handle(event, model, connection).await? {
            ModelEventResponse::Stop if halt => break,
            ModelEventResponse::Cancel if halt => return Ok(false),
            _ => {}
        }
    }
    Ok(true)
}

pub(in crate::database::eloquent) async fn retrieved<M: Model>(
    models: &mut [M],
    connection: &Connection,
) -> DatabaseResult<()> {
    for model in models {
        fire(model, connection, ModelEvent::Retrieved, false).await?;
    }
    Ok(())
}
