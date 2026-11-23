use ::http::{header::HOST, uri::Authority};
use serde_json::Value;

use crate::{
    http::{HttpError, proxy::parse_authority},
    support::{Arr, Collection},
};

use super::{Request, matches_pattern};

mod query;

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Request {
    pub fn normalize_query_string(query: &str) -> Result<String, HttpError> {
        query::canonical(query)
    }
    /// Returns the effective host, excluding the port.
    pub fn host(&self) -> Result<String, HttpError> {
        Ok(host_name(&self.effective_authority()?))
    }

    /// Returns the effective host and any non-default port.
    pub fn http_host(&self) -> Result<String, HttpError> {
        let authority = self.effective_authority()?;
        let host = host_name(&authority);
        match authority.port_u16() {
            Some(port) if port != if self.secure() { 443 } else { 80 } => {
                Ok(format!("{host}:{port}"))
            }
            _ => Ok(host),
        }
    }

    pub fn scheme_and_http_host(&self) -> Result<String, HttpError> {
        let scheme = if self.secure() { "https" } else { "http" };
        Ok(format!("{scheme}://{}", self.http_host()?))
    }

    /// Returns the origin and trusted proxy base path.
    pub fn root(&self) -> Result<String, HttpError> {
        Ok(format!(
            "{}{}",
            self.scheme_and_http_host()?,
            self.get_base_url()
        ))
    }

    /// Returns the current request URL without its query or trailing slashes.
    pub fn url(&self) -> Result<String, HttpError> {
        Ok(format!(
            "{}{}",
            self.root()?,
            self.uri().path().trim_end_matches('/')
        ))
    }

    /// Returns the current request URL with its normalized query.
    pub fn full_url(&self) -> Result<String, HttpError> {
        let url = self.url()?;
        let query = query::canonical(self.uri().query().unwrap_or_default())?;
        Ok(if query.is_empty() {
            url
        } else {
            format!("{url}{}{query}", self.query_separator())
        })
    }

    /// Checks the full request URL against any supplied wildcard pattern.
    pub fn full_url_is(&self, patterns: &[&str]) -> Result<bool, HttpError> {
        let url = self.full_url()?;
        Ok(patterns
            .iter()
            .any(|pattern| matches_pattern(&url, pattern)))
    }

    /// Returns the URL with the supplied top-level query values merged in.
    pub fn full_url_with_query(&self, additional: &Value) -> Result<String, HttpError> {
        if !Arr::accessible(additional) {
            return Err(HttpError::InvalidRequest);
        }
        let current = self.query(None)?.unwrap_or_default();
        let (url, query) = if empty_query(&current) {
            (self.full_url()?, additional.clone())
        } else {
            let query = Collection::fromValue(current)
                .merge(Collection::fromValue(additional.clone()))
                .to_array()?;
            (self.url()?, query)
        };
        let query = query::encode(&query)?;
        Ok(format!("{url}{}{query}", self.query_separator()))
    }

    /// Returns the URL with the supplied query keys or dotted paths removed.
    pub fn full_url_without_query(&self, keys: &[&str]) -> Result<String, HttpError> {
        let mut current = self.query(None)?.unwrap_or_default();
        for key in keys {
            Arr::forget(&mut current, key).map_err(|_| HttpError::InvalidRequest)?;
        }
        let url = self.url()?;
        Ok(if empty_query(&current) {
            url
        } else {
            format!(
                "{url}{}{}",
                self.query_separator(),
                query::encode(&current)?
            )
        })
    }

    fn query_separator(&self) -> &'static str {
        if self.get_base_url().is_empty() && self.uri().path() == "/" {
            "/?"
        } else {
            "?"
        }
    }

    fn effective_authority(&self) -> Result<Authority, HttpError> {
        let authority = self
            .uri()
            .authority()
            .map(Authority::as_str)
            .or_else(|| self.headers().get(HOST)?.to_str().ok())
            .ok_or(HttpError::InvalidRequest)?;
        parse_authority(authority)
    }
}

fn host_name(authority: &Authority) -> String {
    authority.host().trim_end_matches('.').to_ascii_lowercase()
}

fn empty_query(query: &Value) -> bool {
    match query {
        Value::Object(entries) => entries.is_empty(),
        Value::Array(entries) => entries.is_empty(),
        _ => true,
    }
}
