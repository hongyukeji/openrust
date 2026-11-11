use std::{
    collections::HashMap,
    sync::{Arc, RwLock, Weak},
};

use crate::{
    container::Container, contracts::queue::JobHandler, support::facades::application_scope,
};

use super::{JobContext, Payload, QueueError, QueueResult};

/// The application-scoped registry shared by queue drivers.
pub struct Queue {
    owner: Weak<Container>,
    handlers: RwLock<HashMap<String, Arc<dyn JobHandler>>>,
}

impl Queue {
    pub(super) fn new(owner: Weak<Container>) -> Self {
        Self {
            owner,
            handlers: RwLock::default(),
        }
    }

    pub fn register(&self, name: &str, handler: Arc<dyn JobHandler>) -> QueueResult<()> {
        if name.trim().is_empty() {
            return Err(QueueError::InvalidPayload);
        }
        let previous = self
            .handlers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.to_owned(), handler);
        drop(previous);
        Ok(())
    }

    pub(super) async fn fire(
        &self,
        context: &mut JobContext<'_>,
        payload: &Payload,
    ) -> QueueResult<()> {
        let handler = self
            .handlers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(payload.name())
            .cloned()
            .ok_or_else(|| QueueError::UnknownJob(payload.name().into()))?;
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        application_scope(owner, handler.handle(context, payload)).await
    }
}
