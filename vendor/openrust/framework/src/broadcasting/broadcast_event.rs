use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    contracts::broadcasting::{Factory, ShouldBeUnique, ShouldBroadcast},
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueResult, UniqueJob},
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) unique: Option<UniqueBroadcast>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct UniqueBroadcast {
    id: String,
    for_seconds: Option<u64>,
    via: Option<String>,
}

impl UniqueBroadcast {
    pub(crate) fn for_event<E: ShouldBeUnique>(event: &E) -> Self {
        let name = std::any::type_name::<E>();
        Self {
            id: format!("{}:{name}:{}", name.len(), event.unique_id()),
            for_seconds: event.unique_for(),
            via: event.unique_via().map(str::to_owned),
        }
    }
}

impl BroadcastEvent {
    pub(crate) fn from_event(event: &dyn ShouldBroadcast, socket: Option<&str>) -> Option<Self> {
        if !event.broadcast_when() {
            return None;
        }
        let channels = event.broadcast_on();
        if channels.is_empty() {
            return None;
        }
        Some(Self {
            event: event
                .broadcast_as()
                .unwrap_or_else(|| event.name())
                .to_owned(),
            channels,
            payload: event
                .broadcast_with()
                .unwrap_or_else(|| event.broadcast_data()),
            connections: event.broadcast_connections(),
            socket: event.broadcast_socket().or(socket).map(str::to_owned),
            unique: None,
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
    fn unique_job(&self, payload: &Payload) -> Option<UniqueJob> {
        let unique: UniqueBroadcast =
            serde_json::from_value(payload.data().get("unique")?.clone()).ok()?;
        Some(UniqueJob {
            id: unique.id,
            for_seconds: unique.for_seconds,
            via: unique.via,
            until_processing: false,
        })
    }

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
