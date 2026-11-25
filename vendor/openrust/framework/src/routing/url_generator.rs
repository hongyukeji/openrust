use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use aws_lc_rs::hmac;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use subtle::ConstantTimeEq;

use crate::{
    config::Value,
    contracts::{
        config::Repository,
        routing::{UrlGenerator as Contract, UrlRoutable},
    },
    http::{HttpError, Request, Uri},
};

use super::RouteUrlGenerator;

#[derive(Default)]
pub(crate) struct UrlRoutes {
    pub(super) named: HashMap<String, RouteUrlGenerator>,
    pub(super) actions: HashMap<String, RouteUrlGenerator>,
}

type UrlFormatter = dyn Fn(&str, Option<&RouteUrlGenerator>) -> String + Send + Sync;

/// Generates named routes and signed URLs for the application.
#[derive(Clone)]
pub struct UrlGenerator {
    routes: Arc<UrlRoutes>,
    owner: std::sync::Weak<crate::container::Container>,
    config: Arc<dyn Repository>,
    uri: Option<Uri>,
    base_url: String,
    previous: Option<String>,
    defaults: HashMap<String, String>,
    force_scheme: Option<String>,
    force_root: Option<String>,
    asset_origin: Option<Option<String>>,
    host_formatter: Option<Arc<UrlFormatter>>,
    pub(super) path_formatter: Option<Arc<UrlFormatter>>,
}

/// Route parameters in application-supplied order.
pub type RouteParameters<'a> = Vec<RouteParameter<'a>>;

/// A positional or named route or query parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteParameter<'a> {
    Positional(UrlValue<'a>),
    Named(String, UrlValue<'a>),
}

/// The scalar and nested values accepted by the URL encoder.
#[derive(Clone)]
pub enum UrlValue<'a> {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(RouteParameters<'a>),
    #[doc(hidden)]
    Model(&'a dyn UrlRoutable),
}

impl PartialEq for UrlValue<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (Self::Number(left), Self::Number(right)) => left == right,
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Array(left), Self::Array(right)) => left == right,
            (Self::Model(left), Self::Model(right)) => std::ptr::eq(*left, *right),
            _ => false,
        }
    }
}

impl fmt::Debug for UrlValue<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => formatter.write_str("Null"),
            Self::Bool(value) => formatter.debug_tuple("Bool").field(value).finish(),
            Self::Number(value) => formatter.debug_tuple("Number").field(value).finish(),
            Self::String(value) => formatter.debug_tuple("String").field(value).finish(),
            Self::Array(value) => formatter.debug_tuple("Array").field(value).finish(),
            Self::Model(_) => formatter.debug_struct("Model").finish_non_exhaustive(),
        }
    }
}

/// Selects whether a signature includes the origin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureMode {
    Absolute,
    Relative,
}

/// Query keys excluded during signature verification.
pub enum SignatureQueryFilter<'a> {
    Keys(&'a [&'a str]),
    Predicate(&'a (dyn Fn(&str) -> bool + Send + Sync)),
}

impl<'a> From<&'a [&'a str]> for SignatureQueryFilter<'a> {
    fn from(keys: &'a [&'a str]) -> Self {
        Self::Keys(keys)
    }
}
impl<'a, const N: usize> From<&'a [&'a str; N]> for SignatureQueryFilter<'a> {
    fn from(keys: &'a [&'a str; N]) -> Self {
        Self::Keys(keys)
    }
}
impl<'a> From<&'a Vec<&'a str>> for SignatureQueryFilter<'a> {
    fn from(keys: &'a Vec<&'a str>) -> Self {
        Self::Keys(keys)
    }
}

pub type UrlResult<T> = Result<T, UrlError>;

/// A failure to construct a URL, signature, or redirect.
#[derive(Debug)]
#[non_exhaustive]
pub enum UrlError {
    MissingRoute,
    MissingAction,
    MissingParameter,
    InvalidParameter,
    InvalidOrigin,
    ModelAttribute(crate::database::DatabaseError),
    MissingSigningKey,
    ReservedSignatureParameter,
    InvalidRedirect,
    Unavailable,
    Session(crate::session::SessionError),
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingAction => "Controller action is not defined.",
            Self::MissingRoute => "Named route is not defined.",
            Self::MissingParameter => "Required route parameter is missing.",
            Self::InvalidParameter => "Invalid URL parameter.",
            Self::ModelAttribute(_) => "Could not read the URL model attribute.",
            Self::InvalidOrigin => "Invalid HTTP URL origin.",
            Self::MissingSigningKey => "URL signing key is missing.",
            Self::ReservedSignatureParameter => "URL parameter is reserved for signing.",
            Self::InvalidRedirect => "Invalid redirect target or status.",
            Self::Unavailable => "URL service is unavailable.",
            Self::Session(error) => return error.fmt(f),
        })
    }
}

impl std::error::Error for UrlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ModelAttribute(error) => Some(error),
            _ => None,
        }
    }
}

impl From<UrlError> for HttpError {
    fn from(source: UrlError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<&str> for UrlValue<'_> {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for UrlValue<'_> {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

macro_rules! integer_url_values {
    ($($integer:ty),+ $(,)?) => {$(
        impl From<$integer> for UrlValue<'_> {
            fn from(value: $integer) -> Self {
                Self::Number(value.into())
            }
        }
    )+};
}
integer_url_values!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl From<bool> for UrlValue<'_> {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl UrlValue<'_> {
    pub(super) fn scalar(&self) -> UrlResult<Option<String>> {
        Ok(match self {
            Self::Null | Self::Array(_) => None,
            Self::Model(model) => {
                return model.get_route_key()?.map_or(Ok(None), |key| key.scalar());
            }
            Self::Bool(value) => Some(u8::from(*value).to_string()),
            Self::Number(value) => Some(value.to_string()),
            Self::String(value) => Some(value.clone()),
        })
    }

    pub(super) fn empty(&self) -> bool {
        matches!(self, Self::Null) || matches!(self, Self::String(value) if value.is_empty())
    }
}

impl<'a> RouteParameter<'a> {
    pub fn named(name: impl Into<String>, value: impl Into<UrlValue<'a>>) -> Self {
        Self::Named(name.into(), value.into())
    }

    pub fn positional(value: impl Into<UrlValue<'a>>) -> Self {
        Self::Positional(value.into())
    }

    /// Borrows the model until the URL is generated.
    pub fn model(name: impl Into<String>, model: &'a impl UrlRoutable) -> UrlResult<Self> {
        Ok(Self::named(name, UrlValue::Model(model)))
    }

    /// Borrows the model for the next positional route parameter.
    pub fn positional_model(model: &'a impl UrlRoutable) -> UrlResult<Self> {
        Ok(Self::positional(UrlValue::Model(model)))
    }

    fn name(&self) -> Option<&str> {
        match self {
            Self::Named(name, _) => Some(name),
            Self::Positional(_) => None,
        }
    }
}

const URL_DEFAULTS: &str = "openrust.routing.url.defaults";

impl UrlGenerator {
    pub(crate) fn new(routes: UrlRoutes, config: Arc<dyn Repository>) -> Self {
        Self {
            routes: Arc::new(routes),
            owner: std::sync::Weak::new(),
            config,
            uri: None,
            base_url: String::new(),
            previous: None,
            defaults: HashMap::new(),
            force_scheme: None,
            force_root: None,
            asset_origin: None,
            host_formatter: None,
            path_formatter: None,
        }
    }

    pub(crate) fn with_owner(
        mut self,
        owner: std::sync::Weak<crate::container::Container>,
    ) -> Self {
        self.owner = owner;
        self
    }

    pub fn force_scheme(&mut self, scheme: Option<&str>) {
        self.force_scheme = scheme.map(str::to_owned);
    }
    pub fn force_https(&mut self, force: bool) {
        self.force_scheme(force.then_some("https"));
    }
    pub fn force_root_url(&mut self, root: Option<&str>) {
        self.force_root = root.map(|root| root.trim_end_matches('/').to_owned());
    }
    /// 设置路由 URL 的来源；None 恢复请求或配置中的来源。
    pub fn use_origin(&mut self, root: Option<&str>) {
        self.force_root_url(root.filter(|value| !value.is_empty()));
    }

    /// 设置资源来源；None 显式取消 CDN 来源，恢复普通 URL 来源。
    pub fn use_asset_origin(&mut self, root: Option<&str>) {
        self.asset_origin = Some(
            root.filter(|value| !value.is_empty())
                .map(|value| value.trim_end_matches('/').to_owned()),
        );
    }

    pub fn get_default_parameters(&self) -> &HashMap<String, String> {
        &self.defaults
    }

    pub fn secure_asset(&self, path: &str) -> UrlResult<String> {
        self.asset(path, Some(true))
    }

    pub fn asset_from(&self, root: &str, path: &str, secure: Option<bool>) -> UrlResult<String> {
        if path.contains('\\') || path.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(UrlError::InvalidParameter);
        }
        let mut root = valid_http_url(root)?;
        root.set_scheme(self.root(secure)?.scheme())
            .map_err(|()| UrlError::InvalidOrigin)?;
        root.set_query(None);
        root.set_fragment(None);
        Ok(format!(
            "{}/{}",
            root.as_str()
                .trim_end_matches('/')
                .replace("/index.php", ""),
            path.trim_matches('/')
        ))
    }

    pub fn format_host_using(
        &mut self,
        formatter: impl Fn(&str, Option<&RouteUrlGenerator>) -> String + Send + Sync + 'static,
    ) {
        self.host_formatter = Some(Arc::new(formatter));
    }
    pub fn format_path_using(
        &mut self,
        formatter: impl Fn(&str, Option<&RouteUrlGenerator>) -> String + Send + Sync + 'static,
    ) {
        self.path_formatter = Some(Arc::new(formatter));
    }
    pub(super) fn format(
        &self,
        root: &str,
        path: &str,
        route: Option<&RouteUrlGenerator>,
    ) -> String {
        let path = format!("/{}", path.trim_matches('/'));
        let root = self
            .host_formatter
            .as_ref()
            .map_or_else(|| root.to_owned(), |format| format(root, route));
        let path = self
            .path_formatter
            .as_ref()
            .map_or(path.clone(), |format| format(&path, route));
        format!(
            "{}/{}",
            root.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
        .trim_end_matches('/')
        .to_owned()
    }

    pub(crate) fn with_uri(&self, uri: Option<Uri>) -> Self {
        Self {
            uri,
            ..self.clone()
        }
    }

    pub(crate) fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Returns a generator with additional defaults, without mutating the application service.
    pub fn with_defaults(mut self, defaults: &[(&str, &str)]) -> Self {
        self.defaults.extend(
            defaults
                .iter()
                .map(|(name, value)| ((*name).into(), (*value).into())),
        );
        self
    }

    pub(crate) fn merge_defaults(&mut self, attributes: &crate::http::RequestAttributes) {
        if let Some(defaults) = attributes.get(URL_DEFAULTS)
            && let Some(defaults) = defaults.downcast_ref::<HashMap<String, String>>()
        {
            self.defaults.extend(defaults.clone());
        }
    }

    pub(crate) fn set_defaults(request: &mut Request, values: &[(&str, &str)]) {
        let attributes = request.attributes();
        let mut defaults = attributes
            .get(URL_DEFAULTS)
            .and_then(|value| value.downcast_ref::<HashMap<String, String>>().cloned())
            .unwrap_or_default();
        defaults.extend(
            values
                .iter()
                .map(|(name, value)| ((*name).into(), (*value).into())),
        );
        attributes.insert(
            URL_DEFAULTS,
            crate::container::ParameterValue::new(defaults),
        );
    }

    pub fn for_request(&self, request: &Request) -> Self {
        let mut urls = self
            .with_uri(Some(request_uri(request)))
            .with_base_url(request.get_base_url().to_owned());
        urls.merge_defaults(request.attributes());
        let same_origin = |target: &&str| {
            let target = *target;
            target.starts_with('/') && !target.starts_with("//") && !target.contains('\\')
                || valid_http_url(target)
                    .ok()
                    .zip(urls.root(None).ok())
                    .is_some_and(|(target, root)| target.origin() == root.origin())
        };
        urls.previous = request
            .headers()
            .get(::http::header::REFERER)
            .and_then(|value| value.to_str().ok())
            .filter(same_origin)
            .or_else(|| request.session().ok()?.previous_url().filter(same_origin))
            .map(str::to_owned);
        urls
    }

    fn string_config(&self, name: &str) -> Option<String> {
        match self.config.get(name) {
            Some(Value::String(value)) if !value.is_empty() => Some(value),
            _ => None,
        }
    }

    fn request_root(&self) -> Option<UrlResult<::url::Url>> {
        let uri = self.uri.as_ref()?;
        Some(valid_http_url(&format!(
            "{}://{}{}",
            uri.scheme_str()?,
            uri.authority()?,
            self.base_url
        )))
    }

    fn configured_root(&self) -> UrlResult<::url::Url> {
        valid_http_url(
            &self
                .string_config("app.url")
                .ok_or(UrlError::InvalidOrigin)?,
        )
    }

    fn root(&self, secure: Option<bool>) -> UrlResult<::url::Url> {
        let mut root = if let Some(root) = &self.force_root {
            valid_http_url(root)?
        } else if self.config.get("app.url_explicit") == Some(Value::Bool(true)) {
            self.configured_root()?
        } else {
            self.request_root()
                .unwrap_or_else(|| self.configured_root())?
        };
        if let Some(scheme) = &self.force_scheme {
            root.set_scheme(scheme)
                .map_err(|()| UrlError::InvalidOrigin)?;
        }
        if let Some(secure) = secure {
            root.set_scheme(if secure { "https" } else { "http" })
                .map_err(|()| UrlError::InvalidOrigin)?;
        }
        root.set_query(None);
        root.set_fragment(None);
        Ok(root)
    }

    fn signing_key(&self) -> UrlResult<String> {
        self.string_config("app.key")
            .ok_or(UrlError::MissingSigningKey)
    }

    fn previous_keys(&self) -> Vec<Value> {
        match self.config.get("app.previous_keys") {
            Some(Value::Array(keys)) => keys,
            _ => Vec::new(),
        }
    }

    pub(crate) fn valid_signature_at(
        &self,
        request: &Request,
        mode: SignatureMode,
        ignore: SignatureQueryFilter<'_>,
        now: SystemTime,
    ) -> UrlResult<bool> {
        let query = request.uri().query().unwrap_or_default();
        let mut supplied = None;
        let mut expires = None;
        for (key, value) in ::url::form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "signature" => supplied = Some(value),
                "expires" => expires = Some(value),
                name if name.starts_with("signature[") || name.starts_with("expires[") => {
                    return Ok(false);
                }
                _ => {}
            }
        }
        let Some(supplied) = supplied else {
            return Ok(false);
        };
        let mut target = if mode == SignatureMode::Absolute {
            let urls = self.with_uri(Some(request_uri(request)));
            let root = urls
                .request_root()
                .unwrap_or_else(|| urls.configured_root())?;
            let path = request.uri().path().trim_end_matches('/');
            format!(
                "{}{base}{path}",
                root.origin().ascii_serialization(),
                base = request.get_base_url()
            )
        } else {
            format!("/{}", request.uri().path().trim_matches('/'))
        };
        let raw = query
            .split('&')
            .filter(|part| {
                let name = part.split_once('=').map_or(*part, |(name, _)| name);
                name != "signature"
                    && (name == "expires"
                        || !match ignore {
                            SignatureQueryFilter::Keys(keys) => keys.contains(&name),
                            SignatureQueryFilter::Predicate(ignored) => ignored(name),
                        })
            })
            .collect::<Vec<_>>()
            .join("&");
        if !raw.is_empty() {
            target.push('?');
            target.push_str(&raw);
        }
        let key = self.signing_key()?;
        let mut valid = signature(&target, &key)
            .as_bytes()
            .ct_eq(supplied.as_bytes());
        for old in self.previous_keys() {
            if let Value::String(old) = old
                && !old.is_empty()
            {
                valid |= signature(&target, &old)
                    .as_bytes()
                    .ct_eq(supplied.as_bytes());
            }
        }
        if !bool::from(valid) {
            return Ok(false);
        }
        let now = unix_seconds(now)?;
        Ok(expires.is_none_or(|expires| {
            expires.is_empty() || expires == "0" || !is_expired(&expires, now)
        }))
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "PHP accepts numeric fractional expiry values"
)]
fn is_expired(expires: &str, now: u64) -> bool {
    expires
        .trim()
        .parse::<f64>()
        .is_ok_and(|seconds| (now as f64) > seconds)
}

impl Contract for UrlGenerator {
    fn full(&self) -> UrlResult<String> {
        let root = self.root(None)?;
        let path = self
            .uri
            .as_ref()
            .map_or("", Uri::path)
            .trim_end_matches('/');
        let mut output = format!("{}{path}", root.as_str().trim_end_matches('/'));
        let query = Request::normalize_query_string(
            self.uri.as_ref().and_then(Uri::query).unwrap_or_default(),
        )
        .map_err(|_| UrlError::InvalidParameter)?;
        if !query.is_empty() {
            if path.is_empty() && root.path() == "/" {
                output.push('/');
            }
            output.push('?');
            output.push_str(&query);
        }
        Ok(output)
    }

    fn previous_path(&self, fallback: Option<&str>) -> UrlResult<String> {
        let previous = self.previous(fallback)?;
        let uri = valid_http_url(&previous)?;
        let root = self.root(None)?;
        let base = root.path().trim_end_matches('/');
        let path = uri.path();
        let path = path
            .strip_prefix(base)
            .filter(|suffix| suffix.is_empty() || suffix.starts_with('/'))
            .unwrap_or(path)
            .trim_end_matches('/');
        Ok(if path.is_empty() { "/" } else { path }.into())
    }

    fn current(&self) -> UrlResult<String> {
        self.to(self.uri.as_ref().map_or("/", Uri::path), &[], None)
    }

    fn previous(&self, fallback: Option<&str>) -> UrlResult<String> {
        self.to(
            self.previous.as_deref().or(fallback).unwrap_or("/"),
            &[],
            None,
        )
    }

    fn to(
        &self,
        path: &str,
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String> {
        if valid_http_url(path).is_ok() || path.starts_with('#') {
            return Ok(path.to_owned());
        }
        if path.starts_with("//")
            || path.contains('\\')
            || path.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(UrlError::InvalidParameter);
        }
        let root = self.root(secure)?;
        let mut output = root.as_str().trim_end_matches('/').to_owned();
        let (path, query) = path
            .split_once('?')
            .map_or((path, None), |(path, query)| (path, Some(query)));
        let path = path.trim_matches('/');
        if !path.is_empty() {
            output.push('/');
            output.push_str(path);
        }
        for parameter in parameters {
            let value = match parameter {
                RouteParameter::Named(_, value) | RouteParameter::Positional(value) => value,
            };
            if let Some(value) = value.scalar()?.filter(|value| !value.is_empty()) {
                output.push('/');
                output.push_str(&encode(&value));
            }
        }
        let root_text = root.as_str().trim_end_matches('/');
        let path = output.strip_prefix(root_text).unwrap_or(&output);
        output = self.format(root_text, path, None);
        if let Some(query) = query {
            output.push('?');
            output.push_str(query);
        }
        Ok(output)
    }

    fn route(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        absolute: bool,
    ) -> UrlResult<String> {
        let route = self.routes.named.get(name).ok_or(UrlError::MissingRoute)?;
        route.to(
            &self.root(None)?,
            parameters,
            absolute,
            &self.defaults,
            Some(self),
        )
    }

    fn action(
        &self,
        action: &str,
        parameters: &[RouteParameter],
        absolute: bool,
    ) -> UrlResult<String> {
        let route = self
            .routes
            .actions
            .get(action.trim_matches('\\'))
            .ok_or(UrlError::MissingAction)?;
        route.to(
            &self.root(None)?,
            parameters,
            absolute,
            &self.defaults,
            Some(self),
        )
    }

    fn query(
        &self,
        path: &str,
        query: &[RouteParameter],
        parameters: &[RouteParameter],
        secure: Option<bool>,
    ) -> UrlResult<String> {
        let (path, fragment) = path
            .split_once('#')
            .map_or((path, ""), |(path, fragment)| (path, fragment));
        let (path, existing) = path.split_once('?').unwrap_or((path, ""));
        let url = self.to(path, parameters, secure)?;
        let mut merged = json_parameters(
            crate::http::parse_query(existing).map_err(|_| UrlError::InvalidParameter)?,
        );
        for item in query {
            match item.name() {
                Some(name)
                    if name
                        .parse::<i64>()
                        .is_ok_and(|number| number.to_string() == name) =>
                {
                    if let RouteParameter::Named(_, value) = item {
                        merged.push(RouteParameter::Positional(value.clone()));
                    }
                }
                Some(name) => {
                    if let Some(value) = merged.iter_mut().find(|entry| entry.name() == Some(name))
                    {
                        *value = item.clone();
                    } else {
                        merged.push(item.clone());
                    }
                }
                None => merged.push(item.clone()),
            }
        }
        let path = url.as_str();
        let mut output = path.to_owned();
        let query = query_string(&merged, false)?;
        if !query.is_empty() {
            output.push('?');
            output.push_str(&query);
        }
        if !fragment.is_empty() {
            output.push('#');
            output.push_str(fragment);
        }
        Ok(output)
    }

    fn asset(&self, path: &str, secure: Option<bool>) -> UrlResult<String> {
        if valid_http_url(path).is_ok() {
            return Ok(path.to_owned());
        }
        if path.starts_with("//")
            || path.contains('\\')
            || path.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(UrlError::InvalidParameter);
        }
        let root = match self
            .asset_origin
            .clone()
            .unwrap_or_else(|| self.string_config("app.asset_url"))
        {
            Some(asset) => valid_http_url(&asset)?,
            None => self.root(secure)?,
        };
        Ok(format!(
            "{}/{}",
            root.as_str()
                .trim_end_matches('/')
                .replace("/index.php", ""),
            path.trim_matches('/')
        ))
    }

    fn signed_route(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        mode: SignatureMode,
    ) -> UrlResult<String> {
        if parameters
            .iter()
            .any(|item| matches!(item.name(), Some("expires" | "signature")))
        {
            return Err(UrlError::ReservedSignatureParameter);
        }
        let mut parameters = parameters.to_vec();
        if let Some(expiration) = expiration {
            parameters.push(RouteParameter::named(
                "expires",
                unix_seconds(expiration)?.to_string(),
            ));
        }
        parameters.sort_by(|left, right| left.name().cmp(&right.name()));
        let key = self.signing_key()?;
        let absolute = mode == SignatureMode::Absolute;
        let mut target = self.route(name, &parameters, absolute)?;
        let signature = signature(&target, &key);
        target.push_str(if target.contains('?') {
            "&signature="
        } else {
            "?signature="
        });
        target.push_str(&signature);
        Ok(target)
    }

    fn temporary_signed_route(
        &self,
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        mode: SignatureMode,
    ) -> UrlResult<String> {
        self.signed_route(name, parameters, Some(expiration), mode)
    }

    fn has_valid_signature(
        &self,
        request: &Request,
        mode: SignatureMode,
        ignore_query: SignatureQueryFilter<'_>,
    ) -> UrlResult<bool> {
        self.valid_signature_at(
            request,
            mode,
            ignore_query,
            crate::support::utc_now_for(self.owner.upgrade().as_deref()).into(),
        )
    }
}

fn request_uri(request: &Request) -> Uri {
    url_uri(request.uri(), request.headers().get(::http::header::HOST))
}

pub(crate) fn url_uri(uri: &Uri, host: Option<&::http::HeaderValue>) -> Uri {
    if uri.scheme().is_some() && uri.authority().is_some() {
        return uri.clone();
    }
    host.and_then(|header| {
        let host = header.to_str().ok()?;
        let origin = valid_http_url(&format!("http://{host}")).ok()?;
        if origin.path() != "/" || origin.query().is_some() || origin.fragment().is_some() {
            return None;
        }
        Uri::builder()
            .scheme("http")
            .authority(host)
            .path_and_query(uri.path_and_query()?.clone())
            .build()
            .ok()
    })
    .unwrap_or_else(|| uri.clone())
}

pub(crate) fn valid_http_url(value: &str) -> UrlResult<::url::Url> {
    if value.contains('\\') || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(UrlError::InvalidOrigin);
    }
    let parsed = ::url::Url::parse(value).map_err(|_| UrlError::InvalidOrigin)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(UrlError::InvalidOrigin);
    }
    Ok(parsed)
}

fn unix_seconds(time: SystemTime) -> UrlResult<u64> {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| UrlError::InvalidParameter)
}

fn signature(target: &str, key: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let key = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
    let digest = hmac::sign(&key, target.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest.as_ref() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

const NON_UNRESERVED: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub(super) fn encode(text: &str) -> String {
    utf8_percent_encode(text, NON_UNRESERVED).to_string()
}

pub(super) fn query_string(parameters: &[RouteParameter], route: bool) -> UrlResult<String> {
    let mut parts = Vec::new();
    let mut index = 0;
    for parameter in parameters {
        match parameter {
            RouteParameter::Named(key, value) => append_query(&mut parts, key, value)?,
            RouteParameter::Positional(value) if route => {
                if let Some(value) = value.scalar()? {
                    parts.push(encode(&value));
                }
            }
            RouteParameter::Positional(value) => {
                append_query(&mut parts, &index.to_string(), value)?;
                index += 1;
            }
        }
    }
    Ok(parts.join("&"))
}

pub(crate) fn json_parameters(value: Value) -> RouteParameters<'static> {
    match value {
        Value::Object(values) => values
            .into_iter()
            .map(|(name, value)| {
                let value = query_value(value);
                if name
                    .parse::<i64>()
                    .is_ok_and(|number| number.to_string() == name)
                {
                    RouteParameter::Positional(value)
                } else {
                    RouteParameter::Named(name, value)
                }
            })
            .collect(),
        Value::Array(values) => values
            .into_iter()
            .map(|value| RouteParameter::Positional(query_value(value)))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn encode_query_value(value: Value) -> UrlResult<String> {
    let UrlValue::Array(parameters) = query_value(value) else {
        return Err(UrlError::InvalidParameter);
    };
    query_string(&parameters, false)
}

fn query_value(value: Value) -> UrlValue<'static> {
    match value {
        Value::String(value) => UrlValue::String(value),
        Value::Bool(value) => UrlValue::Bool(value),
        Value::Number(value) => UrlValue::Number(value),
        Value::Array(_) => UrlValue::Array(json_parameters(value)),
        Value::Object(values) => UrlValue::Array(
            values
                .into_iter()
                .map(|(name, value)| RouteParameter::Named(name, query_value(value)))
                .collect(),
        ),
        _ => UrlValue::Null,
    }
}

fn append_query(parts: &mut Vec<String>, key: &str, value: &UrlValue<'_>) -> UrlResult<()> {
    match value {
        UrlValue::Array(children) => {
            let mut index = 0;
            for child in children {
                match child {
                    RouteParameter::Named(name, value) => {
                        append_query(parts, &format!("{key}[{name}]"), value)?;
                    }
                    RouteParameter::Positional(value) => {
                        append_query(parts, &format!("{key}[{index}]"), value)?;
                        index += 1;
                    }
                }
            }
        }
        UrlValue::Null => {}
        value => {
            if let Some(value) = value.scalar()? {
                parts.push(format!("{}={}", encode(key), encode(&value)));
            }
        }
    }
    Ok(())
}
