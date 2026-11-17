use std::{collections::HashSet, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    contracts::session::SessionHandler, cookie::CookieOptions, http::Request,
    support::facades::Cookie,
};

use super::{
    SessionError, SessionFuture, SessionResult,
    store::{unix_seconds, valid_id},
};

const COOKIE_LIMIT: usize = 4096;

/// Stores each session's protected payload in the cookie named by its session ID.
pub struct CookieSessionHandler {
    incoming: Option<(String, Payload)>,
    exceptions: Arc<HashSet<String>>,
    minutes: i64,
    seconds: i64,
    expire_on_close: bool,
}

#[derive(Deserialize, Serialize)]
struct Payload {
    data: String,
    expires: i64,
}

impl CookieSessionHandler {
    pub(crate) fn for_request(
        request: &Request,
        id: Option<&str>,
        minutes: u64,
        expire_on_close: bool,
    ) -> SessionResult<Self> {
        let exceptions = request
            .cookie_encryption
            .as_ref()
            .ok_or(SessionError::UnprotectedCookie)?
            .exceptions
            .clone();
        let minutes = i64::try_from(minutes).map_err(|_| SessionError::Configuration)?;
        let seconds = minutes.checked_mul(60).ok_or(SessionError::Configuration)?;
        let mut handler = Self {
            incoming: None,
            exceptions,
            minutes,
            seconds,
            expire_on_close,
        };
        if let Some(id) = id.filter(|id| valid_id(id)) {
            handler.validate(id)?;
            let cookie = request
                .cookie(Some(id))
                .map_err(|_| SessionError::Encoding)?;
            handler.incoming = cookie
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .filter(|value| value.len() <= COOKIE_LIMIT)
                .and_then(|value| serde_json::from_str(value).ok())
                .map(|payload| (id.to_owned(), payload));
        }
        Ok(handler)
    }

    fn validate(&self, id: &str) -> SessionResult<()> {
        if !valid_id(id) {
            return Err(SessionError::Encoding);
        }
        if self.exceptions.contains(id) {
            return Err(SessionError::UnprotectedCookie);
        }
        Ok(())
    }
}

impl SessionHandler for CookieSessionHandler {
    fn read<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            self.validate(id)?;
            Ok(self
                .incoming
                .as_ref()
                .filter(|(name, payload)| name == id && payload.expires >= unix_seconds())
                .map(|(_, payload)| payload.data.as_bytes().to_vec()))
        })
    }

    fn write<'a>(
        &'a mut self,
        id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            self.validate(id)?;
            if bytes.len() > COOKIE_LIMIT {
                return Err(SessionError::PayloadTooLarge);
            }
            let expires = unix_seconds()
                .checked_add(self.seconds)
                .ok_or(SessionError::Clock)?;
            let data = std::str::from_utf8(bytes)
                .map_err(|_| SessionError::Encoding)?
                .to_owned();
            let payload = Payload { data, expires };
            let value = serde_json::to_string(&payload).map_err(|_| SessionError::Encoding)?;
            let minutes = if self.expire_on_close {
                0
            } else {
                self.minutes
            };
            let cookie = Cookie::make(id, &value, minutes, &CookieOptions::default())
                .map_err(|_| SessionError::Storage)?
                .with_encrypted_header_limit(COOKIE_LIMIT);
            Cookie::queue(cookie).map_err(|_| SessionError::Storage)?;
            self.incoming = Some((id.to_owned(), payload));
            Ok(())
        })
    }

    fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            self.validate(id)?;
            Cookie::expire(id, None, None).map_err(|_| SessionError::Storage)?;
            if self.incoming.as_ref().is_some_and(|(name, _)| name == id) {
                self.incoming = None;
            }
            Ok(())
        })
    }

    fn gc(&mut self, _: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
        Box::pin(async { Ok(Some(0)) })
    }
}
