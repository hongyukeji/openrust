use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::cookie::{Factory, QueueingFactory},
    support::facades::Facade,
};

use super::{Cookie as Value, CookieError, CookieJar, CookieOptions, middleware::with_queue};

/// Creates cookies and accesses the current request's response queue.
pub struct Cookie;

impl Facade for Cookie {
    type Service = CookieJar;

    fn getFacadeAccessor() -> ServiceKey {
        "cookie".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Cookie facade API")]
impl Cookie {
    pub(crate) fn factory() -> Result<Arc<CookieJar>, CookieError> {
        Self::getFacadeRoot().map_err(|_| CookieError)
    }

    pub fn make(
        name: &str,
        value: &str,
        minutes: i64,
        options: &CookieOptions,
    ) -> Result<Value, CookieError> {
        Self::factory()?.make(name, value, minutes, options)
    }

    pub fn forever(name: &str, value: &str, options: &CookieOptions) -> Result<Value, CookieError> {
        Self::factory()?.forever(name, value, options)
    }

    pub fn forget(
        name: &str,
        path: Option<&str>,
        domain: Option<&str>,
    ) -> Result<Value, CookieError> {
        Self::factory()?.forget(name, path, domain)
    }

    pub fn queue(cookie: Value) -> Result<(), CookieError> {
        with_queue(|jar| jar.queue(cookie))
    }

    pub fn queued(name: &str, path: Option<&str>) -> Result<Option<Value>, CookieError> {
        with_queue(|jar| jar.queued(name, path).cloned())
    }

    pub fn hasQueued(name: &str, path: Option<&str>) -> Result<bool, CookieError> {
        with_queue(|jar| jar.hasQueued(name, path))
    }

    pub fn getQueuedCookies() -> Result<Vec<Value>, CookieError> {
        with_queue(|jar| jar.getQueuedCookies().into_iter().cloned().collect())
    }

    pub fn flushQueuedCookies() -> Result<(), CookieError> {
        with_queue(|jar| {
            jar.flushQueuedCookies();
        })
    }

    pub fn unqueue(name: &str, path: Option<&str>) -> Result<(), CookieError> {
        with_queue(|jar| jar.unqueue(name, path))
    }

    pub fn expire(name: &str, path: Option<&str>, domain: Option<&str>) -> Result<(), CookieError> {
        with_queue(|jar| jar.expire(name, path, domain))?
    }
}
