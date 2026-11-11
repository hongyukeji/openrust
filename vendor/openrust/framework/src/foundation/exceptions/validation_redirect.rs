use ::http::{
    HeaderValue,
    header::{HOST, LOCATION, REFERER},
};
use serde_json::Value;

use crate::{
    foundation::configuration::Exceptions,
    http::{Request, Response, StatusCode},
    validation::ValidationException,
};

const SENSITIVE: &[&str] = &["current_password", "password", "password_confirmation"];

pub(super) async fn response(
    request: &mut Request,
    exception: &ValidationException,
    config: Option<&Exceptions>,
) -> Option<Response> {
    if !request.hasSession() {
        return None;
    }
    let target = exception
        .redirect_target()
        .and_then(local_location)
        .or_else(|| referer(request))
        .or_else(|| {
            let previous = request.session().ok()?.previousUrl()?;
            local_location(previous)
        })
        .unwrap_or_else(|| HeaderValue::from_static("/"));
    let mut input = request
        .input(None)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| serde_json::json!({}));
    let bag = input
        .get("_error_bag")
        .and_then(Value::as_str)
        .unwrap_or_else(|| exception.error_bag())
        .to_owned();
    remove_sensitive(&mut input);
    if let Some(config) = config {
        for path in config.flash_exclusions() {
            if let Some(fields) = input.as_object_mut() {
                fields.remove(path);
            }
            remove_path(&mut input, &path.split('.').collect::<Vec<_>>());
        }
    }
    let session = request.session_mut().ok()?;
    session.flashInput(input).ok()?;
    session.flashErrors(&bag, exception.errors()).ok()?;
    Some(Response::empty(StatusCode::FOUND).header(LOCATION, target))
}

fn local_location(target: &str) -> Option<HeaderValue> {
    if !target.starts_with('/')
        || target.starts_with("//")
        || target.contains('\\')
        || target.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    HeaderValue::from_str(target).ok()
}

fn referer(request: &Request) -> Option<HeaderValue> {
    let referer = request.headers().get(REFERER)?.to_str().ok()?;
    if referer.starts_with('/') {
        return local_location(referer);
    }
    // Compare the URL origin before stripping it from Location.
    let url = url::Url::parse(referer).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let authority = request
        .uri()
        .authority()
        .map(|value| value.as_str())
        .or_else(|| request.headers().get(HOST)?.to_str().ok())?;
    let scheme = request.uri().scheme_str().unwrap_or("http");
    let base = url::Url::parse(&format!("{scheme}://{authority}/")).ok()?;
    if base.origin() != url.origin() {
        return None;
    }
    let mut target = url.path().to_owned();
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    local_location(&target)
}

fn remove_sensitive(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|key, _| {
                !key.rsplit('.')
                    .next()
                    .is_some_and(|key| SENSITIVE.contains(&key))
            });
            for value in fields.values_mut() {
                remove_sensitive(value);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_sensitive(item);
            }
        }
        _ => {}
    }
}

fn remove_path(value: &mut Value, path: &[&str]) {
    let Some((&first, rest)) = path.split_first() else {
        return;
    };
    match value {
        Value::Object(fields) if first == "*" && rest.is_empty() => fields.clear(),
        Value::Object(fields) if first == "*" => {
            for value in fields.values_mut() {
                remove_path(value, rest);
            }
        }
        Value::Object(fields) if rest.is_empty() => {
            fields.remove(first);
        }
        Value::Object(fields) => {
            if let Some(value) = fields.get_mut(first) {
                remove_path(value, rest);
            }
        }
        Value::Array(items) if first == "*" && rest.is_empty() => items.clear(),
        Value::Array(items) if first == "*" => {
            for item in items {
                remove_path(item, rest);
            }
        }
        Value::Array(items) => {
            if let Some(value) = first
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get_mut(index))
            {
                if rest.is_empty() {
                    *value = Value::Null;
                } else {
                    remove_path(value, rest);
                }
            }
        }
        _ => {}
    }
}
