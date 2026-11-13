use std::{
    any::{Any, TypeId},
    collections::HashMap,
    future::Future,
    sync::{Arc, RwLock},
};

use crate::{
    contracts::{container::Container as _, events::Dispatcher as _},
    database::{Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction},
    events::{DispatchResult, Dispatcher, EventContext, ListenerResponse},
    support::facades::application_scope,
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
#[allow(non_snake_case, reason = "Uses Laravel-style lifecycle names")]
pub trait ModelEventListener<M: Model>: Send + Sync {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        connection: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>>;

    /// Handles a lifecycle event inside an explicit model transaction.
    fn handleInTransaction<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        self.handle(event, model, transaction.query_connection())
    }
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
    fire_with_transaction(model, connection, event, halt, None).await
}

pub(in crate::database::eloquent) async fn fire_with_transaction<M: Model>(
    model: &mut M,
    connection: &Connection,
    event: ModelEvent,
    halt: bool,
    mut transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    if SUPPRESSED.try_with(|()| ()).is_ok() {
        return Ok(true);
    }
    let Some(container) = Connection::model_container(Some(connection)) else {
        return Ok(true);
    };
    if let Some(mapped) = model.dispatchesEvents(event) {
        let dispatcher = container
            .make::<Dispatcher>()
            .map_err(DatabaseError::Unavailable)?;
        let mut context = match transaction.as_deref_mut() {
            Some(transaction) => EventContext::in_transaction(transaction),
            None => EventContext::new(),
        };
        let response = application_scope(container.clone(), async {
            if halt {
                let response = dispatcher.until(mapped, &mut context).await?;
                return Ok(match response {
                    Some(ListenerResponse::Value(value))
                        if value.downcast_ref::<bool>() == Some(&false) =>
                    {
                        ModelEventResponse::Cancel
                    }
                    Some(ListenerResponse::Value(value))
                        if empty_mapped_response(value.as_ref()) =>
                    {
                        ModelEventResponse::Continue
                    }
                    Some(_) => ModelEventResponse::Stop,
                    None => ModelEventResponse::Continue,
                });
            }
            Ok(match dispatcher.dispatch(mapped, &mut context).await? {
                DispatchResult::Completed(responses) if responses.iter().any(Option::is_some) => {
                    ModelEventResponse::Stop
                }
                _ => ModelEventResponse::Continue,
            })
        })
        .await
        .map_err(|error| DatabaseError::ModelEvent(Box::new(error)))?;

        match response {
            ModelEventResponse::Cancel => return Ok(false),
            ModelEventResponse::Stop => return Ok(true),
            ModelEventResponse::Continue => {}
        }
    }
    let events = container
        .make::<ModelEvents>()
        .map_err(DatabaseError::Unavailable)?;
    for listener in events.listeners::<M>(event) {
        let listener = listener
            .downcast_ref::<Arc<dyn ModelEventListener<M>>>()
            .expect("model event listeners are registered by model type");
        let response = match transaction.as_deref_mut() {
            Some(transaction) => {
                listener
                    .handleInTransaction(event, model, transaction)
                    .await?
            }
            None => listener.handle(event, model, connection).await?,
        };
        match response {
            ModelEventResponse::Stop if halt => break,
            ModelEventResponse::Cancel if halt => return Ok(false),
            _ => {}
        }
    }
    Ok(true)
}

// PHP empty scalar responses fall through to the model's lifecycle listeners.
fn empty_mapped_response(response: &dyn Any) -> bool {
    macro_rules! is_zero {
        ($($scalar:ty),+) => {
            $(response.downcast_ref::<$scalar>() == Some(&(0 as $scalar)))||+
        };
    }

    response.is::<()>()
        || is_zero!(
            i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
        )
        || response
            .downcast_ref::<String>()
            .is_some_and(|value| value.is_empty() || value == "0")
        || response
            .downcast_ref::<&str>()
            .is_some_and(|value| matches!(*value, "" | "0"))
}

pub(in crate::database::eloquent) async fn retrieved<M: Model>(
    models: &mut [M],
    connection: &Connection,
) -> DatabaseResult<()> {
    retrieved_with_transaction(models, connection, None).await
}

pub(in crate::database::eloquent) async fn retrieved_with_transaction<M: Model>(
    models: &mut [M],
    connection: &Connection,
    mut transaction: Option<&mut Transaction>,
) -> DatabaseResult<()> {
    for model in models {
        fire_with_transaction(
            model,
            connection,
            ModelEvent::Retrieved,
            false,
            transaction.as_deref_mut(),
        )
        .await?;
    }
    Ok(())
}
