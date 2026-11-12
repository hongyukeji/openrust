use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    contracts::broadcasting::{Factory, ShouldBroadcast},
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult},
};

use super::{BroadcastChannel, BroadcastException, BroadcastPayload, BroadcastResult};

pub(crate) const JOB: &str = "openrust.broadcast.event";

/// The delivery data for a compiled broadcastable event.
#[derive(Serialize, Deserialize)]
pub struct BroadcastEvent {
    event: String,
    channels: Vec<BroadcastChannel>,
    payload: BroadcastPayload,
    connections: Vec<String>,
    socket: Option<String>,
}

impl BroadcastEvent {
    pub(crate) fn from_event(event: &dyn ShouldBroadcast, socket: Option<&str>) -> Option<Self> {
        if !event.broadcastWhen() {
            return None;
        }
        let channels = event.broadcastOn();
        if channels.is_empty() {
            return None;
        }
        Some(Self {
            event: event
                .broadcastAs()
                .unwrap_or_else(|| event.name())
                .to_owned(),
            channels,
            payload: event
                .broadcastWith()
                .unwrap_or_else(|| event.broadcastData()),
            connections: event.broadcastConnections(),
            socket: socket.map(str::to_owned),
        })
    }

    pub async fn handle(&self, factory: &dyn Factory) -> BroadcastResult<()> {
        let default = self.connections.is_empty().then_some(None);
        let connections = self.connections.iter().map(|name| Some(name.as_str()));
        for name in default.into_iter().chain(connections) {
            factory
                .connection(name)?
                .broadcast(
                    &self.channels,
                    &self.event,
                    &self.payload,
                    self.socket.as_deref(),
                )
                .await?;
        }
        Ok(())
    }
}

pub(super) struct DeliverBroadcast(pub(super) Arc<dyn Factory>);

impl crate::contracts::queue::JobHandler for DeliverBroadcast {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let event: BroadcastEvent = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            event.handle(self.0.as_ref()).await.map_err(execution)
        })
    }
}

fn execution(source: BroadcastException) -> QueueError {
    QueueError::Execution {
        source: Box::new(source),
    }
}
