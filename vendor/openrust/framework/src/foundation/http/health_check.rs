use std::sync::{Arc, Weak};

use ::http::{
    HeaderValue,
    header::{CACHE_CONTROL, CONTENT_TYPE},
};

use crate::{
    container::Container,
    contracts::{container::Container as _, routing::Handler},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
};

/// Reports that the application has booted and can handle HTTP requests.
pub struct HealthCheck {
    container: Weak<Container>,
}

impl HealthCheck {
    pub(crate) fn new(container: Arc<Container>) -> Self {
        Self {
            container: Arc::downgrade(&container),
        }
    }
}

impl Handler for HealthCheck {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let (body, content_type) = if request.expectsJson() {
                (String::from(r#"{"status":"up"}"#), "application/json")
            } else {
                let container = self
                    .container
                    .upgrade()
                    .ok_or_else(|| HttpError::Internal {
                        source: Box::new(std::io::Error::other(
                            "application container unavailable",
                        )),
                    })?;
                let config = container
                    .get::<dyn crate::contracts::config::Repository>("config")
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                let name = config.get("app.name");
                let name = name
                    .as_ref()
                    .and_then(|value| value.as_str())
                    .unwrap_or("OpenRust");
                let name = name
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace(char::from(39), "&#039;");
                (
                    format!(
                        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><title>{name}</title></head><body><h1>Application up</h1><p>HTTP request received.</p></body></html>"#
                    ),
                    "text/html; charset=utf-8",
                )
            };
            let mut response = Response::new(StatusCode::OK, Body::from(body));
            response
                .parts_mut()
                .headers
                .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
            response
                .parts_mut()
                .headers
                .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache, private"));
            Ok(response)
        })
    }
}
