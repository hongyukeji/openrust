use base64::Engine as _;
use http::{Method, header};

use super::{Request, content_types};
use crate::{contracts::session::Session as _, http::HttpError};

#[derive(Clone, Default)]
struct Metadata {
    locale: Option<String>,
    default_locale: Option<String>,
    format: Option<String>,
    formats: indexmap::IndexMap<String, Vec<String>>,
}

struct ResolvedRoutes {
    resolver: super::RouteResolver,
    values: std::sync::OnceLock<super::ResolvedRoute>,
}

impl Request {
    fn metadata(&self) -> Metadata {
        self.extensions()
            .get::<Metadata>()
            .cloned()
            .unwrap_or_default()
    }
    fn update_metadata(&mut self, update: impl FnOnce(&mut Metadata)) {
        let mut metadata = self.metadata();
        update(&mut metadata);
        self.parts.extensions.insert(metadata);
    }
    pub fn set_method(&mut self, method: Method) {
        self.real_method = None;
        self.parts.method = method;
    }
    pub fn get_method(&self) -> &Method {
        self.method()
    }
    pub fn is_method_safe(&self) -> bool {
        matches!(
            *self.method(),
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
        )
    }
    pub fn is_method_idempotent(&self) -> bool {
        self.is_method_safe() || matches!(*self.method(), Method::PUT | Method::DELETE)
    }
    pub fn is_method_cacheable(&self) -> bool {
        matches!(*self.method(), Method::GET | Method::HEAD)
    }
    pub fn get_scheme(&self) -> &'static str {
        if self.secure() { "https" } else { "http" }
    }
    pub fn get_port(&self) -> Result<u16, HttpError> {
        let authority: http::uri::Authority = self
            .http_host()?
            .parse()
            .map_err(|_| HttpError::InvalidRequest)?;
        Ok(authority
            .port_u16()
            .unwrap_or(if self.secure() { 443 } else { 80 }))
    }
    pub fn get_host(&self) -> Result<String, HttpError> {
        self.host()
    }
    pub fn get_http_host(&self) -> Result<String, HttpError> {
        self.http_host()
    }
    pub fn get_scheme_and_http_host(&self) -> Result<String, HttpError> {
        self.scheme_and_http_host()
    }
    pub fn get_request_uri(&self) -> String {
        self.uri()
            .path_and_query()
            .map_or("/", |value| value.as_str())
            .to_owned()
    }
    pub fn get_uri(&self) -> Result<String, HttpError> {
        self.full_url()
    }
    pub fn get_uri_for_path(&self, path: &str) -> Result<String, HttpError> {
        if !path.starts_with('/') {
            return Err(HttpError::InvalidRequest);
        }
        Ok(format!("{}{}", self.root()?, path))
    }
    pub fn get_relative_uri_for_path(&self, path: &str) -> Result<String, HttpError> {
        if !path.starts_with('/') {
            return Ok(path.to_owned());
        }
        let source = url::Url::parse(&self.full_url()?).map_err(|_| HttpError::InvalidRequest)?;
        let target = source.join(path).map_err(|_| HttpError::InvalidRequest)?;
        source
            .make_relative(&target)
            .ok_or(HttpError::InvalidRequest)
    }
    pub fn get_query_string(&self) -> Result<Option<String>, HttpError> {
        let query = Self::normalize_query_string(self.uri().query().unwrap_or_default())?;
        Ok((!query.is_empty()).then_some(query))
    }
    // 原生 HTTP 服务没有 CGI 脚本入口，基础路径来自可信代理前缀。
    pub fn get_script_name(&self) -> &'static str {
        ""
    }
    pub fn get_base_path(&self) -> &str {
        self.get_base_url()
    }
    pub fn get_user_info(&self) -> Option<String> {
        let encoded = self.header("authorization").ok()??;
        let (scheme, data) = encoded.split_once(' ')?;
        if !scheme.eq_ignore_ascii_case("basic") {
            return None;
        }
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .ok()?,
        )
        .ok()
    }
    pub fn get_user(&self) -> Option<String> {
        self.get_user_info()?
            .split_once(':')
            .map(|(user, _)| user.to_owned())
    }
    pub fn get_password(&self) -> Option<String> {
        self.get_user_info()?
            .split_once(':')
            .map(|(_, password)| password.to_owned())
    }
    pub fn get_protocol_version(&self) -> String {
        format!("{:?}", self.version())
    }
    pub fn get_e_tags(&self) -> Vec<String> {
        self.header("if-none-match")
            .ok()
            .flatten()
            .map_or_else(Vec::new, |value| {
                value.split(',').map(|tag| tag.trim().to_owned()).collect()
            })
    }
    pub fn is_no_cache(&self) -> bool {
        self.header("cache-control")
            .ok()
            .flatten()
            .is_some_and(|value| {
                value.split(',').any(|item| {
                    item.trim()
                        .split('=')
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case("no-cache"))
                })
            })
            || self
                .header("pragma")
                .ok()
                .flatten()
                .is_some_and(|value| value.eq_ignore_ascii_case("no-cache"))
    }
    pub fn is_xml_http_request(&self) -> bool {
        self.ajax()
    }
    pub fn prefer_safe_content(&self) -> bool {
        self.secure()
            && self
                .header("prefer")
                .ok()
                .flatten()
                .is_some_and(|value| value.split(',').any(|item| item.trim() == "safe"))
    }
    pub fn is_from_trusted_proxy(&self) -> bool {
        self.proxy.trusted
    }
    pub fn get_charsets(&self) -> Vec<String> {
        self.accepted_tokens(header::ACCEPT_CHARSET)
    }
    pub fn get_encodings(&self) -> Vec<String> {
        self.accepted_tokens(header::ACCEPT_ENCODING)
    }
    fn accepted_tokens(&self, header: header::HeaderName) -> Vec<String> {
        let mut headers = http::HeaderMap::new();
        if let Some(value) = self.headers().get(header) {
            headers.insert(header::ACCEPT, value.clone());
        }
        content_types::parse_accept(&headers)
    }
    pub fn set_locale(&mut self, locale: &str) -> crate::translation::TranslationResult<()> {
        let locale = crate::translation::canonical_locale(locale)?;
        self.update_metadata(|metadata| metadata.locale = Some(locale));
        Ok(())
    }
    pub fn get_locale(&self) -> String {
        self.metadata()
            .locale
            .unwrap_or_else(|| self.get_default_locale())
    }
    pub fn set_default_locale(
        &mut self,
        locale: &str,
    ) -> crate::translation::TranslationResult<()> {
        let locale = crate::translation::canonical_locale(locale)?;
        self.update_metadata(|metadata| metadata.default_locale = Some(locale));
        Ok(())
    }
    pub fn get_default_locale(&self) -> String {
        self.metadata()
            .default_locale
            .unwrap_or_else(|| "en".to_owned())
    }
    pub fn set_request_locale(
        &mut self,
        locale: &str,
    ) -> crate::translation::TranslationResult<()> {
        self.set_locale(locale)
    }
    pub fn set_default_request_locale(
        &mut self,
        locale: &str,
    ) -> crate::translation::TranslationResult<()> {
        self.set_default_locale(locale)
    }
    pub fn set_request_format(&mut self, format: Option<&str>) {
        self.update_metadata(|metadata| metadata.format = format.map(str::to_owned));
    }
    pub fn get_request_format(&self, default: Option<&str>) -> Option<String> {
        self.metadata()
            .format
            .or_else(|| self.route("_format").map(str::to_owned))
            .or_else(|| default.map(str::to_owned))
    }
    pub fn get_mime_types(&self, format: &str) -> Vec<String> {
        self.metadata()
            .formats
            .get(format)
            .cloned()
            .unwrap_or_else(|| {
                content_types::FORMATS
                    .iter()
                    .find(|(name, _)| *name == format)
                    .map_or_else(Vec::new, |(_, values)| {
                        values.iter().map(|value| (*value).to_owned()).collect()
                    })
            })
    }
    pub fn get_mime_type(&self, format: &str) -> Option<String> {
        self.get_mime_types(format).first().cloned()
    }
    pub fn set_format(&mut self, format: &str, mime_types: Vec<String>) {
        self.update_metadata(|metadata| {
            metadata.formats.insert(format.to_owned(), mime_types);
        });
    }
    pub fn get_format(&self, mime_type: &str, subtype_fallback: bool) -> Option<String> {
        let canonical = mime_type.split(';').next()?.trim();
        let metadata = self.metadata();
        metadata
            .formats
            .iter()
            .rev()
            .find(|(_, values)| values.iter().any(|value| value == mime_type))
            .or_else(|| {
                metadata
                    .formats
                    .iter()
                    .rev()
                    .find(|(_, values)| values.iter().any(|value| value == canonical))
            })
            .map(|(name, _)| name.clone())
            .or_else(|| {
                content_types::FORMATS
                    .iter()
                    .rev()
                    .find(|(name, values)| {
                        !metadata.formats.contains_key(*name) && values.contains(&canonical)
                    })
                    .map(|(name, _)| (*name).to_owned())
            })
            .or_else(|| content_types::format_for_suffix(canonical).map(str::to_owned))
            .or_else(|| {
                if !subtype_fallback {
                    return None;
                }
                let (_, subtype) = canonical.split_once('/')?;
                (!subtype.contains('+'))
                    .then(|| subtype.strip_prefix("x-").unwrap_or(subtype).to_owned())
            })
    }
    pub fn get_content_type_format(&self) -> Option<String> {
        self.get_format(self.header("content-type").ok()??, false)
    }
    pub fn get_preferred_format(&self, default: Option<&str>) -> Option<String> {
        self.get_request_format(None)
            .or_else(|| {
                self.get_acceptable_content_types()
                    .iter()
                    .find_map(|value| self.get_format(value, true))
            })
            .or_else(|| default.map(str::to_owned))
    }
    pub fn set_laravel_session(&mut self, session: crate::session::Store) {
        self.attach_session(session);
    }
    pub fn set_session(&mut self, session: crate::session::Store) {
        self.attach_session(session);
    }
    pub fn has_previous_session(&self) -> bool {
        self.session()
            .ok()
            .is_some_and(|session| self.has_cookie(session.get_name()).unwrap_or(false))
    }
    pub fn set_json(&mut self, json: serde_json::Value) -> Result<(), HttpError> {
        if !json.is_object() && !json.is_array() {
            return Err(HttpError::InvalidRequest);
        }
        self.json_input = Some(Ok(json));
        Ok(())
    }
    pub fn server(&self, key: Option<&str>) -> serde_json::Value {
        let mut values = serde_json::json!({"REQUEST_METHOD":self.get_real_method().as_str(),"REQUEST_URI":self.get_request_uri(),"SERVER_PROTOCOL":self.get_protocol_version(),"REMOTE_ADDR":self.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>().map(|info|info.0.ip().to_string())});
        for (name, value) in self.headers() {
            if let Ok(value) = value.to_str() {
                let name = name.as_str().replace('-', "_").to_ascii_uppercase();
                let key = if matches!(name.as_str(), "CONTENT_TYPE" | "CONTENT_LENGTH") {
                    name
                } else {
                    format!("HTTP_{name}")
                };
                values[key] = value.into();
            }
        }
        key.map_or(values.clone(), |key| {
            values.get(key).cloned().unwrap_or_default()
        })
    }
    pub fn set_user_resolver(&mut self, callback: super::UserResolver) {
        self.parts.extensions.insert(callback);
    }
    pub fn get_user_resolver(&self) -> Option<super::UserResolver> {
        self.extensions().get::<super::UserResolver>().cloned()
    }
    pub fn set_route_resolver(&mut self, callback: super::RouteResolver) {
        self.parts
            .extensions
            .insert(std::sync::Arc::new(ResolvedRoutes {
                resolver: callback,
                values: Default::default(),
            }));
    }
    pub fn get_route_resolver(&self) -> Option<super::RouteResolver> {
        self.extensions()
            .get::<std::sync::Arc<ResolvedRoutes>>()
            .map(|state| state.resolver.clone())
    }
    pub(super) fn custom_resolved_route(&self) -> Option<&super::ResolvedRoute> {
        let state = self.extensions().get::<std::sync::Arc<ResolvedRoutes>>()?;
        Some(state.values.get_or_init(|| (state.resolver.0)()))
    }
    pub(super) fn resolved_route_value(&self, key: &str) -> Option<&str> {
        self.custom_resolved_route()?
            .parameters
            .get(key)
            .map(String::as_str)
    }
    pub fn route_metadata(&self) -> Option<&crate::routing::RouteMetadata> {
        if let Some(route) = self.custom_resolved_route() {
            route.metadata.as_ref()
        } else {
            self.extensions().get::<crate::routing::RouteMetadata>()
        }
    }
    pub fn resolved_route(&self) -> Option<super::ResolvedRoute> {
        if let Some(route) = self.custom_resolved_route() {
            return Some(route.clone());
        }
        if !self.matched_route {
            return None;
        }
        Some(super::ResolvedRoute {
            name: self.route_name.clone(),
            metadata: self.route_metadata().cloned(),
            parameters: self.route_parameters.iter().cloned().collect(),
            models: self
                .route_models
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        })
    }
}
