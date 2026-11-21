use std::{
    borrow::Cow,
    sync::{Arc, OnceLock},
    time::Instant,
};

use percent_encoding::percent_decode_str;

use crate::{
    container::ParameterValue,
    foundation::exceptions::Handler,
    routing::binding::{BindingPlan, RouteModels},
};

use super::{Body, HeaderMap, Method, RequestParts, Uri};

mod content_types;
pub(crate) mod context;
mod cookies;
mod flash_data;
mod input;
mod input_value;
mod native;
mod precognition;
pub(crate) mod query;
pub(crate) use query::{parse as parse_query, parse_uri as parse_uri_query};
mod url;
mod variables;
pub use context::RequestAttributes;
pub use input_value::{RequestField, RequestValue};

/// An incoming HTTP request with owned metadata and body.
pub struct Request {
    parts: RequestParts,
    pub(crate) proxy: super::ProxyRequest,
    real_method: Option<Method>,
    body: Body,
    pub(crate) received_at: Instant,
    query_cache: OnceLock<serde_json::Value>,
    query_input: Option<serde_json::Value>,
    acceptable_content_types: OnceLock<Vec<String>>,
    cookie_cache: OnceLock<serde_json::Value>,
    decrypted_cookies: Option<serde_json::Value>,
    pub(crate) cookie_encryption: Option<crate::cookie::CookieEncryption>,
    session: Option<crate::session::Store>,
    raw_input: input::RawBody,
    payload_input: Option<Result<input::Payload, input::InputFailure>>,
    form_input: Option<serde_json::Value>,
    json_input: Option<Result<serde_json::Value, input::InputFailure>>,
    attributes: RequestAttributes,
    route_parameters: context::RouteParameters,
    pub(crate) route_models: RouteModels,
    pub(crate) binding_plan: BindingPlan,
    pub(crate) exceptions: Option<Arc<Handler>>,
    pub(crate) api_prefix: Option<Arc<str>>,
    pub(crate) matched_route: bool,
    pub(crate) route_name: Option<String>,
    pub(crate) session_block: Option<crate::session::RouteBlock>,
    pub(crate) route_terminators: Vec<Arc<dyn crate::contracts::http::Middleware>>,
}

impl Request {
    /// Resolves constructor or method dependencies that need this HTTP request.
    #[allow(non_snake_case, reason = "Matches the container assembly API")]
    pub async fn withContainer<T>(&mut self, resolve: impl std::future::Future<Output = T>) -> T {
        context::resolving(self, resolve).await
    }

    pub fn from_parts(parts: RequestParts, body: Body) -> Self {
        Self {
            parts,
            proxy: Default::default(),
            real_method: None,
            body,
            received_at: Instant::now(),
            query_cache: OnceLock::new(),
            query_input: None,
            acceptable_content_types: OnceLock::new(),
            cookie_cache: OnceLock::new(),
            decrypted_cookies: None,
            cookie_encryption: None,
            session: None,
            raw_input: input::RawBody::Unread,
            payload_input: None,
            form_input: None,
            json_input: None,
            attributes: RequestAttributes::default(),
            route_parameters: Arc::default(),
            route_models: RouteModels::new(),
            binding_plan: BindingPlan::default(),
            exceptions: None,
            api_prefix: None,
            matched_route: false,
            route_name: None,
            session_block: None,
            route_terminators: Vec::new(),
        }
    }
    pub fn parts(&self) -> &RequestParts {
        &self.parts
    }
    pub fn parts_mut(&mut self) -> &mut RequestParts {
        self.query_cache.take();
        self.acceptable_content_types.take();
        self.cookie_cache.take();
        self.payload_input.take();
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

    /// Returns the transport method before a POST method override.
    #[allow(non_snake_case, reason = "Matches the Laravel Request API")]
    pub fn getRealMethod(&self) -> &Method {
        self.real_method.as_ref().unwrap_or(&self.parts.method)
    }

    pub fn uri(&self) -> &Uri {
        self.proxy.uri.as_ref().unwrap_or(&self.parts.uri)
    }
    /// Returns the query bag or an exact top-level key, independently of the body.
    pub fn query(&self, key: Option<&str>) -> Result<Option<serde_json::Value>, super::HttpError> {
        let values = self.query_values()?;
        let value = match (key, values) {
            (None, values) => Some(values),
            (Some(key), serde_json::Value::Object(fields)) => fields.get(key),
            (Some(key), serde_json::Value::Array(fields)) => key
                .parse::<usize>()
                .ok()
                .filter(|index| index.to_string() == key)
                .and_then(|index| fields.get(index)),
            _ => None,
        };
        Ok(value.cloned())
    }

    fn query_values(&self) -> Result<&serde_json::Value, super::HttpError> {
        if let Some(values) = self.query_input.as_ref().or_else(|| self.query_cache.get()) {
            return Ok(values);
        }
        let parsed = query::parse(self.uri().query().unwrap_or_default())?;
        Ok(self.query_cache.get_or_init(|| parsed))
    }

    pub fn ajax(&self) -> bool {
        self.headers()
            .get("x-requested-with")
            .is_some_and(|value| value == "XMLHttpRequest")
    }

    pub fn pjax(&self) -> bool {
        self.headers()
            .get("x-pjax")
            .is_some_and(|value| !value.is_empty() && value != "0")
    }

    pub fn prefetch(&self) -> bool {
        ["x-moz", "purpose", "sec-purpose"].into_iter().any(|name| {
            self.headers()
                .get(name)
                .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"prefetch"))
        })
    }

    #[allow(non_snake_case, reason = "遵循 Laravel Request 命名")]
    pub fn userAgent(&self) -> Option<&str> {
        self.headers()
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
    }

    pub async fn user(
        &mut self,
        guard: Option<&str>,
    ) -> crate::auth::AuthResult<Option<Arc<dyn crate::contracts::auth::Authenticatable>>> {
        crate::support::facades::Auth::manager()?
            .userForGuard(self, guard)
            .await
    }

    pub fn fingerprint(&self) -> Result<String, super::HttpError> {
        let route = self
            .parts
            .extensions
            .get::<crate::routing::RouteMetadata>()
            .ok_or_else(|| super::HttpError::Internal {
                source: std::io::Error::other("无法生成请求指纹：尚未匹配路由").into(),
            })?;
        let mut parts = route.methods.clone();
        parts.extend([
            route.domain.clone().unwrap_or_default(),
            if route.uri == "/" {
                "/".to_owned()
            } else {
                route.uri.trim_matches('/').to_owned()
            },
            self.ip().map(|ip| ip.to_string()).unwrap_or_default(),
        ]);
        let hash = aws_lc_rs::digest::digest(
            &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
            parts.join("|").as_bytes(),
        );
        Ok(hash
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.parts.headers
    }

    /// Returns the first text value for a header.
    pub fn header(&self, name: &str) -> Result<Option<&str>, super::HttpError> {
        let name = ::http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| super::HttpError::InvalidRequest)?;
        self.headers()
            .get(name)
            .map(|value| {
                std::str::from_utf8(value.as_bytes()).map_err(|_| super::HttpError::InvalidRequest)
            })
            .transpose()
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn hasHeader(&self, name: &str) -> bool {
        self.headers().contains_key(name)
    }

    /// Extracts the token after the last ASCII case-insensitive Bearer marker.
    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn bearerToken(&self) -> Result<Option<&str>, super::HttpError> {
        Ok(self.header("authorization")?.and_then(|header| {
            let position = header
                .as_bytes()
                .windows(7)
                .rposition(|window| window.eq_ignore_ascii_case(b"Bearer "))?;
            let token = &header[position + 7..];
            Some(token.split_once(',').map_or(token, |(token, _)| token))
        }))
    }

    /// Reads the cookie bag or an exact top-level cookie name.
    pub fn cookie(&self, key: Option<&str>) -> Result<Option<serde_json::Value>, super::HttpError> {
        Ok(self.cookie_value(key)?.cloned())
    }

    /// An empty cookie is present. Missing values and failed decryptions are absent.
    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn hasCookie(&self, key: &str) -> Result<bool, super::HttpError> {
        Ok(self.cookie_value(Some(key))?.is_some())
    }

    fn cookie_value(
        &self,
        key: Option<&str>,
    ) -> Result<Option<&serde_json::Value>, super::HttpError> {
        let cookies = match self
            .decrypted_cookies
            .as_ref()
            .or_else(|| self.cookie_cache.get())
        {
            Some(cookies) => cookies,
            None => {
                let parsed = cookies::parse(self.headers())?;
                self.cookie_cache.get_or_init(|| parsed)
            }
        };
        Ok(match (key, cookies) {
            (None, cookies) => Some(cookies),
            (Some(key), serde_json::Value::Object(cookies)) => cookies.get(key),
            (Some(key), serde_json::Value::Array(cookies)) => key
                .parse::<usize>()
                .ok()
                .filter(|index| index.to_string() == key)
                .and_then(|index| cookies.get(index)),
            _ => None,
        }
        .filter(|value| key.is_none() || !value.is_null()))
    }

    pub(crate) fn replace_cookies(
        &mut self,
        cookies: Option<serde_json::Value>,
    ) -> Option<serde_json::Value> {
        std::mem::replace(&mut self.decrypted_cookies, cookies)
    }

    pub(crate) fn cookie_is_protected(&self, name: &str) -> bool {
        self.cookie_encryption
            .as_ref()
            .is_some_and(|encryption| !encryption.exceptions.contains(name))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn hasSession(&self) -> bool {
        self.session.is_some()
    }

    pub fn session(&self) -> Result<&crate::session::Store, crate::session::SessionError> {
        self.session
            .as_ref()
            .ok_or(crate::session::SessionError::Configuration)
    }

    pub fn session_mut(
        &mut self,
    ) -> Result<&mut crate::session::Store, crate::session::SessionError> {
        self.session
            .as_mut()
            .ok_or(crate::session::SessionError::Configuration)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub(crate) fn setLaravelSession(&mut self, session: crate::session::Store) {
        context::set_session_cache(self, Some(session.cache_attributes().clone()));
        self.session = Some(session);
    }

    pub(crate) fn take_session(&mut self) -> Option<crate::session::Store> {
        context::set_session_cache(self, None);
        self.session.take()
    }

    pub(crate) fn is_secure(&self) -> bool {
        self.secure()
    }

    pub fn secure(&self) -> bool {
        self.proxy
            .secure
            .unwrap_or_else(|| self.uri().scheme_str() == Some("https"))
    }

    pub fn ip(&self) -> Option<std::net::IpAddr> {
        self.ips().first().copied()
    }

    pub fn ips(&self) -> &[std::net::IpAddr] {
        self.proxy.ips.as_deref().unwrap_or_else(|| {
            self.parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|info| self.proxy.peer.get_or_init(|| info.0.ip()))
                .map_or(&[], std::slice::from_ref)
        })
    }

    #[allow(non_snake_case, reason = "Matches Laravel's request URL API")]
    pub fn getBaseUrl(&self) -> &str {
        &self.proxy.prefix
    }

    /// Returns the raw URL path without surrounding slashes, or `/` for the root.
    pub fn path(&self) -> &str {
        let path = self.uri().path().trim_matches('/');
        if path.is_empty() { "/" } else { path }
    }

    /// Percent-decodes the request path.
    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn decodedPath(&self) -> Cow<'_, str> {
        percent_decode_str(self.path()).decode_utf8_lossy()
    }

    /// Returns the nonempty segments of the decoded path.
    pub fn segments(&self) -> Vec<String> {
        self.decodedPath()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Returns the decoded segment at a one-based index.
    pub fn segment(&self, index: usize) -> Option<String> {
        self.decodedPath()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .nth(index.checked_sub(1)?)
            .map(str::to_owned)
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn isMethod(&self, method: &str) -> bool {
        self.method().as_str().eq_ignore_ascii_case(method)
    }

    /// Matches the resolved route name against any supplied wildcard pattern.
    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub fn routeIs(&self, patterns: &[&str]) -> bool {
        self.route_name.as_deref().is_some_and(|name| {
            patterns
                .iter()
                .any(|pattern| matches_pattern(name, pattern))
        })
    }

    /// Matches the decoded request path against a pattern where `*` spans any characters.
    pub fn is(&self, pattern: &str) -> bool {
        matches_pattern(&self.decodedPath(), pattern)
    }

    /// Matches paths below the API prefix. An empty prefix has no path-based match.
    #[allow(non_snake_case, reason = "Matches the application routing API")]
    pub fn isApiPath(&self) -> bool {
        self.api_prefix
            .as_deref()
            .filter(|prefix| !prefix.is_empty())
            .is_some_and(|prefix| {
                self.decodedPath()
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            })
    }

    pub(crate) fn matches_url_or_path(&self, pattern: &str) -> bool {
        let pattern = if pattern == "/" {
            pattern
        } else {
            pattern.trim_matches('/')
        };
        if self.is(pattern) {
            return true;
        }
        let authority = self
            .uri()
            .authority()
            .map(|value| value.as_str())
            .or_else(|| self.headers().get(::http::header::HOST)?.to_str().ok());
        authority.is_some_and(|authority| {
            let scheme = self.uri().scheme_str().unwrap_or("http");
            let target = self
                .uri()
                .path_and_query()
                .map_or("/", ::http::uri::PathAndQuery::as_str);
            matches_pattern(
                &format!("{scheme}://{authority}{}{target}", self.getBaseUrl()),
                pattern,
            )
        })
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

pub(crate) fn matches_pattern(path: &str, pattern: &str) -> bool {
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
