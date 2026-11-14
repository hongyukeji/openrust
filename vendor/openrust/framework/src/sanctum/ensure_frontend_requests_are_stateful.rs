use std::sync::Arc;

use crate::{
    config::{ConfigError, Value},
    contracts::{config::Repository, http::Middleware},
    http::{HttpError, HttpFuture, Next, Request, Response, matches_pattern},
};

#[derive(Clone)]
pub(crate) struct StatefulFrontend;

impl StatefulFrontend {
    pub(crate) fn is_active(request: &Request) -> bool {
        request.parts().extensions.get::<Self>().is_some()
    }
}

pub struct EnsureFrontendRequestsAreStateful {
    patterns: Vec<String>,
    stack: Vec<Arc<dyn Middleware>>,
}

impl EnsureFrontendRequestsAreStateful {
    pub fn new(
        config: &dyn Repository,
        stack: Vec<Arc<dyn Middleware>>,
    ) -> Result<Self, ConfigError> {
        let domains = match config.get("sanctum.stateful") {
            Some(Value::Array(values)) => values
                .into_iter()
                .map(|value| match value {
                    Value::String(domain) => Ok(domain),
                    _ => Err(ConfigError::InvalidType {
                        key: "sanctum.stateful".into(),
                        expected: "a list of domain strings",
                    }),
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(Value::String(domains)) => domains.split(',').map(str::to_owned).collect(),
            None => {
                let mut defaults = [
                    "localhost",
                    "localhost:3000",
                    "127.0.0.1",
                    "127.0.0.1:8000",
                    "::1",
                ]
                .map(str::to_owned)
                .to_vec();
                if let Some(host) = super::settings::application_host(config) {
                    defaults.push(host);
                }
                defaults
            }
            Some(_) => {
                return Err(ConfigError::InvalidType {
                    key: "sanctum.stateful".into(),
                    expected: "a comma-separated string or list of domains",
                });
            }
        };
        let patterns = domains
            .iter()
            .map(|domain| domain.trim())
            .filter(|domain| !domain.is_empty())
            .map(|domain| format!("{domain}/*"))
            .collect();
        Ok(Self { patterns, stack })
    }

    fn matches_frontend(&self, request: &Request) -> bool {
        let Some(origin) = request
            .headers()
            .get(http::header::REFERER)
            .filter(|value| !value.is_empty())
            .or_else(|| request.headers().get(http::header::ORIGIN))
            .and_then(|value| value.to_str().ok())
            .and_then(|value| {
                value
                    .strip_prefix("https://")
                    .or_else(|| value.strip_prefix("http://"))
            })
        else {
            return false;
        };
        let origin = if origin.ends_with('/') {
            std::borrow::Cow::Borrowed(origin)
        } else {
            std::borrow::Cow::Owned(format!("{origin}/"))
        };
        self.patterns.iter().any(|pattern| {
            if pattern.strip_suffix("/*") == Some(super::settings::CURRENT_REQUEST_HOST) {
                let authority = request.uri().authority().cloned().or_else(|| {
                    request
                        .headers()
                        .get(http::header::HOST)?
                        .to_str()
                        .ok()?
                        .parse::<http::uri::Authority>()
                        .ok()
                });
                return authority.is_some_and(|host| {
                    origin
                        .strip_prefix(host.as_str())
                        .is_some_and(|path| path.starts_with('/'))
                });
            }
            matches_pattern(&origin, pattern)
        })
    }
}

impl Middleware for EnsureFrontendRequestsAreStateful {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !self.matches_frontend(request) {
                return Ok(next.run(request).await);
            }
            request.parts_mut().extensions.insert(StatefulFrontend);
            let response = next.run_through(&self.stack, request).await;
            request.parts_mut().extensions.remove::<StatefulFrontend>();
            Ok(response)
        })
    }
}
