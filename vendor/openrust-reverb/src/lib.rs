//! OpenRust's Reverb server and Pusher-compatible protocol.

mod broadcaster;
#[path = "../config/reverb.rs"]
mod configuration;
mod servers;

pub use broadcaster::{ReverbBroadcaster, ReverbServiceProvider};

pub mod protocols;

/// The service providers registered by OpenRust package discovery.
pub fn openrust_providers() -> Vec<Box<dyn openrust::support::ServiceProvider>> {
    vec![Box::new(ReverbServiceProvider)]
}
