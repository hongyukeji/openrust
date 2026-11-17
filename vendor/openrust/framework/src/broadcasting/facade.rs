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

impl Broadcast {
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
