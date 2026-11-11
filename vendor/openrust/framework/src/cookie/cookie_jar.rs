use std::time::{Duration, UNIX_EPOCH};

use crate::contracts::cookie::{Factory, QueueingFactory};

use super::{Cookie, CookieError, CookieOptions, SameSite, cookie_definition::cookie_time};

const LAST_HTTP_DATE: u64 = 253_402_300_799;

/// Creates cookies and queues them within the current request.
pub struct CookieJar {
    path: String,
    domain: Option<String>,
    secure: Option<bool>,
    same_site: Option<SameSite>,
    queued: Vec<(String, Vec<Cookie>)>,
}

impl Default for CookieJar {
    fn default() -> Self {
        Self {
            path: "/".into(),
            domain: None,
            secure: None,
            same_site: Some(SameSite::Lax),
            queued: Vec::new(),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's CookieJar API")]
impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn setDefaultPathAndDomain(
        &mut self,
        path: &str,
        domain: Option<&str>,
        secure: Option<bool>,
        same_site: Option<SameSite>,
    ) -> &mut Self {
        self.path = nonempty(path).unwrap_or("/").to_owned();
        self.domain = domain.and_then(nonempty).map(str::to_owned);
        self.secure = secure;
        self.same_site = same_site;
        self
    }

    pub(crate) fn for_request(&self) -> Self {
        Self {
            path: self.path.clone(),
            domain: self.domain.clone(),
            secure: self.secure,
            same_site: self.same_site,
            queued: Vec::new(),
        }
    }

    fn build(
        &self,
        name: &str,
        value: Option<&str>,
        minutes: i64,
        options: &CookieOptions,
    ) -> Result<Cookie, CookieError> {
        let seconds = minutes.checked_mul(60).ok_or(CookieError)?;
        let expires = if seconds == 0 {
            None
        } else {
            let now = cookie_time()?;
            let interval = Duration::from_secs(seconds.unsigned_abs());
            let expires = if seconds < 0 {
                now.checked_sub(interval)
            } else {
                now.checked_add(interval)
            }
            .ok_or(CookieError)?;
            if expires
                .duration_since(UNIX_EPOCH)
                .is_ok_and(|value| value.as_secs() > LAST_HTTP_DATE)
            {
                return Err(CookieError);
            }
            (expires > UNIX_EPOCH).then_some(expires)
        };
        let mut options = options.clone();
        options.path = Some(
            options
                .path
                .as_deref()
                .and_then(nonempty)
                .unwrap_or(&self.path)
                .to_owned(),
        );
        options.domain = options
            .domain
            .as_deref()
            .and_then(nonempty)
            .map(str::to_owned)
            .or_else(|| self.domain.clone());
        options.secure = options.secure.or(self.secure);
        options.same_site = options.same_site.or(self.same_site);
        Cookie::new(name, value, expires, options)
    }

    pub fn queued(&self, name: &str, path: Option<&str>) -> Option<&Cookie> {
        let (_, cookies) = self.queued.iter().find(|(key, _)| key == name)?;
        match path {
            Some(path) => cookies.iter().find(|cookie| cookie.path() == path),
            None => cookies.last(),
        }
    }

    pub fn hasQueued(&self, name: &str, path: Option<&str>) -> bool {
        self.queued(name, path).is_some()
    }

    pub fn expire(
        &mut self,
        name: &str,
        path: Option<&str>,
        domain: Option<&str>,
    ) -> Result<(), CookieError> {
        let cookie = self.forget(name, path, domain)?;
        self.queue(cookie);
        Ok(())
    }

    pub fn getQueuedCookies(&self) -> Vec<&Cookie> {
        self.queued
            .iter()
            .flat_map(|(_, cookies)| cookies)
            .collect()
    }

    pub fn flushQueuedCookies(&mut self) -> &mut Self {
        self.queued.clear();
        self
    }

    pub(super) fn drain(&mut self) -> Vec<Cookie> {
        std::mem::take(&mut self.queued)
            .into_iter()
            .flat_map(|(_, cookies)| cookies)
            .collect()
    }
}

impl Factory for CookieJar {
    fn make(
        &self,
        name: &str,
        value: &str,
        minutes: i64,
        options: &CookieOptions,
    ) -> Result<Cookie, CookieError> {
        self.build(name, Some(value), minutes, options)
    }

    fn forever(
        &self,
        name: &str,
        value: &str,
        options: &CookieOptions,
    ) -> Result<Cookie, CookieError> {
        self.make(name, value, 576_000, options)
    }

    fn forget(
        &self,
        name: &str,
        path: Option<&str>,
        domain: Option<&str>,
    ) -> Result<Cookie, CookieError> {
        self.build(
            name,
            None,
            0,
            &CookieOptions {
                path: path.map(str::to_owned),
                domain: domain.map(str::to_owned),
                ..CookieOptions::default()
            },
        )
    }
}

impl QueueingFactory for CookieJar {
    fn queue(&mut self, cookie: Cookie) {
        if let Some((_, cookies)) = self
            .queued
            .iter_mut()
            .find(|(name, _)| name == cookie.name())
        {
            if let Some(current) = cookies
                .iter_mut()
                .find(|current| current.path() == cookie.path())
            {
                *current = cookie;
            } else {
                cookies.push(cookie);
            }
        } else {
            self.queued.push((cookie.name().to_owned(), vec![cookie]));
        }
    }

    fn unqueue(&mut self, name: &str, path: Option<&str>) {
        if let Some(index) = self.queued.iter().position(|(key, _)| key == name) {
            if let Some(path) = path {
                self.queued[index].1.retain(|cookie| cookie.path() != path);
                if !self.queued[index].1.is_empty() {
                    return;
                }
            }
            self.queued.remove(index);
        }
    }

    fn get_queued_cookies(&self) -> Vec<&Cookie> {
        self.getQueuedCookies()
    }
}

fn nonempty(value: &str) -> Option<&str> {
    (!matches!(value, "" | "0")).then_some(value)
}
