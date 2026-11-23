use std::{
    error::Error,
    fmt::Write as _,
    io::{self, Write as _},
    sync::{Mutex, RwLock, Weak},
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

tokio::task_local! {static CURRENTLY_REPORTING:Option<(usize,usize)>;}
fn exception_identity(exception: &Exception<'_>) -> usize {
    if let Some(shared) = exception.error().downcast_ref::<SharedException>() {
        return shared.identity().as_ptr() as *const () as usize;
    }
    exception.error() as *const (dyn Error + 'static) as *const () as usize
}

/// Reports application exceptions and renders HTTP or console responses.
pub struct Handler {
    configuration: RwLock<Option<Exceptions>>,
    owner: Weak<Container>,
    reported: Mutex<Vec<Weak<dyn Error + Send + Sync>>>,
}

impl Handler {
    pub(crate) fn new(configuration: Option<Exceptions>, owner: Weak<Container>) -> Self {
        Self {
            configuration: RwLock::new(configuration),
            owner,
            reported: Mutex::default(),
        }
    }

    fn configuration(&self) -> Option<Exceptions> {
        self.configuration.read().unwrap().clone()
    }

    pub(crate) async fn render_http(&self, request: &mut Request, error: &HttpError) -> Response {
        if matches!(error, HttpError::DumpAndDie(_)) {
            return error.bare_response();
        }
        let original = Exception::Http(error);
        let mapped = self
            .configuration()
            .and_then(|config| config.map_exception(&original));
        let exception = mapped.as_ref().map(Exception::Mapped).unwrap_or(original);
        if self.should_report(&exception).await && !self.duplicate(&exception, true) {
            let _ = self.write_report(&exception, Some(request)).await;
        }
        self.render_http_response_only(request, error).await
    }

    async fn render_http_response_only(
        &self,
        request: &mut Request,
        error: &HttpError,
    ) -> Response {
        if matches!(error, HttpError::DumpAndDie(_)) {
            return error.bare_response();
        }
        let original = Exception::Http(error);
        if let Some(mapped) = self
            .configuration()
            .and_then(|config| config.map_exception(&original))
        {
            let exception = Exception::Mapped(&mapped);
            if let Some(error) = exception.downcast_ref::<HttpError>() {
                return self.render_http_unmapped_response(request, error).await;
            }
            return self.render_unmapped(request, &exception);
        }
        self.render_http_unmapped_response(request, error).await
    }

    async fn render_http_unmapped_response(
        &self,
        request: &mut Request,
        error: &HttpError,
    ) -> Response {
        if matches!(error, HttpError::DumpAndDie(_)) {
            return error.bare_response();
        }
        let exception = Exception::Http(error);
        let configuration_snapshot = self.configuration();
        let config = configuration_snapshot.as_ref();
        let response = if let Some(custom) =
            config.and_then(|config| config.render_using(&exception, request))
        {
            custom
        } else if let Some(response) = exception
            .downcast_ref::<HttpResponseException>()
            .and_then(HttpResponseException::take_response)
        {
            response
        } else {
            let json = self.renders_json(request, &exception);
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

    fn render_unmapped(&self, request: &Request, exception: &Exception<'_>) -> Response {
        let response = self
            .configuration()
            .as_ref()
            .and_then(|config| config.render_using(exception, request))
            .or_else(|| {
                exception
                    .downcast_ref::<HttpResponseException>()
                    .and_then(HttpResponseException::take_response)
            })
            .unwrap_or_else(|| {
                let json = self.renders_json(request, exception);
                self.render_default(request, exception, json)
            });
        self.respond(response, exception, request)
    }

    fn renders_json(&self, request: &Request, exception: &Exception<'_>) -> bool {
        self.configuration().as_ref().map_or_else(
            || request.expects_json(),
            |config| config.renders_json(request, exception),
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
        match self.configuration().as_ref() {
            Some(config) => config.respond_using(response, exception, request),
            None => response,
        }
    }

    fn duplicate(&self, exception: &Exception<'_>, claim: bool) -> bool {
        if !self
            .configuration()
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
        self.configuration()
            .as_ref()
            .map_or(default, |configuration| {
                configuration.allows_reporting(exception, default)
            })
    }

    async fn allows_throttle(&self, exception: &Exception<'_>) -> bool {
        match self
            .configuration()
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
            max: limit.max_attempts,
            decay: limit.decay_seconds,
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
        let identity = (self as *const Self as usize, exception_identity(exception));
        CURRENTLY_REPORTING
            .scope(Some(identity), self.write_report_inner(exception, request))
            .await
    }
    async fn write_report_inner(
        &self,
        exception: &Exception<'_>,
        request: Option<&mut Request>,
    ) -> LogResult<()> {
        if self
            .configuration()
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
            let configuration_snapshot = self.configuration();
            let configuration = configuration_snapshot.as_ref();
            let level = configuration.map_or(Level::Error, |config| config.report_level(exception));
            let mut context = self.context_for_exception(exception).fields;
            if let Some(error) = exception.downcast_ref::<HttpError>() {
                context.insert("status".into(), error.status().as_u16().into());
            }
            let snapshot = self.build_log_exception(exception, None, true);
            if let Some(request) = request {
                return logger
                    .log_with_exception_for_request(
                        request,
                        level,
                        &exception.error().to_string(),
                        &context,
                        &snapshot,
                    )
                    .await;
            }
            return logger.log_with_exception(
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

impl ExceptionHandler for Handler {
    fn configure(
        &self,
        update: &mut dyn FnMut(&mut Exceptions),
    ) -> Result<(), crate::contracts::container::BindingResolutionError> {
        let mut configuration = self
            .configuration
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut candidate = configuration.clone().unwrap_or_default();
        update(&mut candidate);
        let retired = configuration.replace(candidate);
        drop(configuration);
        drop(retired);
        Ok(())
    }
    fn is_reporting(&self, exception: &Exception<'_>) -> Option<bool> {
        let identity = (self as *const Self as usize, exception_identity(exception));
        Some(
            CURRENTLY_REPORTING
                .try_with(|current| *current == Some(identity))
                .unwrap_or(false),
        )
    }
    fn render_http<'a>(
        &'a self,
        request: &'a mut Request,
        error: &'a HttpError,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move { Handler::render_http(self, request, error).await })
    }

    fn render_http_response<'a>(
        &'a self,
        request: &'a mut Request,
        error: &'a HttpError,
    ) -> HttpFuture<'a, Response> {
        Box::pin(async move { self.render_http_response_only(request, error).await })
    }

    fn build_context_for_exception(&self, exception: &Exception<'_>) -> crate::log::LogContext {
        self.configuration()
            .as_ref()
            .map(|config| config.report_context(exception))
            .unwrap_or_default()
    }
    fn context_for_exception<'a>(
        &self,
        exception: &Exception<'a>,
    ) -> crate::contracts::debug::ExceptionContext<'a> {
        let mut fields = self.build_context_for_exception(exception);
        if let Some(user) = self
            .owner
            .upgrade()
            .and_then(|owner| owner.get::<crate::auth::AuthManager>("auth").ok())
            .and_then(|auth| auth.resolved_user(None))
        {
            let id = serde_json::to_value(user.get_auth_identifier()).expect("认证标识可序列化");
            if id != serde_json::Value::Null
                && id != serde_json::Value::Bool(false)
                && id != serde_json::json!(0)
                && id != serde_json::json!("")
                && id != serde_json::json!("0")
            {
                fields.insert("userId".into(), id);
            }
        }
        crate::contracts::debug::ExceptionContext {
            fields,
            exception: *exception,
        }
    }

    fn build_log_exception(
        &self,
        exception: &Exception<'_>,
        name: Option<&'static str>,
        reported: bool,
    ) -> serde_json::Value {
        let configuration_snapshot = self.configuration();
        let configuration = configuration_snapshot.as_ref();
        let name = name
            .or_else(|| configuration.and_then(|config| config.node_name(exception.root_error())));
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

    fn should_stop_retries(&self, exception: &Exception<'_>) -> bool {
        self.configuration()
            .as_ref()
            .is_some_and(|configuration| configuration.stops_retries(exception))
    }

    fn should_report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async move {
            !self.duplicate(exception, false)
                && self.allowed(exception)
                && self.allows_throttle(exception).await
        })
    }

    fn report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async move {
            let mapped = self
                .configuration()
                .and_then(|config| config.map_exception(exception));
            let exception = mapped.as_ref().map(Exception::Mapped).unwrap_or(*exception);
            let exception = &exception;
            if !self.should_report(exception).await || self.duplicate(exception, true) {
                return Ok(());
            }
            self.write_report(exception, None).await
        })
    }

    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response {
        let mapped = self
            .configuration()
            .and_then(|config| config.map_exception(exception));
        let exception = mapped.as_ref().map(Exception::Mapped).unwrap_or(*exception);
        self.render_unmapped(request, &exception)
    }

    fn render_for_console(&self, output: &mut Output, exception: &Exception<'_>) {
        writeln!(output.stderr, "{}", exception.error()).expect("writing to String");
    }
}
