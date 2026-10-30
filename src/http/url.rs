//! 命名路由的 URL 生成。

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::{Error, Result};
use crate::http::domain::DomainPattern;
use crate::http::routes::NamedRouteDefinition;
use axum::extract::FromRequestParts;
use axum::http::{StatusCode, request::Parts};

#[derive(Clone)]
struct NamedRoute {
    path: String,
    domain: Option<String>,
}

/// 根据 Laravel 式命名路由生成绝对 URL。
///
/// 在 HTTP handler 中可直接提取。域参数按单个 DNS 标签校验，路径参数按路径段编码；
/// 未用于域或路径的参数成为 query 参数。scheme 和端口来自 `app.url`；
/// host 使用已登记的域模板，无域模板时来自 `app.url`，不采信当前请求的 `Host`。
#[derive(Clone)]
pub struct RouteUrlGenerator {
    inner: Arc<RouteUrlInner>,
}

struct RouteUrlInner {
    base_url: Option<::url::Url>,
    routes: BTreeMap<String, NamedRoute>,
}

impl RouteUrlGenerator {
    pub(crate) fn empty() -> Self {
        Self {
            inner: Arc::new(RouteUrlInner {
                base_url: None,
                routes: BTreeMap::new(),
            }),
        }
    }

    pub(crate) fn new(base_url: &str, routes: Vec<NamedRouteDefinition>) -> Result<Self> {
        let base_url = ::url::Url::parse(base_url).map_err(|_| {
            Error::custom("url_base_invalid", "app.url must be an absolute HTTP URL")
        })?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(Error::custom(
                "url_base_invalid",
                "app.url must use HTTP or HTTPS without credentials, query or fragment",
            ));
        }

        let mut named = BTreeMap::new();
        for NamedRouteDefinition { name, path, domain } in routes {
            if name.trim().is_empty() || name.chars().any(char::is_whitespace) {
                return Err(Error::custom(
                    "http_route_name_invalid",
                    "route names must be non-empty and contain no whitespace",
                ));
            }
            if let Some(pattern) = &domain {
                let pattern = DomainPattern::parse(pattern)?;
                for label in pattern.as_str().split('.') {
                    if let Some(parameter) = label
                        .strip_prefix('{')
                        .and_then(|part| part.strip_suffix('}'))
                        && path
                            .split('/')
                            .any(|segment| route_parameter(segment) == Some(parameter))
                    {
                        return Err(Error::custom(
                            "http_route_parameter_duplicate",
                            "domain and path parameter names must differ",
                        ));
                    }
                }
            }
            if named
                .insert(
                    name.clone(),
                    NamedRoute {
                        path: path.clone(),
                        domain,
                    },
                )
                .is_some()
            {
                return Err(Error::custom(
                    "http_route_name_duplicate",
                    "route names must be unique",
                ));
            }
            let mut normalized_path = path.as_str();
            if let Some(path) = normalized_path.strip_prefix('/') {
                normalized_path = path;
            }
            if let Some(path) = normalized_path.strip_suffix('/') {
                normalized_path = path;
            }
            if path != "/" && normalized_path.split('/').any(str::is_empty) {
                return Err(Error::custom(
                    "http_route_pattern_unsupported",
                    "named routes cannot contain empty path segments",
                ));
            }
            if path.split('/').any(|segment| {
                (segment.contains('{') || segment.contains('}') || segment.contains('*'))
                    && route_parameter(segment).is_none()
            }) {
                return Err(Error::custom(
                    "http_route_pattern_unsupported",
                    "named route parameters must occupy a complete path segment",
                ));
            }
        }

        Ok(Self {
            inner: Arc::new(RouteUrlInner {
                base_url: Some(base_url),
                routes: named,
            }),
        })
    }

    /// 按名称生成 URL；缺失的域或路径参数报错，额外参数按字典序加入 query。
    pub fn route(&self, name: &str, parameters: &BTreeMap<String, String>) -> Result<String> {
        let route = self.inner.routes.get(name).ok_or_else(|| {
            generation_error(
                "http_named_route_not_found",
                "named route was not registered",
            )
        })?;

        let mut remaining = parameters.clone();
        let mut url = self.inner.base_url.clone().ok_or_else(|| {
            Error::custom(
                "url_base_invalid",
                "app.url is unavailable for named routes",
            )
        })?;
        if let Some(domain) = &route.domain {
            let mut generated = Vec::new();
            for label in domain.split('.') {
                if let Some(parameter) = label
                    .strip_prefix('{')
                    .and_then(|part| part.strip_suffix('}'))
                {
                    let value = remaining.remove(parameter).ok_or_else(|| {
                        generation_error(
                            "http_route_parameter_missing",
                            format!("required route parameter `{parameter}` is missing"),
                        )
                    })?;
                    if !valid_domain_label(&value) {
                        return Err(generation_error(
                            "http_route_parameter_invalid",
                            format!("domain route parameter `{parameter}` must be one DNS label"),
                        ));
                    }
                    generated.push(value.to_ascii_lowercase());
                } else {
                    generated.push(label.to_owned());
                }
            }
            let host = generated.join(".");
            if DomainPattern::parse(&host).is_err() {
                return Err(generation_error(
                    "http_route_parameter_invalid",
                    "domain route parameter is not a valid DNS label",
                ));
            }
            url.set_host(Some(&host)).map_err(|_| {
                generation_error(
                    "http_route_parameter_invalid",
                    "domain route host is invalid",
                )
            })?;
        }
        let mut path_segments = url.path_segments_mut().map_err(|()| {
            Error::custom(
                "url_base_invalid",
                "app.url cannot be used as a hierarchical URL",
            )
        })?;
        path_segments.pop_if_empty();
        for segment in route
            .path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
        {
            if let Some(parameter) = route_parameter(segment) {
                let value = remaining.remove(parameter).ok_or_else(|| {
                    generation_error(
                        "http_route_parameter_missing",
                        format!("required route parameter `{parameter}` is missing"),
                    )
                })?;
                if matches!(value.as_str(), "." | "..") {
                    return Err(generation_error(
                        "http_route_parameter_invalid",
                        format!("route parameter `{parameter}` is not a valid path segment"),
                    ));
                }
                path_segments.push(&value);
            } else if segment.contains('{') || segment.contains('}') {
                return Err(generation_error(
                    "http_route_pattern_unsupported",
                    "named route parameters must occupy a complete path segment",
                ));
            } else {
                path_segments.push(segment);
            }
        }
        if route.path.ends_with('/') && route.path != "/" {
            path_segments.push("");
        }
        drop(path_segments);

        if !remaining.is_empty() {
            let mut query = url.query_pairs_mut();
            for (key, value) in remaining {
                query.append_pair(&key, &value);
            }
        }
        Ok(url.to_string())
    }
}

fn generation_error(code: impl Into<String>, message: impl Into<String>) -> Error {
    Error::custom_with_status(StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

impl<S> FromRequestParts<S> for RouteUrlGenerator
where
    S: Send + Sync,
{
    type Rejection = Error;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self>> + Send {
        std::future::ready(
            parts
                .extensions
                .get::<Self>()
                .cloned()
                .ok_or_else(route_url_unavailable),
        )
    }
}

pub(crate) fn route_url_unavailable() -> Error {
    Error::custom(
        "http_route_url_unavailable",
        "route URL generation is unavailable outside the application HTTP router",
    )
}

fn route_parameter(segment: &str) -> Option<&str> {
    let parameter = segment.strip_prefix('{')?.strip_suffix('}')?;
    (!parameter.is_empty()
        && !parameter
            .chars()
            .any(|c| matches!(c, '{' | '}' | '?' | '*')))
    .then_some(parameter)
}

fn valid_domain_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.is_ascii()
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
