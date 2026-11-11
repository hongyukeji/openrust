use std::{borrow::Cow, sync::Arc};

use ::http::header::ACCEPT;
use percent_encoding::percent_decode_str;

use crate::{
    container::ParameterValue,
    foundation::exceptions::Handler,
    routing::binding::{BindingPlan, RouteModels},
};

use super::{Body, HeaderMap, Method, RequestParts, Uri};

pub(crate) mod context;
pub use context::RequestAttributes;

/// An incoming HTTP request with owned metadata and body.
pub struct Request {
    parts: RequestParts,
    body: Body,
    attributes: RequestAttributes,
    route_parameters: context::RouteParameters,
    pub(crate) route_models: RouteModels,
    pub(crate) binding_plan: BindingPlan,
    pub(crate) exceptions: Option<Arc<Handler>>,
}

impl Request {
    pub fn from_parts(parts: RequestParts, body: Body) -> Self {
        Self {
            parts,
            body,
            attributes: RequestAttributes::default(),
            route_parameters: Arc::default(),
            route_models: RouteModels::new(),
            binding_plan: BindingPlan::default(),
            exceptions: None,
        }
    }
    pub fn parts(&self) -> &RequestParts {
        &self.parts
    }
    pub fn parts_mut(&mut self) -> &mut RequestParts {
        &mut self.parts
    }
    pub fn take_body(&mut self) -> Body {
        std::mem::replace(&mut self.body, Body::empty())
    }
    pub fn into_parts(self) -> (RequestParts, Body) {
        (self.parts, self.body)
    }
    pub fn method(&self) -> &Method {
        &self.parts.method
    }
    pub fn uri(&self) -> &Uri {
        &self.parts.uri
    }
    pub fn headers(&self) -> &HeaderMap {
        &self.parts.headers
    }

    /// Returns the decoded URL path without surrounding slashes, or `/` for the root.
    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn decodedPath(&self) -> Cow<'_, str> {
        let path = self.uri().path().trim_matches('/');
        percent_decode_str(if path.is_empty() { "/" } else { path }).decode_utf8_lossy()
    }

    /// Matches the decoded request path against a pattern where `*` spans any characters.
    pub fn is(&self, pattern: &str) -> bool {
        let path = self.decodedPath();
        let Some((prefix, rest)) = pattern.split_once('*') else {
            return path == pattern;
        };
        let Some(path) = path.strip_prefix(prefix) else {
            return false;
        };
        let Some((middle, suffix)) = rest.rsplit_once('*') else {
            return path.ends_with(rest);
        };
        let Some(path) = path.strip_suffix(suffix) else {
            return false;
        };
        middle
            .split('*')
            .try_fold(path, |remaining, part| {
                remaining.split_once(part).map(|(_, tail)| tail)
            })
            .is_some()
    }

    /// Determines whether the request prefers a JSON response.
    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn expectsJson(&self) -> bool {
        let headers = self.headers();
        let preferred = preferred_content_type(headers);
        let wants_json = preferred.is_some_and(|media| {
            let media = media.to_ascii_lowercase();
            media.contains("/json") || media.contains("+json")
        });
        let ajax = headers
            .get("x-requested-with")
            .is_some_and(|value| value == "XMLHttpRequest");
        let pjax = headers
            .get("x-pjax")
            .is_some_and(|value| !value.is_empty() && value != "0");
        let accepts_any = preferred.is_some_and(|media| matches!(media, "*" | "*/*"))
            || !headers.contains_key(ACCEPT);
        wants_json || (ajax && !pjax && accepts_any)
    }

    /// The named attributes set by server-side application code for this request.
    pub fn attributes(&self) -> &RequestAttributes {
        &self.attributes
    }

    pub fn route(&self, name: &str) -> Option<&str> {
        self.route_parameters
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn route_model<T: std::any::Any>(&self, name: &str) -> Option<&T> {
        self.route_models.get(name)?.downcast_ref()
    }

    pub(crate) fn set_route_parameters(&mut self, parameters: Vec<(String, String)>) {
        self.route_parameters = parameters.into();
        self.route_models.clear();
        self.binding_plan = BindingPlan::default();
        context::set_parameters(&self.attributes, &self.route_parameters);
    }

    pub(crate) fn set_route_model(&mut self, name: String, model: ParameterValue) {
        context::set_model(&self.attributes, &name, &model);
        self.route_models.insert(name, model);
    }
}

fn preferred_content_type(headers: &HeaderMap) -> Option<&str> {
    let mut preferred = None;
    let mut highest_quality = 0.0;
    for value in headers
        .get_all(ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
    {
        for range in split_header(value, ',') {
            let mut parts = split_header(range, ';').map(str::trim);
            let Some(media) = parts.next().filter(|media| !media.is_empty()) else {
                continue;
            };
            let quality = parts
                .filter_map(|part| part.split_once('='))
                .find(|(name, _)| name.trim().eq_ignore_ascii_case("q"))
                .map_or(1.0, |(_, value)| {
                    value.trim().trim_matches('"').parse::<f32>().unwrap_or(0.0)
                });
            if quality > highest_quality && quality <= 1.0 {
                preferred = Some(media);
                highest_quality = quality;
            }
        }
    }
    preferred
}

fn split_header(value: &str, separator: char) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    let mut escaped = false;
    value.split(move |character| {
        if escaped {
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else {
            return character == separator && !quoted;
        }
        false
    })
}
