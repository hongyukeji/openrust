use std::{
    error::Error,
    fmt::Write as _,
    io::{self, Write as _},
    sync::{Mutex, Weak},
};

use crate::{
    console::Output,
    container::Container,
    contracts::container::Container as _,
    contracts::debug::{Exception, ExceptionHandler, SharedException},
    foundation::configuration::Exceptions,
    http::{HttpError, Request, Response},
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
        let default = exception
            .downcast_ref::<HttpError>()
            .is_none_or(|error| matches!(error, HttpError::Internal { .. }));
        self.configuration
            .as_ref()
            .map_or(default, |configuration| {
                configuration.allows_reporting(exception, default)
            })
    }
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's exception handler contract"
)]
impl ExceptionHandler for Handler {
    fn shouldReport(&self, exception: &Exception<'_>) -> bool {
        !self.duplicate(exception, false) && self.allowed(exception)
    }

    fn report(&self, exception: &Exception<'_>) -> LogResult<()> {
        if !self.shouldReport(exception) || self.duplicate(exception, true) {
            return Ok(());
        }
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
            return logger.log(level, &exception.error().to_string(), &context);
        }
        // Startup can fail before the log binding is installed.
        if let Exception::Http(error) = exception {
            writeln!(io::stderr().lock(), "HTTP exception: {error}")
                .map_err(|error| LogError::WriteFailed(Box::new(error)))?;
        }
        Ok(())
    }

    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response {
        let configuration = self.configuration.as_ref();
        let response = configuration
            .and_then(|config| config.render_using(exception, request))
            .unwrap_or_else(|| {
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
                super::render::default_response(request, exception, error, configuration, debug)
            });
        match configuration {
            Some(config) => config.respond_using(response, exception, request),
            None => response,
        }
    }

    fn renderForConsole(&self, output: &mut Output, exception: &Exception<'_>) {
        writeln!(output.stderr, "{}", exception.error()).expect("writing to String");
    }
}
