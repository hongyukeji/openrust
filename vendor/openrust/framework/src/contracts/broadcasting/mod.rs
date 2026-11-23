//! Extension contracts for broadcast events, channel handlers, and drivers.

#![allow(non_snake_case)]

use std::sync::Arc;

use crate::{
    broadcasting::{
        BroadcastChannel, BroadcastFuture, BroadcastPayload, BroadcastResult, ChannelAuthorization,
        ChannelModels, ChannelParameters,
    },
    contracts::{auth::Authenticatable, events::Event},
    http::{Request, Response},
};

pub trait Factory: Send + Sync {
    fn connection(&self, name: Option<&str>) -> BroadcastResult<Arc<dyn Broadcaster>>;
}

pub trait Broadcaster: Send + Sync {
    fn resolve_authenticated_user_using(
        &self,
        _: Arc<crate::broadcasting::AuthenticatedUserResolver>,
    ) -> BroadcastResult<()> {
        Err(crate::broadcasting::BroadcastException::UnsupportedUserAuthentication)
    }
    fn resolve_authenticated_user<'a>(
        &'a self,
        _: &'a mut Request,
    ) -> BroadcastFuture<'a, BroadcastResult<Option<Response>>> {
        Box::pin(async {
            Err(crate::broadcasting::BroadcastException::UnsupportedUserAuthentication)
        })
    }

    /// Authenticates the subscriber, authorizes the channel through the application's
    /// `ChannelRegistry`, and produces the driver's authorization response.
    fn auth<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> BroadcastFuture<'a, BroadcastResult<Response>>;
    fn valid_authentication_response<'a>(
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
        _user: &'a dyn Authenticatable,
        _parameters: &'a ChannelParameters,
    ) -> BroadcastFuture<'a, BroadcastResult<ChannelAuthorization>> {
        Box::pin(async { Ok(ChannelAuthorization::Denied) })
    }

    fn join_bound<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        parameters: &'a ChannelParameters,
        _models: &'a ChannelModels,
    ) -> BroadcastFuture<'a, BroadcastResult<ChannelAuthorization>> {
        self.join(user, parameters)
    }
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
    fn broadcast_on(&self) -> Vec<BroadcastChannel>;
    fn broadcast_as(&self) -> Option<&str> {
        None
    }
    fn broadcast_with(&self) -> Option<BroadcastPayload> {
        None
    }
    /// Fields exposed when the event has no explicit broadcast payload.
    fn broadcast_data(&self) -> BroadcastPayload {
        BroadcastPayload::new()
    }
    fn broadcast_when(&self) -> bool {
        true
    }
    fn broadcast_connections(&self) -> Vec<String> {
        Vec::new()
    }
    fn broadcast_queue(&self) -> Option<&str> {
        None
    }
    fn broadcast_queue_connection(&self) -> Option<&str> {
        None
    }
    fn broadcast_socket(&self) -> Option<&str> {
        None
    }
    fn should_broadcast_now(&self) -> bool {
        false
    }
    fn broadcast_after_commit(&self) -> Option<bool> {
        None
    }
}

pub trait ShouldBroadcastNow: ShouldBroadcast {}

/// A queued broadcast that allows only one pending delivery per event type and ID.
pub trait ShouldBeUnique: ShouldBroadcast {
    fn unique_id(&self) -> &str {
        ""
    }

    fn unique_for(&self) -> Option<u64> {
        None
    }

    fn unique_via(&self) -> Option<&str> {
        None
    }
}
