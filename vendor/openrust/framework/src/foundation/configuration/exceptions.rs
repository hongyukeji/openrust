use std::{error::Error, sync::Arc};

use crate::{
    contracts::debug::Exception,
    http::{HttpError, Request, Response},
};

type JsonPolicy = dyn Fn(&Request, &HttpError) -> bool + Send + Sync;
type Reporter = dyn Fn(&Exception<'_>) -> ReportDecision + Send + Sync;
type Renderer = dyn Fn(&Exception<'_>, &Request) -> Option<Response> + Send + Sync;
type Responder = dyn Fn(Response, &Exception<'_>, &Request) -> Response + Send + Sync;

/// Controls whether later callbacks and default reporting run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportDecision {
    Continue,
    Stop,
}

/// Configures exception reporting and HTTP rendering.
#[derive(Clone, Default)]
pub struct Exceptions {
    json_policy: Option<Arc<JsonPolicy>>,
    reporters: Vec<Arc<Reporter>>,
    renderers: Vec<Arc<Renderer>>,
    responder: Option<Arc<Responder>>,
}

impl Exceptions {
    /// Registers a report callback for an exception type or one of its sources.
    pub fn report<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E) -> ReportDecision + Send + Sync + 'static,
    ) -> &mut Self {
        self.reporters.push(Arc::new(move |exception| {
            exception
                .downcast_ref::<E>()
                .map_or(ReportDecision::Continue, &callback)
        }));
        self
    }

    /// Registers an HTTP renderer. Returning `None` leaves the error to the next renderer.
    pub fn render<E: Error + 'static>(
        &mut self,
        callback: impl Fn(&E, &Request) -> Option<Response> + Send + Sync + 'static,
    ) -> &mut Self {
        self.renderers.push(Arc::new(move |exception, request| {
            exception
                .downcast_ref::<E>()
                .and_then(|error| callback(error, request))
        }));
        self
    }

    /// Sets the final callback for custom and default HTTP exception responses.
    pub fn respond(
        &mut self,
        callback: impl Fn(Response, &Exception<'_>, &Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        self.responder = Some(Arc::new(callback));
        self
    }

    /// Overrides the default request-based JSON decision.
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's exception configuration API"
    )]
    pub fn shouldRenderJsonWhen(
        &mut self,
        policy: impl Fn(&Request, &HttpError) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.json_policy = Some(Arc::new(policy));
        self
    }

    pub(crate) fn renders_json(&self, request: &Request, error: &HttpError) -> bool {
        match &self.json_policy {
            Some(policy) => policy(request, error),
            None => request.expectsJson(),
        }
    }

    pub(crate) fn should_continue_reporting(&self, exception: &Exception<'_>) -> bool {
        self.reporters
            .iter()
            .all(|report| report(exception) == ReportDecision::Continue)
    }

    pub(crate) fn render_using(
        &self,
        exception: &Exception<'_>,
        request: &Request,
    ) -> Option<Response> {
        self.renderers
            .iter()
            .find_map(|render| render(exception, request))
    }

    pub(crate) fn respond_using(
        &self,
        response: Response,
        exception: &Exception<'_>,
        request: &Request,
    ) -> Response {
        match &self.responder {
            Some(respond) => respond(response, exception, request),
            None => response,
        }
    }
}
