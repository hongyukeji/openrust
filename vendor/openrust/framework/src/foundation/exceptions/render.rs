use std::{fmt::Write as _, sync::Arc};

use ::http::{HeaderValue, header::CONTENT_TYPE};

use crate::{
    contracts::debug::Exception,
    foundation::configuration::Exceptions,
    http::{Body, HttpError, Request, Response, StatusCode},
    view::Factory,
};

pub(super) fn default_response(
    request: &Request,
    exception: &Exception<'_>,
    error: Option<&HttpError>,
    configuration: Option<&Exceptions>,
    debug: impl FnOnce() -> bool,
) -> Response {
    let status = error.map_or(StatusCode::INTERNAL_SERVER_ERROR, HttpError::status);
    let message = match error {
        Some(HttpError::Throttled { .. }) => "Too Many Attempts.",
        _ => status.canonical_reason().unwrap_or("HTTP Error"),
    };
    let json = configuration.map_or_else(
        || request.expectsJson(),
        |config| config.renders_json(request, error),
    );
    let sources = (status.is_server_error() && debug()).then(|| debug_sources(exception));
    let (content_type, body) = if json {
        let message = if status.is_server_error() {
            sources
                .as_ref()
                .and_then(|sources| sources.first())
                .map_or("Server Error", String::as_str)
        } else {
            message
        };
        let mut payload = serde_json::json!({"message": message});
        if let Some(sources) = sources {
            payload["sources"] = serde_json::json!(sources);
        }
        ("application/json", payload.to_string())
    } else {
        let page = if sources.is_none() {
            error_page(request, status, message)
        } else {
            None
        };
        (
            "text/html; charset=utf-8",
            page.unwrap_or_else(|| html(status, message, sources.as_deref())),
        )
    };
    let mut response = Response::new(status, Body::from(body));
    response
        .parts_mut()
        .headers
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    match error {
        Some(error) => error.with_response_headers(response),
        None => response,
    }
}

fn error_page(request: &Request, status: StatusCode, message: &str) -> Option<String> {
    let views = request.parts().extensions.get::<Arc<Factory>>()?;
    let exact = format!("errors.{}", status.as_u16());
    if let Some(html) = views.render_error(&exact, status.as_u16(), message) {
        return Some(html);
    }
    // Laravel supplies dedicated fallback pages for these statuses.
    if matches!(
        status,
        StatusCode::NOT_FOUND | StatusCode::INTERNAL_SERVER_ERROR | StatusCode::SERVICE_UNAVAILABLE
    ) {
        return None;
    }
    let family = if status.is_client_error() {
        "errors.4xx"
    } else if status.is_server_error() {
        "errors.5xx"
    } else {
        return None;
    };
    views.render_error(family, status.as_u16(), message)
}

fn debug_sources(exception: &Exception<'_>) -> Vec<String> {
    let original = exception.error();
    let mut current = match exception {
        Exception::Http(_) => original.source(),
        Exception::Error(_) => Some(original),
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

fn html(status: StatusCode, message: &str, sources: Option<&[String]>) -> String {
    let title = format!("{} {message}", status.as_u16());
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
