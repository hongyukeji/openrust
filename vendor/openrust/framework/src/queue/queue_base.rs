use std::{
    collections::HashMap,
    sync::{Arc, RwLock, Weak},
};

use crate::{
    container::Container,
    contracts::{container::Container as _, queue::JobHandler},
    support::facades::application_scope,
};

use super::{JobContext, Payload, QueueError, QueueManager, QueueResult};

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
        if name.trim().is_empty() || name.starts_with("events.listener:") {
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

    pub(super) fn register_listeners(
        &self,
        registrations: Vec<(String, Arc<dyn JobHandler>)>,
    ) -> QueueResult<()> {
        let mut handlers = self
            .handlers
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut names = std::collections::HashSet::new();
        if registrations
            .iter()
            .any(|(name, _)| handlers.contains_key(name) || !names.insert(name))
        {
            return Err(QueueError::InvalidConfiguration);
        }
        handlers.extend(registrations);
        Ok(())
    }

    pub(super) fn manager(&self) -> QueueResult<Arc<QueueManager>> {
        self.owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?
            .make::<QueueManager>()
            .map_err(QueueError::Unavailable)
    }

    pub(super) async fn run_chain_catches(
        &self,
        job: &Payload,
        cause: &QueueError,
    ) -> QueueResult<()> {
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        let manager = owner
            .make::<QueueManager>()
            .map_err(QueueError::Unavailable)?;
        application_scope(owner, manager.run_chain_catches(job, cause)).await;
        Ok(())
    }

    fn handler(&self, name: &str) -> Option<Arc<dyn JobHandler>> {
        self.handlers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    pub(super) async fn run_failed_handler(
        &self,
        context: &mut JobContext<'_>,
        payload: &Payload,
        error: &QueueError,
    ) -> QueueResult<()> {
        let Some(handler) = self.handler(payload.name()) else {
            return Ok(());
        };
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        application_scope(owner, async {
            handler.failed(context, payload, error).await
        })
        .await
    }

    pub(super) async fn fire(
        &self,
        context: &mut JobContext<'_>,
        payload: &Payload,
    ) -> QueueResult<()> {
        context.chain_edits = Default::default();
        context.chain_update = None;
        let handler = self
            .handler(payload.name())
            .ok_or_else(|| QueueError::UnknownJob(payload.name().into()))?;
        let owner = self
            .owner
            .upgrade()
            .ok_or(QueueError::InvalidConfiguration)?;
        application_scope(owner, async {
            let middleware = handler.middleware();
            let running = context
                .running_handler
                .filter(|_| handler.interruptible().is_some());
            if let Some(running) = running {
                *running
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handler.clone());
            }
            let result = super::middleware::JobNext::new(handler.as_ref(), &middleware)
                .run(context, payload)
                .await;
            if let Some(running) = running {
                *running
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
            result?;
            if context.release_after.is_none() {
                context.finish_chain_edit(payload)?;
            }
            Ok(())
        })
        .await
    }
}
