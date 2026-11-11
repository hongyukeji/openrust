use std::{marker::PhantomData, sync::Arc};

use serde::de::DeserializeOwned;

use crate::{
    contracts::{
        events::{Event, Listener},
        queue::JobHandler,
    },
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult},
};

use super::EventContext;

/// Invokes a registered event listener through the queue worker.
pub struct CallQueuedListener<E> {
    listener: Arc<dyn Listener>,
    event: PhantomData<fn() -> E>,
}

impl<E> CallQueuedListener<E> {
    pub fn new(listener: Arc<dyn Listener>) -> Self {
        Self {
            listener,
            event: PhantomData,
        }
    }
}

impl<E: Event + DeserializeOwned> JobHandler for CallQueuedListener<E> {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let event: E = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            self.listener
                .handle(&event, &mut EventContext::new())
                .await
                .map_err(|source| QueueError::Execution {
                    source: Box::new(source),
                })?;
            Ok(())
        })
    }
}
