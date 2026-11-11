use std::{any::TypeId, error::Error, sync::Arc};

use crate::{
    contracts::debug::Exception,
    http::{HttpError, Request, Response},
    log::{Level, LogContext},
};

type JsonPolicy = dyn Fn(&Request, &HttpError) -> bool + Send + Sync;
type Reporter = dyn Fn(&Exception<'_>) -> ReportDecision + Send + Sync;
type Renderer = dyn Fn(&Exception<'_>, &Request) -> Option<Response> + Send + Sync;
type Responder = dyn Fn(Response, &Exception<'_>, &Request) -> Response + Send + Sync;

type Matcher = dyn Fn(&Exception<'_>) -> bool + Send + Sync;
type Context = dyn Fn(&Exception<'_>) -> LogContext + Send + Sync;

#[derive(Clone)]
struct Rule {
    kind: TypeId,
    matches: Arc<Matcher>,
}

impl Rule {
    fn typed<E: Error + 'static>() -> Self {
        Self {
            kind: TypeId::of::<E>(),
            matches: Arc::new(|exception| exception.downcast_ref::<E>().is_some()),
        }
    }
}

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
    ignored: Vec<Rule>,
    unignored: Vec<Rule>,
    ignore_when: Vec<Arc<Matcher>>,
    levels: Vec<(Rule, Level)>,
    context: Vec<Arc<Context>>,
    without_duplicates: bool,
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

    /// Sets the default log level for a matching exception type.
    pub fn level<E: Error + 'static>(&mut self, level: Level) -> &mut Self {
        let rule = Rule::typed::<E>();
        if let Some((_, configured)) = self
            .levels
            .iter_mut()
            .find(|(existing, _)| existing.kind == rule.kind)
        {
            *configured = level;
        } else {
            self.levels.push((rule, level));
        }
        self
    }

    /// Adds fields to the default exception log record.
    pub fn context(
        &mut self,
        callback: impl Fn(&Exception<'_>) -> LogContext + Send + Sync + 'static,
    ) -> &mut Self {
        self.context.push(Arc::new(callback));
        self
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's exception configuration API"
    )]
    pub fn dontReport<E: Error + 'static>(&mut self) -> &mut Self {
        self.ignored.push(Rule::typed::<E>());
        self
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's exception configuration API"
    )]
    pub fn stopIgnoring<E: Error + 'static>(&mut self) -> &mut Self {
        let rule = Rule::typed::<E>();
        self.ignored.retain(|existing| existing.kind != rule.kind);
        self.unignored.push(rule);
        self
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's exception configuration API"
    )]
    pub fn dontReportWhen(
        &mut self,
        callback: impl Fn(&Exception<'_>) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.ignore_when.push(Arc::new(callback));
        self
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's exception configuration API"
    )]
    pub fn dontReportDuplicates(&mut self) -> &mut Self {
        self.without_duplicates = true;
        self
    }

    pub(crate) fn suppress_duplicates(&self) -> bool {
        self.without_duplicates
    }

    pub(crate) fn allows_reporting(&self, exception: &Exception<'_>, default: bool) -> bool {
        (default || self.unignored.iter().any(|rule| (rule.matches)(exception)))
            && !self.ignored.iter().any(|rule| (rule.matches)(exception))
            && !self
                .ignore_when
                .iter()
                .any(|predicate| predicate(exception))
    }

    pub(crate) fn report_level(&self, exception: &Exception<'_>) -> Level {
        self.levels
            .iter()
            .find_map(|(rule, level)| (rule.matches)(exception).then_some(*level))
            .unwrap_or(Level::Error)
    }

    pub(crate) fn report_context(&self, exception: &Exception<'_>) -> LogContext {
        self.context
            .iter()
            .flat_map(|callback| callback(exception))
            .collect()
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

    pub(crate) fn renders_json(&self, request: &Request, error: Option<&HttpError>) -> bool {
        match (&self.json_policy, error) {
            (Some(policy), Some(error)) => policy(request, error),
            (Some(policy), None) => {
                let fallback = HttpError::Internal {
                    source: Box::new(std::io::Error::other("Application exception")),
                };
                policy(request, &fallback)
            }
            (None, _) => request.expectsJson(),
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
