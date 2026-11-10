use std::{
    fmt::Write as _,
    io::{self, Write as _},
};

use crate::{
    console::Output,
    contracts::debug::{Exception, ExceptionHandler},
    foundation::configuration::Exceptions,
    http::{HttpError, Request, Response},
    log::{LogError, LogResult},
};

/// Reports application exceptions and renders HTTP or console responses.
pub struct Handler {
    configuration: Option<Exceptions>,
}

impl Handler {
    pub(crate) fn new(configuration: Option<Exceptions>) -> Self {
        Self { configuration }
    }
}

#[allow(
    non_snake_case,
    reason = "Matches Laravel's exception handler contract"
)]
impl ExceptionHandler for Handler {
    fn shouldReport(&self, exception: &Exception<'_>) -> bool {
        matches!(
            exception,
            Exception::Http(HttpError::Internal { .. }) | Exception::Error(_)
        )
    }

    fn report(&self, exception: &Exception<'_>) -> LogResult<()> {
        if !self.shouldReport(exception)
            || self
                .configuration
                .as_ref()
                .is_some_and(|configuration| !configuration.should_continue_reporting(exception))
        {
            return Ok(());
        }
        // Console failures already have a stderr renderer. HTTP needs a diagnostic sink
        // until the application log manager is wired in.
        if let Exception::Http(error) = exception {
            writeln!(io::stderr().lock(), "HTTP exception: {error}")
                .map_err(|error| LogError::WriteFailed(Box::new(error)))?;
        }
        Ok(())
    }

    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response {
        let fallback;
        let error = match exception {
            Exception::Http(error) => *error,
            Exception::Error(_) => {
                fallback = HttpError::Internal {
                    source: Box::new(io::Error::other("Application exception")),
                };
                &fallback
            }
        };
        let Some(configuration) = &self.configuration else {
            return error.default_response(request, None);
        };
        let response = configuration
            .render_using(exception, request)
            .unwrap_or_else(|| error.default_response(request, Some(configuration)));
        configuration.respond_using(response, exception, request)
    }

    fn renderForConsole(&self, output: &mut Output, exception: &Exception<'_>) {
        writeln!(output.stderr, "{}", exception.error()).expect("writing to String");
    }
}
