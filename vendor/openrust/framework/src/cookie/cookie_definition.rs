use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ::http::HeaderValue;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::CookieError;

pub(super) const URL_ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');
const MAX_HEADER_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    Lax,
    Strict,
    None,
}

/// Per-cookie overrides of the configured cookie defaults.
#[derive(Clone, Debug)]
pub struct CookieOptions {
    pub path: Option<String>,
    pub domain: Option<String>,
    pub secure: Option<bool>,
    pub http_only: bool,
    pub raw: bool,
    pub same_site: Option<SameSite>,
    pub partitioned: bool,
}

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            path: None,
            domain: None,
            secure: None,
            http_only: true,
            raw: false,
            same_site: None,
            partitioned: false,
        }
    }
}

/// An HTTP cookie with its value and attributes.
#[derive(Clone)]
pub struct Cookie {
    name: String,
    value: Option<String>,
    expires: Option<SystemTime>,
    options: CookieOptions,
}

impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cookie").finish_non_exhaustive()
    }
}

impl Cookie {
    pub(super) fn new(
        name: &str,
        value: Option<&str>,
        expires: Option<SystemTime>,
        options: CookieOptions,
    ) -> Result<Self, CookieError> {
        if name.is_empty()
            || name == "0"
            || name.len() > MAX_HEADER_BYTES
            || name.bytes().any(|byte| byte.is_ascii_control())
            || (options.raw && name.parse::<::http::HeaderName>().is_err())
            || options.path.as_deref().is_some_and(invalid_attribute)
            || options.domain.as_deref().is_some_and(invalid_attribute)
            || value.is_some_and(|value| {
                value.len() > MAX_HEADER_BYTES || options.raw && invalid_attribute(value)
            })
        {
            return Err(CookieError);
        }
        let cookie = Self {
            name: name.to_owned(),
            value: value.map(str::to_owned),
            expires,
            options,
        };
        cookie.to_header(false)?;
        Ok(cookie)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
    pub fn path(&self) -> &str {
        self.options.path.as_deref().unwrap_or("/")
    }
    pub fn domain(&self) -> Option<&str> {
        self.options.domain.as_deref()
    }
    pub fn secure(&self) -> Option<bool> {
        self.options.secure
    }
    pub fn http_only(&self) -> bool {
        self.options.http_only
    }
    pub fn same_site(&self) -> Option<SameSite> {
        self.options.same_site
    }
    pub fn expires(&self) -> Option<SystemTime> {
        self.expires
    }

    pub fn with_value(&self, value: &str) -> Result<Self, CookieError> {
        Self::new(&self.name, Some(value), self.expires, self.options.clone())
    }

    pub fn to_header(&self, secure_default: bool) -> Result<HeaderValue, CookieError> {
        use fmt::Write as _;

        let now = cookie_time()?;
        let mut wire = String::new();
        if self.options.raw {
            wire.push_str(&self.name);
        } else {
            wire.extend(utf8_percent_encode(&self.name, URL_ENCODE));
        }
        wire.push('=');
        let expires = if let Some(value) = self.value.as_deref().filter(|value| !value.is_empty()) {
            if self.options.raw {
                wire.push_str(value);
            } else {
                wire.extend(utf8_percent_encode(value, URL_ENCODE));
            }
            self.expires
        } else {
            wire.push_str("deleted");
            Some(
                now.checked_sub(Duration::from_secs(31_536_001))
                    .ok_or(CookieError)?,
            )
        };
        if let Some(expires) = expires {
            let age = expires.duration_since(now).unwrap_or_default().as_secs();
            write!(
                wire,
                "; expires={}; Max-Age={age}",
                httpdate::fmt_http_date(expires)
            )
            .map_err(|_| CookieError)?;
        }
        write!(wire, "; path={}", self.path()).map_err(|_| CookieError)?;
        if let Some(domain) = self.domain() {
            write!(wire, "; domain={domain}").map_err(|_| CookieError)?;
        }
        if self.secure().unwrap_or(secure_default) {
            wire.push_str("; secure");
        }
        if self.http_only() {
            wire.push_str("; httponly");
        }
        match self.same_site() {
            Some(SameSite::Lax) => wire.push_str("; samesite=lax"),
            Some(SameSite::Strict) => wire.push_str("; samesite=strict"),
            Some(SameSite::None) => wire.push_str("; samesite=none"),
            None => {}
        }
        if self.options.partitioned {
            wire.push_str("; partitioned");
        }
        if wire.len() > MAX_HEADER_BYTES {
            return Err(CookieError);
        }
        let mut header = HeaderValue::from_str(&wire).map_err(|_| CookieError)?;
        header.set_sensitive(true);
        Ok(header)
    }
}

fn invalid_attribute(value: &str) -> bool {
    value.len() > MAX_HEADER_BYTES
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b';')
}

pub(super) fn cookie_time() -> Result<SystemTime, CookieError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CookieError)?
        .as_secs();
    UNIX_EPOCH
        .checked_add(Duration::from_secs(seconds))
        .ok_or(CookieError)
}
