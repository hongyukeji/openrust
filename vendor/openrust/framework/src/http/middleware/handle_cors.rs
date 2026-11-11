use std::collections::BTreeMap;

use ::http::{HeaderName, HeaderValue, header::*};
use regex::RegexSet;
use serde::Deserialize;

use crate::{
    config::{ConfigError, Value},
    contracts::http::Middleware,
    http::{HeaderMap, HttpError, HttpFuture, Method, Next, Request, Response, StatusCode, Uri},
};

/// Answers CORS preflights and adds policy headers to matching HTTP responses.
pub struct HandleCors {
    paths: Paths,
    origins: Origins,
    methods: Allowed,
    headers: Allowed,
    exposed_headers: Option<HeaderValue>,
    max_age: Option<HeaderValue>,
    credentials: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Options {
    paths: Paths,
    allowed_origins: Vec<String>,
    allowed_origins_patterns: Vec<String>,
    allowed_methods: Vec<String>,
    allowed_headers: Vec<String>,
    exposed_headers: Vec<String>,
    max_age: Option<u64>,
    supports_credentials: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Paths {
    List(Vec<String>),
    ByHost(BTreeMap<String, HostPath>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum HostPath {
    Default(String),
    Host(Vec<String>),
}

impl Default for Paths {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

impl Paths {
    fn matches(&self, request: &Request) -> bool {
        let authority = request
            .uri()
            .authority()
            .cloned()
            .or_else(|| request.headers().get(HOST)?.to_str().ok()?.parse().ok());
        let matches = |pattern: &String| {
            let pattern = if pattern == "/" {
                pattern
            } else {
                pattern.trim_matches('/')
            };
            request.is(pattern)
                || authority.as_ref().is_some_and(|authority| {
                    let scheme = request.uri().scheme_str().unwrap_or("http");
                    let url = format!(
                        "{scheme}://{authority}{}",
                        request
                            .uri()
                            .path_and_query()
                            .map_or("/", ::http::uri::PathAndQuery::as_str)
                    );
                    wildcard_matches(pattern, &url)
                })
        };
        match self {
            Self::List(paths) => paths.iter().any(matches),
            Self::ByHost(paths) => {
                let by_host = authority
                    .as_ref()
                    .and_then(|authority| paths.get(&authority.host().to_ascii_lowercase()));
                if let Some(HostPath::Host(paths)) = by_host {
                    return paths.iter().any(matches);
                }
                paths.values().any(|entry| match entry {
                    HostPath::Default(path) => matches(path),
                    HostPath::Host(_) => false,
                })
            }
        }
    }
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let Some((prefix, rest)) = pattern.split_once('*') else {
        return pattern == value;
    };
    let Some((middle, suffix)) = rest.rsplit_once('*') else {
        return value
            .strip_prefix(prefix)
            .is_some_and(|value| value.ends_with(rest));
    };
    value
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .is_some_and(|value| {
            middle
                .split('*')
                .try_fold(value, |value, fragment| {
                    value.split_once(fragment).map(|(_, tail)| tail)
                })
                .is_some()
        })
}

enum Origins {
    Fixed(HeaderValue),
    Reflect {
        any: bool,
        exact: Vec<HeaderValue>,
        patterns: RegexSet,
    },
}

enum Allowed {
    Reflect,
    Fixed(HeaderValue),
}

impl HandleCors {
    pub(crate) fn from_config(value: Option<Value>) -> Result<Option<Self>, ConfigError> {
        let Some(value) = value else { return Ok(None) };
        let options: Options = serde_json::from_value(value)
            .map_err(|_| invalid("cors", "an object with valid CORS options"))?;
        if matches!(&options.paths, Paths::List(paths) if paths.is_empty())
            || matches!(&options.paths, Paths::ByHost(paths) if paths.is_empty())
        {
            return Ok(None);
        }
        let origins = Origins::new(
            options.allowed_origins,
            options.allowed_origins_patterns,
            options.supports_credentials,
        )?;
        let methods = Allowed::new(
            options
                .allowed_methods
                .into_iter()
                .map(|method| method.to_ascii_uppercase())
                .collect(),
            "cors.allowed_methods",
            |method| method.parse::<Method>().is_ok(),
        )?;
        let headers = Allowed::new(
            options
                .allowed_headers
                .into_iter()
                .map(|header| header.to_ascii_lowercase())
                .collect(),
            "cors.allowed_headers",
            |header| header.parse::<HeaderName>().is_ok(),
        )?;
        let exposed_headers = if options.exposed_headers.is_empty() {
            None
        } else {
            Some(header_list(
                &options.exposed_headers,
                "cors.exposed_headers",
                |header| header.parse::<HeaderName>().is_ok(),
            )?)
        };
        Ok(Some(Self {
            paths: options.paths,
            origins,
            methods,
            headers,
            exposed_headers,
            max_age: options.max_age.map(HeaderValue::from),
            credentials: options.supports_credentials,
        }))
    }

    fn apply(&self, request: &Request, response: &mut Response, preflight: bool) {
        let headers = &mut response.parts_mut().headers;
        if request.method() == Method::OPTIONS {
            vary(headers, ACCESS_CONTROL_REQUEST_METHOD);
        }
        if let Some(origin) = self.origins.allowed(request, headers) {
            headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        }
        if !headers.contains_key(ACCESS_CONTROL_ALLOW_ORIGIN) {
            return;
        }
        if self.credentials {
            headers.insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
        if preflight {
            self.methods.apply(
                headers,
                request.headers(),
                ACCESS_CONTROL_ALLOW_METHODS,
                ACCESS_CONTROL_REQUEST_METHOD,
            );
            self.headers.apply(
                headers,
                request.headers(),
                ACCESS_CONTROL_ALLOW_HEADERS,
                ACCESS_CONTROL_REQUEST_HEADERS,
            );
            if let Some(value) = &self.max_age {
                headers.insert(ACCESS_CONTROL_MAX_AGE, value.clone());
            }
        } else if let Some(value) = &self.exposed_headers {
            headers.insert(ACCESS_CONTROL_EXPOSE_HEADERS, value.clone());
        }
    }
}

impl Middleware for HandleCors {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !self.paths.matches(request) {
                return Ok(next.run(request).await);
            }
            let preflight = request.method() == Method::OPTIONS
                && request
                    .headers()
                    .contains_key(ACCESS_CONTROL_REQUEST_METHOD);
            let mut response = if preflight {
                Response::empty(StatusCode::NO_CONTENT)
            } else {
                next.run(request).await
            };
            self.apply(request, &mut response, preflight);
            Ok(response)
        })
    }
}

impl Origins {
    fn new(
        origins: Vec<String>,
        mut patterns: Vec<String>,
        credentials: bool,
    ) -> Result<Self, ConfigError> {
        let any = origins.iter().any(|origin| origin == "*");
        if any && !credentials {
            return Ok(Self::Fixed(HeaderValue::from_static("*")));
        }
        let mut exact = Vec::new();
        for origin in origins {
            if origin == "*" {
                continue;
            }
            if origin.contains('*') {
                let pattern = origin
                    .split('*')
                    .map(regex::escape)
                    .collect::<Vec<_>>()
                    .join(".*");
                patterns.push(format!(r"\A{pattern}\z"));
                continue;
            }
            if !valid_origin(&origin) {
                return Err(invalid(
                    "cors.allowed_origins",
                    "HTTP origins, null or wildcard patterns",
                ));
            }
            exact.push(
                origin
                    .parse()
                    .map_err(|_| invalid("cors.allowed_origins", "valid HTTP header values"))?,
            );
        }
        if !any && patterns.is_empty() && exact.len() == 1 {
            return Ok(Self::Fixed(exact.remove(0)));
        }
        let patterns = patterns.iter().map(|pattern| {
            // Laravel applications commonly use slash or hash delimiters for patterns.
            let Some(delimiter) = pattern.chars().next().filter(|c| matches!(c, '/' | '#')) else {
                return pattern.clone();
            };
            let Some((body, flags)) = pattern[1..].rsplit_once(delimiter) else {
                return pattern.clone();
            };
            let flags = flags.replace('u', "");
            if flags.is_empty() {
                body.to_owned()
            } else {
                format!("(?{flags}){body}")
            }
        });
        let patterns = RegexSet::new(patterns).map_err(|_| {
            invalid(
                "cors.allowed_origins_patterns",
                "supported regular expressions",
            )
        })?;
        Ok(Self::Reflect {
            any,
            exact,
            patterns,
        })
    }

    fn allowed(&self, request: &Request, headers: &mut HeaderMap) -> Option<HeaderValue> {
        match self {
            Self::Fixed(origin) => Some(origin.clone()),
            Self::Reflect {
                any,
                exact,
                patterns,
            } => {
                vary(headers, ORIGIN);
                let origin = request.headers().get(ORIGIN)?;
                let value = origin.to_str().ok()?;
                (valid_origin(value)
                    && (*any || exact.contains(origin) || patterns.is_match(value)))
                .then(|| origin.clone())
            }
        }
    }
}

impl Allowed {
    fn new(
        values: Vec<String>,
        key: &str,
        valid: impl Fn(&str) -> bool,
    ) -> Result<Self, ConfigError> {
        if values.iter().any(|value| value == "*") {
            return Ok(Self::Reflect);
        }
        header_list(&values, key, valid).map(Self::Fixed)
    }

    fn apply(
        &self,
        response: &mut HeaderMap,
        request: &HeaderMap,
        allow: HeaderName,
        requested: HeaderName,
    ) {
        let value = match self {
            Self::Fixed(value) => value.clone(),
            Self::Reflect => {
                let value = request
                    .get(&requested)
                    .map_or(HeaderValue::from_static(""), |value| {
                        if requested == ACCESS_CONTROL_REQUEST_METHOD {
                            let method = value.as_bytes().to_ascii_uppercase();
                            HeaderValue::from_bytes(&method)
                                .expect("uppercasing preserves header validity")
                        } else {
                            value.clone()
                        }
                    });
                vary(response, requested);
                value
            }
        };
        response.insert(allow, value);
    }
}

fn header_list(
    values: &[String],
    key: &str,
    valid: impl Fn(&str) -> bool,
) -> Result<HeaderValue, ConfigError> {
    if !values.iter().all(|value| valid(value)) {
        return Err(invalid(key, "a list of valid HTTP tokens"));
    }
    values
        .join(", ")
        .parse()
        .map_err(|_| invalid(key, "a valid HTTP header value"))
}

fn valid_origin(origin: &str) -> bool {
    origin == "null"
        || origin.parse::<Uri>().is_ok_and(|uri| {
            matches!(uri.scheme_str(), Some("http" | "https"))
                && uri.authority().is_some_and(|authority| {
                    let authority_text = authority.as_str();
                    let host = authority.host();
                    !host.is_empty()
                        && !authority_text.contains('@')
                        && (authority_text == host || authority.port_u16().is_some())
                        && origin
                            .split_once("://")
                            .is_some_and(|(_, rest)| rest == authority_text)
                })
        })
}

fn vary(headers: &mut HeaderMap, name: HeaderName) {
    if !headers
        .get_all(VARY)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| value.trim() == "*" || value.trim().eq_ignore_ascii_case(name.as_str()))
    {
        headers.append(
            VARY,
            HeaderValue::from_str(name.as_str()).expect("header names are valid values"),
        );
    }
}

fn invalid(key: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.to_owned(),
        expected,
    }
}
