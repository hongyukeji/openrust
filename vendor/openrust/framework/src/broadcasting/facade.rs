use std::{cell::RefCell, sync::Arc};

use crate::{
    container::ServiceKey,
    contracts::{
        broadcasting::{Broadcaster, ChannelHandler, Factory},
        container::Container as _,
    },
    support::facades::Facade,
};

use super::{
    AnonymousChannels, AnonymousEvent, BroadcastException, BroadcastManager, BroadcastResult,
    ChannelBindings, ChannelOptions, ChannelRegistry, PresenceChannel, PrivateChannel,
};

tokio::task_local! {
    static CHANNELS: RefCell<ChannelRegistry>;
}

/// Selects broadcast connections and registers channel authorization rules.
pub struct Broadcast;

impl Facade for Broadcast {
    type Service = BroadcastManager;

    fn getFacadeAccessor() -> ServiceKey {
        "broadcast".into()
    }
}

#[allow(non_snake_case, reason = "遵循 Laravel 广播用户认证原名")]
impl Broadcast {
    pub fn userRoutes(
        attributes: Option<crate::routing::RouteGroup>,
        connection: Option<&str>,
    ) -> Result<(), crate::routing::RoutingError> {
        use crate::contracts::routing::Registrar as _;
        let handler = Arc::new(super::broadcast_controller::UserBroadcastController {
            connection: connection.map(str::to_owned),
        });
        crate::support::facades::Route::group(
            attributes.unwrap_or_else(|| crate::routing::RouteGroup::new().middleware_name("web")),
            |router| {
                router
                    .add_route(
                        &[crate::http::Method::GET, crate::http::Method::POST],
                        "/broadcasting/user-auth",
                        handler,
                    )?
                    .withoutMiddleware("framework.prevent_request_forgery");
                Ok(())
            },
        )
    }
    pub fn resolveAuthenticatedUserUsing(
        callback: Arc<super::AuthenticatedUserResolver>,
        connection: Option<&str>,
    ) -> BroadcastResult<()> {
        Self::connection(connection)?.resolveAuthenticatedUserUsing(callback)
    }
    pub async fn resolveAuthenticatedUser(
        request: &mut crate::http::Request,
        connection: Option<&str>,
    ) -> BroadcastResult<Option<crate::http::Response>> {
        Self::connection(connection)?
            .resolveAuthenticatedUser(request)
            .await
    }
    pub async fn authenticateUser(
        request: &mut crate::http::Request,
        connection: Option<&str>,
    ) -> BroadcastResult<crate::http::Response> {
        Self::resolveAuthenticatedUser(request, connection)
            .await?
            .ok_or(BroadcastException::AccessDenied)
    }

    pub fn on(channels: impl Into<AnonymousChannels>) -> AnonymousEvent {
        AnonymousEvent::new(channels.into())
    }

    pub fn private(name: impl Into<String>) -> AnonymousEvent {
        Self::on(PrivateChannel { name: name.into() })
    }

    pub fn presence(name: impl Into<String>) -> AnonymousEvent {
        Self::on(PresenceChannel { name: name.into() })
    }

    pub fn connection(name: Option<&str>) -> BroadcastResult<Arc<dyn Broadcaster>> {
        Self::getFacadeRoot()
            .map_err(|error| BroadcastException::Backend(Box::new(error)))?
            .connection(name)
    }

    /// Resolves the current application's channel authorization registry.
    #[allow(non_snake_case, reason = "Matches Laravel's broadcast facade naming")]
    pub fn channelRegistry() -> BroadcastResult<Arc<ChannelRegistry>> {
        crate::support::facades::current_container()
            .ok_or(BroadcastException::NoRegistrationScope)?
            .make::<ChannelRegistry>()
            .map_err(|error| BroadcastException::Backend(Box::new(error)))
    }

    pub fn channel(pattern: &str, handler: impl ChannelHandler + 'static) -> BroadcastResult<()> {
        Self::channelWithOptions(pattern, handler, ChannelOptions::default())
    }

    /// Registers a channel with an explicit authentication guard order.
    #[allow(
        non_snake_case,
        reason = "Rust adapter for Laravel's optional channel options"
    )]
    pub fn channelWithOptions(
        pattern: &str,
        handler: impl ChannelHandler + 'static,
        options: ChannelOptions,
    ) -> BroadcastResult<()> {
        Self::channelWithBindings(pattern, handler, options, ChannelBindings::default())
    }

    /// Registers channel parameters that use model route binding.
    #[allow(non_snake_case, reason = "Matches Laravel channel binding naming")]
    pub fn channelWithBindings(
        pattern: &str,
        handler: impl ChannelHandler + 'static,
        options: ChannelOptions,
        bindings: ChannelBindings,
    ) -> BroadcastResult<()> {
        CHANNELS
            .try_with(|channels| {
                channels
                    .borrow_mut()
                    .register(pattern, handler, options, bindings)
            })
            .map_err(|_| BroadcastException::NoRegistrationScope)?
    }

    pub(crate) fn register(
        register: fn() -> BroadcastResult<()>,
    ) -> BroadcastResult<ChannelRegistry> {
        CHANNELS.sync_scope(RefCell::default(), || {
            register()?;
            Ok(CHANNELS.with(|channels| std::mem::take(&mut *channels.borrow_mut())))
        })
    }
}
