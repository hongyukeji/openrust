use std::{
    error::Error,
    io,
    sync::{Arc, Weak},
};

use ::http::{
    HeaderValue,
    header::{CACHE_CONTROL, CONTENT_TYPE},
};

use crate::{
    container::{Container, ServiceKey},
    contracts::{
        config::Repository,
        container::Container as _,
        debug::{Exception, ExceptionHandler},
        routing::Handler,
    },
    foundation::events::DiagnosingHealth,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Event,
};

/// Reports the application's health after its diagnostic listeners have run.
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
            let container = self
                .container
                .upgrade()
                .ok_or_else(|| internal(io::Error::other("application container unavailable")))?;
            let config = container
                .get::<dyn Repository>("config")
                .map_err(internal)?;
            let healthy = match Event::dispatch(DiagnosingHealth).await {
                Ok(_) => true,
                Err(error) => {
                    if config.get("app.debug").and_then(|value| value.as_bool()) == Some(true) {
                        return Err(internal(error));
                    }
                    if let Ok(handler) = container
                        .get::<dyn ExceptionHandler>(ServiceKey::typed::<dyn ExceptionHandler>())
                    {
                        let _ = handler.report(&Exception::Error(&error)).await;
                    }
                    false
                }
            };
            let status = if healthy {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let response = if request.expectsJson() {
                Response::json(
                    status,
                    serde_json::json!({
                        "status": if healthy { "up" } else { "down" },
                    }),
                )?
            } else {
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
                    .replace('\'', "&#039;");
                let state = if healthy { "" } else { " status-down" };
                let heading = if healthy {
                    "up"
                } else {
                    "experiencing problems"
                };
                let millis = request.received_at.elapsed().as_secs_f64() * 1000.0;
                let body = format!(
                    include_str!("health_up.html"),
                    name = name,
                    state = state,
                    heading = heading,
                    millis = millis
                );
                Response::new(status, Body::from(body)).header(
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                )
            };
            Ok(response.header(CACHE_CONTROL, HeaderValue::from_static("no-cache, private")))
        })
    }
}

fn internal(source: impl Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}
