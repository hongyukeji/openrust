use crate::cookie::Cookie;

use super::Factory;

/// Queues cookies for the response to the current request.
pub trait QueueingFactory: Factory {
    fn queue(&mut self, cookie: Cookie);
    fn unqueue(&mut self, name: &str, path: Option<&str>);
    fn get_queued_cookies(&self) -> Vec<&Cookie>;
}
