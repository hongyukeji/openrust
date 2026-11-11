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
    contracts::{config::Repository, routing::UrlGenerator as Contract},
    http::{HttpError, Request, Uri},
};

use super::RouteUrlGenerator;

/// Generates named routes and signed URLs for the application.
#[derive(Clone)]
pub struct UrlGenerator {
    routes: Arc<HashMap<String, RouteUrlGenerator>>,
    config: Arc<dyn Repository>,
    uri: Option<Uri>,
    previous: Option<String>,
}

/// Route parameters in application-supplied order.
pub type RouteParameters = Vec<RouteParameter>;

/// A positional or named route or query parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteParameter {
    Positional(UrlValue),
    Named(String, UrlValue),
}

/// The scalar and nested values accepted by the URL encoder.
#[derive(Clone, Debug, PartialEq)]
pub enum UrlValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(RouteParameters),
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

pub type UrlResult<T> = Result<T, UrlError>;

/// A failure to construct a URL, signature, or redirect.
#[derive(Debug)]
#[non_exhaustive]
pub enum UrlError {
    MissingRoute,
    MissingParameter,
    InvalidParameter,
    InvalidOrigin,
    MissingSigningKey,
    ReservedSignatureParameter,
    InvalidRedirect,
    Unavailable,
    Session(crate::session::SessionError),
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingRoute => "Named route is not defined.",
            Self::MissingParameter => "Required route parameter is missing.",
            Self::InvalidParameter => "Invalid URL parameter.",
            Self::InvalidOrigin => "Invalid HTTP URL origin.",
            Self::MissingSigningKey => "URL signing key is missing.",
            Self::ReservedSignatureParameter => "URL parameter is reserved for signing.",
            Self::InvalidRedirect => "Invalid redirect target or status.",
            Self::Unavailable => "URL service is unavailable.",
            Self::Session(error) => return error.fmt(f),
        })
    }
}

impl std::error::Error for UrlError {}

impl From<UrlError> for HttpError {
    fn from(source: UrlError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<&str> for UrlValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for UrlValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl UrlValue {
    pub(super) fn scalar(&self) -> Option<String> {
        match self {
            Self::Null | Self::Array(_) => None,
            Self::Bool(value) => Some(u8::from(*value).to_string()),
            Self::Number(value) => Some(value.to_string()),
            Self::String(value) => Some(value.clone()),
        }
    }

    pub(super) fn empty(&self) -> bool {
        matches!(self, Self::Null) || matches!(self, Self::String(value) if value.is_empty())
    }
}

impl RouteParameter {
    pub fn named(name: impl Into<String>, value: impl Into<UrlValue>) -> Self {
        Self::Named(name.into(), value.into())
    }

    pub fn positional(value: impl Into<UrlValue>) -> Self {
        Self::Positional(value.into())
    }

    pub fn model(
        name: impl Into<String>,
        model: &impl crate::contracts::routing::UrlRoutable,
    ) -> UrlResult<Self> {
        let key = model.get_route_key().ok_or(UrlError::MissingParameter)?;
        Ok(Self::named(name, key))
    }

    fn name(&self) -> Option<&str> {
        match self {
            Self::Named(name, _) => Some(name),
            Self::Positional(_) => None,
        }
    }
}

impl UrlGenerator {
    pub(crate) fn new(
        routes: HashMap<String, RouteUrlGenerator>,
        config: Arc<dyn Repository>,
    ) -> Self {
        Self {
            routes: Arc::new(routes),
            config,
            uri: None,
            previous: None,
        }
    }

    pub(crate) fn with_uri(&self, uri: Option<Uri>) -> Self {
        Self {
            uri,
            ..self.clone()
        }
    }

    #[allow(
        non_snake_case,
        reason = "Matches Laravel's request-scoped URL generator"
    )]
    pub fn forRequest(&self, request: &Request) -> Self {
        let mut urls = self.with_uri(Some(request_uri(request)));
        urls.previous = request
            .headers()
            .get(::http::header::REFERER)
            .and_then(|value| value.to_str().ok())
            .filter(|target| {
                target.starts_with('/') && !target.starts_with("//") && !target.contains('\\')
                    || valid_http_url(target)
                        .ok()
                        .zip(urls.root(None).ok())
                        .is_some_and(|(referer, root)| referer.origin() == root.origin())
            })
            .or_else(|| request.session().ok()?.previousUrl())
            .map(str::to_owned);
        urls
    }

    fn string_config(&self, name: &str) -> Option<String> {
        match self.config.get(name) {
            Some(Value::String(value)) if !value.is_empty() => Some(value),
            _ => None,
        }
    }

    fn root(&self, secure: Option<bool>) -> UrlResult<::url::Url> {
        let mut root = match self
            .uri
            .as_ref()
            .and_then(|uri| Some(format!("{}://{}", uri.scheme_str()?, uri.authority()?)))
        {
            Some(origin) => valid_http_url(&origin)?,
            None => valid_http_url(
                &self
                    .string_config("app.url")
                    .ok_or(UrlError::InvalidOrigin)?,
            )?,
        };
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

    fn valid_signature_at(
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
            let root = self.with_uri(Some(request_uri(request))).root(None)?;
            let path = request.uri().path().trim_end_matches('/');
            format!("{}{path}", root.origin().ascii_serialization())
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

#[allow(non_snake_case, reason = "Matches Laravel's URL generator")]
impl Contract for UrlGenerator {
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
        for segment in std::iter::once(path.trim_matches('/').to_owned())
            .chain(parameters.iter().map(|parameter| {
                let value = match parameter {
                    RouteParameter::Named(_, value) | RouteParameter::Positional(value) => value,
                };
                value
                    .scalar()
                    .map_or_else(String::new, |value| encode(&value))
            }))
            .filter(|segment| !segment.is_empty())
        {
            output.push('/');
            output.push_str(&segment);
        }
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
        let route = self.routes.get(name).ok_or(UrlError::MissingRoute)?;
        route.to(&self.root(None)?, parameters, absolute)
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
        let query = query_string(&merged, false);
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
        let root = match self.string_config("app.asset_url") {
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

    fn signedRoute(
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
        let target = self.route(name, &parameters, absolute)?;
        parameters.push(RouteParameter::named("signature", signature(&target, &key)));
        self.route(name, &parameters, absolute)
    }

    fn temporarySignedRoute(
        &self,
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        mode: SignatureMode,
    ) -> UrlResult<String> {
        self.signedRoute(name, parameters, Some(expiration), mode)
    }

    fn hasValidSignature(
        &self,
        request: &Request,
        mode: SignatureMode,
        ignore_query: SignatureQueryFilter<'_>,
    ) -> UrlResult<bool> {
        self.valid_signature_at(request, mode, ignore_query, SystemTime::now())
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

pub(super) fn valid_http_url(value: &str) -> UrlResult<::url::Url> {
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

pub(super) fn query_string(parameters: &[RouteParameter], route: bool) -> String {
    let mut parts = Vec::new();
    let mut index = 0;
    for parameter in parameters {
        match parameter {
            RouteParameter::Named(key, value) => append_query(&mut parts, key, value),
            RouteParameter::Positional(value) if route => {
                if let Some(value) = value.scalar() {
                    parts.push(encode(&value));
                }
            }
            RouteParameter::Positional(value) => {
                append_query(&mut parts, &index.to_string(), value);
                index += 1;
            }
        }
    }
    parts.join("&")
}

fn json_parameters(value: Value) -> RouteParameters {
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

fn query_value(value: Value) -> UrlValue {
    match value {
        Value::String(value) => UrlValue::String(value),
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

fn append_query(parts: &mut Vec<String>, key: &str, value: &UrlValue) {
    match value {
        UrlValue::Array(children) => {
            let mut index = 0;
            for child in children {
                match child {
                    RouteParameter::Named(name, value) => {
                        append_query(parts, &format!("{key}[{name}]"), value);
                    }
                    RouteParameter::Positional(value) => {
                        append_query(parts, &format!("{key}[{index}]"), value);
                        index += 1;
                    }
                }
            }
        }
        UrlValue::Null => {}
        value => {
            if let Some(value) = value.scalar() {
                parts.push(format!("{}={}", encode(key), encode(&value)));
            }
        }
    }
}
