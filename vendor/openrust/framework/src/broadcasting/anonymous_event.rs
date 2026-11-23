use std::sync::Arc;

use crate::{
    contracts::{
        broadcasting::ShouldBroadcast,
        events::{Dispatcher as _, Event},
    },
    events::{DispatchResult, EventContext, EventResult},
    http::Request,
    support::facades::Event as EventFacade,
};

use super::{BroadcastChannel, BroadcastPayload};

/// One or more channels selected by an anonymous broadcast.
pub struct AnonymousChannels(Vec<BroadcastChannel>);

impl<T: Into<BroadcastChannel>> From<T> for AnonymousChannels {
    fn from(channel: T) -> Self {
        Self(vec![channel.into()])
    }
}

impl<T: Into<BroadcastChannel>> From<Vec<T>> for AnonymousChannels {
    fn from(channels: Vec<T>) -> Self {
        Self(channels.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<BroadcastChannel>, const N: usize> From<[T; N]> for AnonymousChannels {
    fn from(channels: [T; N]) -> Self {
        Self(channels.into_iter().map(Into::into).collect())
    }
}

/// A broadcast constructed without declaring an application event type.
pub struct AnonymousEvent {
    channels: Vec<BroadcastChannel>,
    connection: Option<String>,
    name: Option<String>,
    payload: BroadcastPayload,
    socket: Option<String>,
    immediate: bool,
}

impl AnonymousEvent {
    pub const NAME: &'static str = "AnonymousEvent";

    pub(super) fn new(channels: AnonymousChannels) -> Self {
        Self {
            channels: channels.0,
            connection: None,
            name: None,
            payload: BroadcastPayload::new(),
            socket: None,
            immediate: false,
        }
    }

    pub fn via(mut self, connection: impl Into<String>) -> Self {
        self.connection = Some(connection.into());
        self
    }

    /// Sets the wire name (`as`).
    pub fn r#as(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with(mut self, payload: BroadcastPayload) -> Self {
        self.payload = payload;
        self
    }

    /// Excludes the originating socket from the delivery.
    pub fn to_others(mut self, request: &Request) -> Self {
        self.socket = super::request_socket(request);
        self
    }

    pub async fn send(self) -> EventResult<DispatchResult> {
        self.send_with_context(&mut EventContext::new()).await
    }

    pub async fn send_now(mut self) -> EventResult<DispatchResult> {
        self.immediate = true;
        self.send().await
    }

    /// Sends through the event dispatcher with an explicit transaction scope.
    pub async fn send_with_context(
        self,
        context: &mut EventContext<'_>,
    ) -> EventResult<DispatchResult> {
        EventFacade::dispatcher()?
            .dispatch(Arc::new(self), context)
            .await
    }
}

impl Event for AnonymousEvent {
    fn name(&self) -> &str {
        Self::NAME
    }
}

impl ShouldBroadcast for AnonymousEvent {
    fn broadcast_on(&self) -> Vec<BroadcastChannel> {
        self.channels.clone()
    }

    fn broadcast_as(&self) -> Option<&str> {
        self.name
            .as_deref()
            .filter(|name| !matches!(*name, "" | "0"))
    }

    fn broadcast_with(&self) -> Option<BroadcastPayload> {
        Some(self.payload.clone())
    }

    fn broadcast_connections(&self) -> Vec<String> {
        self.connection.iter().cloned().collect()
    }

    fn should_broadcast_now(&self) -> bool {
        self.immediate
    }

    fn broadcast_socket(&self) -> Option<&str> {
        self.socket.as_deref()
    }
}
