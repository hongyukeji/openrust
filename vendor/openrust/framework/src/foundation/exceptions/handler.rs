use std::{
    error::Error,
    fmt::Write as _,
    io::{self, Write as _},
    sync::{Mutex, Weak},
};

use crate::{
    cache::{
        RateLimiter,
        rate_limiting::{
            Limit,
            counters::{self, Admission, Quota},
        },
    },
    console::Output,
    container::Container,
    contracts::container::Container as _,
    contracts::debug::{Exception, ExceptionHandler, SharedException},
    foundation::configuration::{Exceptions, ReportThrottle},
    http::{HttpError, HttpFuture, HttpResponseException, Request, Response},
    log::{Level, LogError, LogResult},
};

/// Reports application exceptions and renders HTTP or console responses.
pub struct Handler {
    configuration: Option<Exceptions>,
    owner: Weak<Container>,
    reported: Mutex<Vec<Weak<dyn Error + Send + Sync>>>,
}

impl Handler {
    pub(crate) fn new(configuration: Option<Exceptions>, owner: Weak<Container>) -> Self {
        Self {
            configuration,
            owner,
            reported: Mutex::default(),
        }
    }

    pub(crate) async fn render_http(&self, request: &mut Request, error: &HttpError) -> Response {
        if matches!(error, HttpError::DumpAndDie(_)) {
            return error.bare_response();
        }
        let exception = Exception::Http(error);
        if self.shouldReport(&exception).await && !self.duplicate(&exception, true) {
            let _ = self.write_report(&exception, Some(request)).await;
        }
        let config = self.configuration.as_ref();
        let response = if let Some(custom) =
            config.and_then(|config| config.render_using(&exception, request))
        {
            custom
        } else if let Some(response) = exception
            .downcast_ref::<HttpResponseException>()
            .and_then(HttpResponseException::takeResponse)
        {
            response
        } else {
            let json = self.renders_json(request, Some(error));
            let redirect = match error {
                HttpError::Validation(validation) if !json => {
                    super::validation_redirect::response(request, validation, config).await
                }
                _ => None,
            };
            redirect.unwrap_or_else(|| self.render_default(request, &exception, json))
        };
        self.respond(response, &exception, request)
    }

    fn renders_json(&self, request: &Request, error: Option<&HttpError>) -> bool {
        self.configuration.as_ref().map_or_else(
            || request.expectsJson(),
            |config| config.renders_json(request, error),
        )
    }

    fn render_default(&self, request: &Request, exception: &Exception<'_>, json: bool) -> Response {
        let error = exception.downcast_ref::<HttpError>();
        let debug = || {
            self.owner
                .upgrade()
                .and_then(|owner| {
                    owner
                        .get::<dyn crate::contracts::config::Repository>("config")
                        .ok()
                })
                .is_some_and(|config| {
                    config.get("app.debug") == Some(serde_json::Value::Bool(true))
                })
        };
        super::render::default_response(request, exception, error, json, debug)
    }

    fn respond(
        &self,
        response: Response,
        exception: &Exception<'_>,
        request: &Request,
    ) -> Response {
        match self.configuration.as_ref() {
            Some(config) => config.respond_using(response, exception, request),
            None => response,
        }
    }

    fn duplicate(&self, exception: &Exception<'_>, claim: bool) -> bool {
        if !self
            .configuration
            .as_ref()
            .is_some_and(Exceptions::suppress_duplicates)
        {
            return false;
        }
        let Some(shared) = exception.downcast_ref::<SharedException>() else {
            return false;
        };
        let identity = shared.identity();
        let mut reported = self
            .reported
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reported.retain(|entry| entry.strong_count() > 0);
        let duplicate = reported.iter().any(|entry| entry.ptr_eq(&identity));
        if claim && !duplicate {
            reported.push(identity);
        }
        duplicate
    }

    fn allowed(&self, exception: &Exception<'_>) -> bool {
        let default = exception.downcast_ref::<HttpResponseException>().is_none()
            && exception
                .downcast_ref::<HttpError>()
                .is_none_or(|error| matches!(error, HttpError::Internal { .. }));
        self.configuration
            .as_ref()
            .map_or(default, |configuration| {
                configuration.allows_reporting(exception, default)
            })
    }

    async fn allows_throttle(&self, exception: &Exception<'_>) -> bool {
        match self
            .configuration
            .as_ref()
            .and_then(|config| config.report_throttle(exception))
        {
            Some((ReportThrottle::Lottery(lottery), _)) => lottery.choose(),
            Some((ReportThrottle::Limit(limit), kind)) => self.admit_report(limit, kind).await,
            Some((ReportThrottle::Unlimited, _)) | None => true,
        }
    }

    async fn admit_report(&self, limit: Limit, kind: &str) -> bool {
        let Some(repository) = self.owner.upgrade().and_then(|owner| {
            owner
                .get::<RateLimiter>("rateLimiter")
                .ok()?
                .repository()
                .ok()
        }) else {
            return true;
        };
        let key = if limit.key.is_empty() {
            counters::key("exception-report-type", "", kind)
        } else {
            counters::key("exception-report", "", &limit.key)
        };
        let quota = Quota {
            key,
            max: limit.maxAttempts,
            decay: limit.decaySeconds,
            charge: true,
        };
        // A failed throttle must leave the original exception reportable.
        !matches!(
            counters::admit(repository.as_ref(), &[quota]).await,
            Ok(Admission::Denied { .. })
        )
    }

    async fn write_report(
        &self,
        exception: &Exception<'_>,
        request: Option<&mut Request>,
    ) -> LogResult<()> {
        if self
            .configuration
            .as_ref()
            .is_some_and(|configuration| !configuration.should_continue_reporting(exception))
        {
            return Ok(());
        }
        let logger = self
            .owner
            .upgrade()
            .and_then(|owner| owner.make::<dyn crate::contracts::log::Logger>().ok());
        if let Some(logger) = logger {
            let configuration = self.configuration.as_ref();
            let level = configuration.map_or(Level::Error, |config| config.report_level(exception));
            let mut context = configuration
                .map(|config| config.report_context(exception))
                .unwrap_or_default();
            if let Some(error) = exception.downcast_ref::<HttpError>() {
                context.insert("status".into(), error.status().as_u16().into());
            }
            let snapshot = self.buildLogException(exception, None, true);
            if let Some(request) = request {
                return logger
                    .logWithExceptionForRequest(
                        request,
                        level,
                        &exception.error().to_string(),
                        &context,
                        &snapshot,
                    )
                    .await;
            }
            return logger.logWithException(
                level,
                &exception.error().to_string(),
                &context,
                None,
                &snapshot,
            );
        }
        // Startup can fail before the log binding is installed.
        if let Exception::Http(error) = exception {
            writeln!(io::stderr().lock(), "HTTP exception: {error}")
                .map_err(|error| LogError::WriteFailed(Box::new(error)))?;
        }
        Ok(())
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl ExceptionHandler for Handler {
    fn buildLogException(
        &self,
        exception: &Exception<'_>,
        name: Option<&'static str>,
        reported: bool,
    ) -> serde_json::Value {
        let configuration = self.configuration.as_ref();
        let name =
            name.or_else(|| configuration.and_then(|config| config.node_name(exception.error())));
        let context = configuration
            .filter(|_| !reported)
            .map(|config| config.report_context(exception))
            .unwrap_or_default();
        crate::log::json::snapshot(
            exception,
            name,
            |error| {
                configuration
                    .map(|config| config.node_context(error))
                    .unwrap_or_default()
            },
            context,
        )
    }

    fn shouldStopRetries(&self, exception: &Exception<'_>) -> bool {
        self.configuration
            .as_ref()
            .is_some_and(|configuration| configuration.stops_retries(exception))
    }

    fn shouldReport<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async move {
            !self.duplicate(exception, false)
                && self.allowed(exception)
                && self.allows_throttle(exception).await
        })
    }

    fn report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async move {
            if !self.shouldReport(exception).await || self.duplicate(exception, true) {
                return Ok(());
            }
            self.write_report(exception, None).await
        })
    }

    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response {
        let response = self
            .configuration
            .as_ref()
            .and_then(|config| config.render_using(exception, request))
            .or_else(|| {
                exception
                    .downcast_ref::<HttpResponseException>()
                    .and_then(HttpResponseException::takeResponse)
            })
            .unwrap_or_else(|| {
                let json = self.renders_json(request, exception.downcast_ref::<HttpError>());
                self.render_default(request, exception, json)
            });
        self.respond(response, exception, request)
    }

    fn renderForConsole(&self, output: &mut Output, exception: &Exception<'_>) {
        writeln!(output.stderr, "{}", exception.error()).expect("writing to String");
    }
}
