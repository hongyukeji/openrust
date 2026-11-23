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

    fn get_facade_accessor() -> ServiceKey {
        "cookie".into()
    }
}

impl Cookie {
    pub(crate) fn factory() -> Result<Arc<CookieJar>, CookieError> {
        Self::get_facade_root().map_err(|_| CookieError)
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

    pub fn has_queued(name: &str, path: Option<&str>) -> Result<bool, CookieError> {
        with_queue(|jar| jar.has_queued(name, path))
    }

    pub fn get_queued_cookies() -> Result<Vec<Value>, CookieError> {
        with_queue(|jar| jar.get_queued_cookies().into_iter().cloned().collect())
    }

    pub fn flush_queued_cookies() -> Result<(), CookieError> {
        with_queue(|jar| {
            jar.flush_queued_cookies();
        })
    }

    pub fn unqueue(name: &str, path: Option<&str>) -> Result<(), CookieError> {
        with_queue(|jar| jar.unqueue(name, path))
    }

    pub fn expire(name: &str, path: Option<&str>, domain: Option<&str>) -> Result<(), CookieError> {
        with_queue(|jar| jar.expire(name, path, domain))?
    }
}
