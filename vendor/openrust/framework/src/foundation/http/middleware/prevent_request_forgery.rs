use ::http::HeaderValue;
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

use crate::{
    contracts::{container::Container as _, http::Middleware, session::Session},
    cookie::CookieValuePrefix,
    http::{HttpError, HttpFuture, Method, Next, Request, Response},
    session::{SessionError, SessionManager},
    support::facades::current_container,
};

/// Checks browser fetch metadata or the current session's CSRF token.
#[derive(Clone, Default)]
pub struct PreventRequestForgery {
    except: Vec<String>,
    origin_only: bool,
    allow_same_site: bool,
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's request forgery configuration"
)]
impl PreventRequestForgery {
    pub fn except(&mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        self.except.extend(paths.into_iter().map(Into::into));
        self
    }

    pub fn allowSameSite(&mut self, allow: bool) -> &mut Self {
        self.allow_same_site = allow;
        self
    }

    pub fn useOriginOnly(&mut self, enabled: bool) -> &mut Self {
        self.origin_only = enabled;
        self
    }

    fn allowed_without_token(&self, request: &Request) -> Result<bool, HttpError> {
        if matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        ) || self
            .except
            .iter()
            .any(|pattern| request.matches_url_or_path(pattern))
        {
            return Ok(true);
        }
        let mut sites = request.headers().get_all("sec-fetch-site").iter();
        let site = sites.next().and_then(|value| value.to_str().ok());
        let allowed = sites.next().is_none()
            && (site == Some("same-origin") || self.allow_same_site && site == Some("same-site"));
        if !allowed && self.origin_only {
            return Err(HttpError::OriginMismatch);
        }
        Ok(allowed)
    }

    async fn tokens_match(&self, request: &mut Request) -> Result<bool, HttpError> {
        let input = request.input(Some("_token")).await?;
        let token = match input {
            Some(serde_json::Value::String(value)) if !matches!(value.as_str(), "" | "0") => {
                Some(value)
            }
            Some(value) if php_truthy(&value) => return Ok(false),
            _ => header(request, "x-csrf-token")
                .filter(|value| !matches!(*value, "" | "0"))
                .map(str::to_owned),
        };
        let expected = request.session()?.token().map(str::as_bytes);
        if let Some(token) = token {
            return Ok(equal(expected, token.as_bytes()));
        }
        let encrypted = header(request, "x-xsrf-token")
            .and_then(|value| {
                request
                    .cookie_encryption
                    .as_ref()?
                    .encrypter
                    .decryptString(value)
                    .ok()
            })
            .map(Zeroizing::new);
        Ok(encrypted.is_some_and(|value| equal(expected, CookieValuePrefix::remove(&value))))
    }
}

impl Middleware for PreventRequestForgery {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !self.allowed_without_token(request)? && !self.tokens_match(request).await? {
                return Err(HttpError::TokenMismatch);
            }
            let mut response = next.run(request).await;
            if !self.origin_only
                && let Some(token) = request.session()?.token()
            {
                let manager = current_container()
                    .ok_or(SessionError::Configuration)?
                    .get::<SessionManager>("session")
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                let cookie = manager.xsrf_cookie(token)?;
                response.setCookie(&cookie, request.is_secure())?;
            }
            Ok(response)
        })
    }
}

fn equal(expected: Option<&[u8]>, supplied: &[u8]) -> bool {
    expected.is_some_and(|expected| !expected.is_empty() && bool::from(expected.ct_eq(supplied)))
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|value: &HeaderValue| value.to_str().ok())
}

fn php_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        serde_json::Value::String(value) => !matches!(value.as_str(), "" | "0"),
        serde_json::Value::Array(values) => !values.is_empty(),
        serde_json::Value::Object(values) => !values.is_empty(),
    }
}
