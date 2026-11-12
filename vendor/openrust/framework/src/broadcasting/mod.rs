//! Channel authorization, broadcaster selection, and queued event delivery.

mod broadcast_controller;
mod broadcast_event;
mod broadcast_manager;
mod broadcast_service_provider;
mod channel;
mod facade;

use std::{collections::BTreeMap, error::Error, fmt, future::Future, pin::Pin};

pub use broadcast_controller::BroadcastController;
pub use broadcast_event::BroadcastEvent;
pub(crate) use broadcast_event::JOB;
pub use broadcast_manager::BroadcastManager;
pub use broadcast_service_provider::BroadcastServiceProvider;
pub use channel::{BroadcastChannel, Channel, ChannelRegistry, PresenceChannel, PrivateChannel};
pub use facade::Broadcast;

pub type BroadcastFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type BroadcastResult<T> = Result<T, BroadcastException>;
pub type BroadcastPayload = serde_json::Map<String, serde_json::Value>;

#[derive(Debug)]
#[non_exhaustive]
pub enum BroadcastException {
    NoRegistrationScope,
    InvalidChannel,
    AccessDenied,
    UnknownConnection(String),
    InvalidConnection(String),
    UnknownDriver(String),
    DuplicateDriver(String),
    Backend(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for BroadcastException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRegistrationScope => {
                formatter.write_str("No active broadcast channel registration.")
            }
            Self::InvalidChannel => formatter.write_str("Invalid broadcast channel pattern."),
            Self::AccessDenied => formatter.write_str("Broadcast channel access denied."),
            Self::UnknownConnection(name) => {
                write!(formatter, "Unknown broadcast connection '{name}'.")
            }
            Self::InvalidConnection(name) => {
                write!(formatter, "Invalid broadcast connection '{name}'.")
            }
            Self::UnknownDriver(name) => write!(formatter, "Unknown broadcast driver '{name}'."),
            Self::DuplicateDriver(name) => write!(
                formatter,
                "Broadcast driver '{name}' is already registered."
            ),
            Self::Backend(_) => formatter.write_str("Broadcast backend failed."),
        }
    }
}

impl Error for BroadcastException {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Backend(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

/// Channel parameters after matching a registered pattern.
pub type ChannelParameters = BTreeMap<String, String>;

/// A channel's authentication guards in lookup order.
#[derive(Default)]
pub struct ChannelOptions {
    pub guards: Vec<String>,
}

/// Authorization decisions for private and presence subscriptions.
pub enum ChannelAuthorization {
    Denied,
    Allowed,
    Presence(BroadcastPayload),
}
