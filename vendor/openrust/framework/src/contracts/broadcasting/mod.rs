//! Extension contracts for broadcast events, channel handlers, and drivers.

#![allow(non_snake_case)]

use std::sync::Arc;

use crate::{
    broadcasting::{
        BroadcastChannel, BroadcastFuture, BroadcastPayload, BroadcastResult, ChannelAuthorization,
        ChannelParameters,
    },
    contracts::{auth::Authenticatable, events::Event},
    http::{Request, Response},
};

pub trait Factory: Send + Sync {
    fn connection(&self, name: Option<&str>) -> BroadcastResult<Arc<dyn Broadcaster>>;
}

pub trait Broadcaster: Send + Sync {
    /// Authenticates the subscriber, authorizes the channel through the application's
    /// `ChannelRegistry`, and produces the driver's authorization response.
    fn auth<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>>;
    fn validAuthenticationResponse<'a>(
        &'a self,
        request: &'a mut Request,
        result: ChannelAuthorization,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>>;
    fn broadcast<'a>(
        &'a self,
        channels: &'a [BroadcastChannel],
        event: &'a str,
        payload: &'a BroadcastPayload,
        socket: Option<&'a str>,
    ) -> BroadcastFuture<'a, BroadcastResult<()>>;
}

/// A channel authorization handler registered with the Broadcast facade.
pub trait ChannelHandler: Send + Sync {
    fn join<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        parameters: &'a ChannelParameters,
    ) -> BroadcastFuture<'a, BroadcastResult<ChannelAuthorization>>;
}

impl<F> ChannelHandler for F
where
    F: Fn(&dyn Authenticatable, &ChannelParameters) -> BroadcastResult<ChannelAuthorization>
        + Send
        + Sync,
{
    fn join<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        parameters: &'a ChannelParameters,
    ) -> BroadcastFuture<'a, BroadcastResult<ChannelAuthorization>> {
        Box::pin(async move { self(user, parameters) })
    }
}

pub trait ShouldBroadcast: Event {
    fn broadcastOn(&self) -> Vec<BroadcastChannel>;
    fn broadcastAs(&self) -> Option<&str> {
        None
    }
    fn broadcastWith(&self) -> Option<BroadcastPayload> {
        None
    }
    /// Fields exposed when the event has no explicit broadcast payload.
    fn broadcastData(&self) -> BroadcastPayload {
        BroadcastPayload::new()
    }
    fn broadcastWhen(&self) -> bool {
        true
    }
    fn broadcastConnections(&self) -> Vec<String> {
        Vec::new()
    }
    fn broadcastQueue(&self) -> Option<&str> {
        None
    }
    fn broadcastQueueConnection(&self) -> Option<&str> {
        None
    }
    fn broadcastAfterCommit(&self) -> Option<bool> {
        None
    }
}

pub trait ShouldBroadcastNow: ShouldBroadcast {}
