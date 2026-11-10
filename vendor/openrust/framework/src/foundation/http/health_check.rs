use std::sync::Arc;

use ::http::{
    HeaderValue,
    header::{CACHE_CONTROL, CONTENT_TYPE},
};

use crate::{
    contracts::{config::Repository, routing::Handler},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
};

/// Reports that the application has booted and can handle HTTP requests.
pub struct HealthCheck {
    config: Arc<dyn Repository>,
}

impl HealthCheck {
    pub(crate) fn new(config: Arc<dyn Repository>) -> Self {
        Self { config }
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
                let name = self.config.get("app.name");
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
