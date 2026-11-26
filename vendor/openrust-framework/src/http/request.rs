use std::{
    borrow::Cow,
    sync::{Arc, OnceLock, Weak},
    time::Instant,
};

use percent_encoding::percent_decode_str;

use crate::{
    container::ParameterValue,
    routing::binding::{BindingPlan, RouteModels},
};

use super::{Body, HeaderMap, Method, RequestParts, Uri};

mod content_types;
pub(crate) mod context;
mod cookies;
mod flash_data;
mod input;
mod input_value;
mod languages;
mod metadata;
mod native;
mod precognition;
pub(crate) mod query;
mod signature;
mod validation;
pub(crate) use query::{parse as parse_query, parse_uri as parse_uri_query};
mod url;
mod variables;
pub use context::RequestAttributes;
pub use input_value::{RequestField, RequestValue};

type UserCallback = dyn for<'a> Fn(
        &'a mut Request,
        Option<&'a str>,
    ) -> super::HttpFuture<
        'a,
        crate::auth::AuthResult<Option<Arc<dyn crate::contracts::auth::Authenticatable>>>,
    > + Send
    + Sync;
/// 请求级用户解析器，可替换当前请求的默认 Guard 解析。
#[derive(Clone)]
pub struct UserResolver(pub Arc<UserCallback>);
/// 解析器返回的路由信息、参数与模型绑定。
#[derive(Clone, Default)]
pub struct ResolvedRoute {
    pub name: Option<String>,
    pub metadata: Option<crate::routing::RouteMetadata>,
    pub parameters: std::collections::BTreeMap<String, String>,
    pub models: std::collections::BTreeMap<String, ParameterValue>,
}
/// 请求级路由解析器；首次读取后缓存当前路由对象。
#[derive(Clone)]
pub struct RouteResolver(pub Arc<dyn Fn() -> ResolvedRoute + Send + Sync>);

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
    languages: OnceLock<Vec<String>>,
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
    pub(crate) exceptions: Option<Weak<crate::container::Container>>,
    pub(crate) api_prefix: Option<Arc<str>>,
    pub(crate) matched_route: bool,
    pub(crate) route_name: Option<String>,
    pub(crate) session_block: Option<crate::session::RouteBlock>,
    pub(crate) route_terminators: Vec<Arc<dyn crate::contracts::http::Middleware>>,
}

impl Request {
    /// 在不持有请求引用的异步操作中借出当前请求，完成或取消后恢复原实例。
    pub async fn with_container<T>(&mut self, resolve: impl std::future::Future<Output = T>) -> T {
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
            languages: OnceLock::new(),
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
        self.languages.take();
        self.cookie_cache.take();
        self.payload_input.take();
        &mut self.parts
    }
    /// 仅变更请求扩展时保留已解析的请求数据。
    pub(crate) fn extensions_mut(&mut self) -> &mut ::http::Extensions {
        &mut self.parts.extensions
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
    pub fn get_real_method(&self) -> &Method {
        self.real_method.as_ref().unwrap_or(&self.parts.method)
    }

    pub fn uri(&self) -> &Uri {
        self.proxy.uri.as_ref().unwrap_or(&self.parts.uri)
    }
    /// Returns the query bag or an exact top-level key, independently of the body.
    pub fn query<'a>(
        &self,
        key: impl Into<Option<&'a str>>,
    ) -> Result<Option<serde_json::Value>, super::HttpError> {
        let key = key.into();
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

    /// 读取查询参数的精确顶层键，缺失或 null 时采用默认值。
    pub fn query_or(
        &self,
        key: &str,
        default: impl Into<serde_json::Value>,
    ) -> Result<serde_json::Value, super::HttpError> {
        Ok(self
            .query(key)?
            .filter(|value| !value.is_null())
            .unwrap_or_else(|| default.into()))
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

    pub fn user_agent(&self) -> Option<&str> {
        self.headers()
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
    }

    pub async fn user(
        &mut self,
        guard: Option<&str>,
    ) -> crate::auth::AuthResult<Option<Arc<dyn crate::contracts::auth::Authenticatable>>> {
        if let Some(resolver) = self.get_user_resolver() {
            return (resolver.0)(self, guard).await;
        }
        crate::support::facades::Auth::manager()?
            .user_for_guard(self, guard)
            .await
    }

    pub fn fingerprint(&self) -> Result<String, super::HttpError> {
        let route = self
            .route_metadata()
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

    pub fn has_header(&self, name: &str) -> bool {
        self.headers().contains_key(name)
    }

    /// Extracts the token after the last ASCII case-insensitive Bearer marker.
    pub fn bearer_token(&self) -> Result<Option<&str>, super::HttpError> {
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
    pub fn has_cookie(&self, key: &str) -> Result<bool, super::HttpError> {
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

    pub fn has_session(&self) -> bool {
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

    pub(crate) fn attach_session(&mut self, session: crate::session::Store) {
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

    pub fn get_client_ip(&self) -> Option<std::net::IpAddr> {
        self.ip()
    }

    pub fn get_client_ips(&self) -> &[std::net::IpAddr] {
        self.ips()
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

    pub fn get_base_url(&self) -> &str {
        &self.proxy.prefix
    }

    /// 返回应用路径，保留开头斜杠及已有编码，不包含可信代理的部署前缀。
    pub fn get_path_info(&self) -> &str {
        self.uri().path()
    }

    /// Returns the raw URL path without surrounding slashes, or `/` for the root.
    pub fn path(&self) -> &str {
        let path = self.uri().path().trim_matches('/');
        if path.is_empty() { "/" } else { path }
    }

    /// Percent-decodes the request path.
    pub fn decoded_path(&self) -> Cow<'_, str> {
        percent_decode_str(self.path()).decode_utf8_lossy()
    }

    /// Returns the nonempty segments of the decoded path.
    pub fn segments(&self) -> Vec<String> {
        self.decoded_path()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Returns the decoded segment at a one-based index.
    pub fn segment(&self, index: usize) -> Option<String> {
        self.decoded_path()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .nth(index.checked_sub(1)?)
            .map(str::to_owned)
    }

    pub fn is_method(&self, method: &str) -> bool {
        self.method().as_str().eq_ignore_ascii_case(method)
    }

    /// 当前解析路由的名称；尊重自定义 RouteResolver。
    pub fn route_name(&self) -> Option<&str> {
        if let Some(route) = self.custom_resolved_route() {
            route.name.as_deref()
        } else {
            self.route_name.as_deref()
        }
    }

    /// Matches the resolved route name against any supplied wildcard pattern.
    pub fn route_is(&self, patterns: &[&str]) -> bool {
        let name = self.route_name();
        name.is_some_and(|name| {
            patterns
                .iter()
                .any(|pattern| matches_pattern(name, pattern))
        })
    }

    /// Matches the decoded request path against a pattern where `*` spans any characters.
    pub fn is(&self, pattern: &str) -> bool {
        matches_pattern(&self.decoded_path(), pattern)
    }

    /// 匹配 API 前缀本身及其子路径；空前缀不按路径匹配。
    pub fn is_api_path(&self) -> bool {
        self.api_prefix
            .as_deref()
            .filter(|prefix| !prefix.is_empty())
            .is_some_and(|prefix| {
                let path = self.decoded_path();
                path == prefix
                    || path
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
                &format!("{scheme}://{authority}{}{target}", self.get_base_url()),
                pattern,
            )
        })
    }

    /// 当前请求实际执行的语言中间件选择状态；未启用时为空。
    /// 当前实际语言请使用 App::current_locale。
    pub fn locale_selection(&self) -> Option<&super::middleware::LocaleSelection> {
        self.extensions()
            .get::<super::middleware::LocaleSelection>()
    }

    /// The named attributes set by server-side application code for this request.
    pub fn attributes(&self) -> &RequestAttributes {
        &self.attributes
    }

    pub fn route(&self, name: &str) -> Option<&str> {
        if self.get_route_resolver().is_some() {
            return self.resolved_route_value(name);
        }
        self.route_parameters
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn route_parameters(&self) -> &[(String, String)] {
        &self.route_parameters
    }

    pub fn route_model<T: std::any::Any>(&self, name: &str) -> Option<&T> {
        self.route_binding_value(name)?.downcast_ref()
    }

    pub(crate) fn route_binding_value(&self, name: &str) -> Option<&ParameterValue> {
        if let Some(route) = self.custom_resolved_route() {
            return route.models.get(name);
        }
        self.route_models.get(name)
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
