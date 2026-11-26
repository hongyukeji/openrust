use std::{fmt::Write as _, sync::Arc};

use ::http::{HeaderValue, header::CONTENT_TYPE};

use crate::{
    contracts::debug::Exception,
    http::{Body, HttpError, Request, Response, StatusCode},
    view::Factory,
};

#[derive(Clone)]
pub(super) struct ViewFailure {
    pub(super) error: crate::contracts::debug::SharedException,
    pub(super) debug: bool,
}

pub(super) fn default_response(
    request: &Request,
    exception: &Exception<'_>,
    error: Option<&HttpError>,
    json: bool,
    debug: impl FnOnce() -> bool,
) -> Response {
    let status = error.map_or(StatusCode::INTERNAL_SERVER_ERROR, HttpError::status);
    let message = match error {
        #[cfg(feature = "sanctum")]
        Some(HttpError::MissingAbility(_)) => crate::sanctum::MissingAbilityException::MESSAGE,
        #[cfg(feature = "sanctum")]
        Some(HttpError::MissingScope(_)) => crate::sanctum::MissingScopeException::MESSAGE,
        Some(HttpError::InvalidSignature) => "Invalid signature.",
        Some(HttpError::Unauthenticated) => "Unauthenticated.",
        Some(HttpError::OriginMismatch) => "Origin mismatch.",
        Some(HttpError::Throttled { .. }) => "Too Many Attempts.",
        Some(HttpError::TokenMismatch) => "CSRF token mismatch.",
        Some(HttpError::AuthorizationDenied(message)) if !message.is_empty() => message,
        Some(HttpError::AuthorizationDeniedWithStatus { message, .. }) if !message.is_empty() => {
            message
        }
        Some(HttpError::Aborted { message, .. }) if !message.is_empty() => message,
        Some(HttpError::Validation(exception)) => exception.message(),
        _ => status.canonical_reason().unwrap_or("HTTP Error"),
    };
    let debug = debug();
    let mut view_failure = None;
    let sources =
        (status.is_server_error() && !matches!(error, Some(HttpError::Aborted { .. })) && debug)
            .then(|| debug_sources(exception));
    let (content_type, body) = if json {
        let message = if let Some(HttpError::Aborted { message, .. }) = error {
            message
        } else if status.is_server_error() {
            sources
                .as_ref()
                .and_then(|sources| sources.first())
                .map_or("Server Error", String::as_str)
        } else {
            message
        };
        let mut payload = serde_json::json!({"message": message});
        if let Some(HttpError::Validation(exception)) = error {
            payload["errors"] = serde_json::json!(exception.errors().messages());
        }
        if let Some(sources) = sources {
            payload["sources"] = serde_json::json!(sources);
        }
        ("application/json", payload.to_string())
    } else {
        let page = if sources.is_none() {
            match error_page(request, exception, status, message) {
                Ok(page) => page,
                Err(error) => {
                    view_failure = Some(ViewFailure {
                        error: crate::contracts::debug::SharedException::new(error),
                        debug,
                    });
                    None
                }
            }
        } else {
            None
        };
        (
            "text/html; charset=utf-8",
            page.unwrap_or_else(|| html(status, message, sources.as_deref())),
        )
    };
    let mut response = Response::new(status, Body::from(body));
    if let Some(failure) = view_failure {
        response.parts_mut().extensions.insert(failure);
    }
    response
        .parts_mut()
        .headers
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    match error {
        Some(error) => error.with_response_headers(response),
        None => response,
    }
}

fn error_page(
    request: &Request,
    exception: &Exception<'_>,
    status: StatusCode,
    message: &str,
) -> crate::view::ViewResult<Option<String>> {
    let Some(views) = request.parts().extensions.get::<Arc<Factory>>() else {
        return Ok(None);
    };
    let exact = format!("errors.{}", status.as_u16());
    if let Some(html) = views.render_error(
        &exact,
        status.as_u16(),
        message,
        super::view_exception::value(exception),
    )? {
        return Ok(Some(html));
    }
    // 专用状态页先于通用4xx／5xx模板，419无标准StatusCode常量。
    if status.as_u16() == 419
        || matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::PAYMENT_REQUIRED
                | StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::TOO_MANY_REQUESTS
                | StatusCode::INTERNAL_SERVER_ERROR
                | StatusCode::SERVICE_UNAVAILABLE
        )
    {
        return views.render_error(
            &format!("errors::{}", status.as_u16()),
            status.as_u16(),
            message,
            super::view_exception::value(exception),
        );
    }
    let family = if status.is_client_error() {
        "errors.4xx"
    } else if status.is_server_error() {
        "errors.5xx"
    } else {
        return Ok(None);
    };
    views.render_error(
        family,
        status.as_u16(),
        message,
        super::view_exception::value(exception),
    )
}

fn debug_sources(exception: &Exception<'_>) -> Vec<String> {
    let original = exception.root_error();
    let mut current = match exception {
        Exception::Http(_) => original.source(),
        Exception::Error(_) | Exception::Mapped(_) | Exception::Typed(_, _) => Some(original),
    };
    let mut sources = Vec::new();
    while let Some(error) = current {
        if sources.len() == 16 {
            break;
        }
        sources.push(error.to_string().chars().take(2048).collect());
        current = error.source();
    }
    sources
}

pub(in crate::foundation) fn maintenance_page() -> String {
    html(StatusCode::SERVICE_UNAVAILABLE, "Service Unavailable", None)
}

fn html(status: StatusCode, message: &str, sources: Option<&[String]>) -> String {
    let title = format!("{} {}", status.as_u16(), escape_html(message));
    let mut html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title></head><body><h1>{title}</h1>"
    );
    if let Some(sources) = sources.filter(|sources| !sources.is_empty()) {
        html.push_str("<ol>");
        for source in sources {
            write!(html, "<li>{}</li>", escape_html(source)).expect("writing to String");
        }
        html.push_str("</ol>");
    }
    html.push_str("</body></html>");
    html
}

fn escape_html(text: &str) -> String {
    let mut html = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '"' => html.push_str("&quot;"),
            '\'' => html.push_str("&#39;"),
            _ => html.push(character),
        }
    }
    html
}
