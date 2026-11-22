use std::collections::BTreeMap;

use http::header::{
    ALLOW, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LANGUAGE, CONTENT_LENGTH, CONTENT_TYPE, ETAG,
    EXPIRES, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED,
};
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use http_body::Body as _;

use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    support::{calendar, facades::Config},
};

type Directives = BTreeMap<String, Option<String>>;

/// Applies the route's HTTP cache directives and checks conditional requests.
pub struct SetCacheHeaders {
    directives: Directives,
    public: Option<bool>,
    etag: Option<Etag>,
    last_modified: Option<String>,
}

enum Etag {
    Auto,
    Explicit(HeaderValue),
}

impl SetCacheHeaders {
    pub fn using(options: &str) -> String {
        if options.is_empty() {
            "cache.headers".into()
        } else {
            format!("cache.headers:{options}")
        }
    }

    pub(crate) fn parse(options: &str) -> Option<Self> {
        let mut options: BTreeMap<_, _> = options
            .trim_end_matches(';')
            .split(';')
            .filter(|option| !option.is_empty())
            .map(|option| match option.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (option, None),
            })
            .collect();
        let mut settings = Self {
            directives: BTreeMap::new(),
            public: options.remove("public").map(enabled),
            etag: match options.remove("etag") {
                Some(Some(value)) => Some(Etag::Explicit(quote_etag(value)?)),
                Some(None) => Some(Etag::Auto),
                None => None,
            },
            last_modified: options.remove("last_modified").flatten().map(str::to_owned),
        };
        if let Some(private) = options.remove("private") {
            settings.public = Some(!enabled(private));
        }
        for (name, value) in options {
            let value = match name {
                "max_age" | "s_maxage" | "stale_while_revalidate" | "stale_if_error" => {
                    Some(value.unwrap_or("1").trim().parse::<i64>().ok()?.to_string())
                }
                "must_revalidate" | "no_cache" | "no_store" | "no_transform"
                | "proxy_revalidate" | "immutable" => enabled(value).then(String::new),
                _ => return None,
            };
            settings.directives.insert(name.replace('_', "-"), value);
        }
        if settings.directives.contains_key("s-maxage") {
            settings.public.get_or_insert(true);
        }
        Some(settings)
    }

    async fn apply(
        &self,
        method: &Method,
        request_headers: &HeaderMap,
        mut response: Response,
    ) -> Result<Response, HttpError> {
        if !matches!(*method, Method::GET | Method::HEAD) || !response.status().is_success() {
            return Ok(response);
        }
        let length = if response.binary_file.is_some() {
            None
        } else {
            response.body.size_hint().exact()
        };
        if method != Method::HEAD && length == Some(0) {
            return Ok(response);
        }

        let mut tag = match &self.etag {
            Some(Etag::Explicit(value)) => Some(value.clone()),
            _ => None,
        };
        let needs_hash =
            matches!(self.etag, Some(Etag::Auto)) && !response.headers().contains_key(ETAG);
        if let Some(length) = length.and_then(|size| usize::try_from(size).ok())
            && ((needs_hash && length > 0) || (length == 1 && method != Method::HEAD))
        {
            let bytes = axum::body::to_bytes(response.take_body(), length)
                .await
                .map_err(internal)?;
            if needs_hash && !bytes.is_empty() && bytes.as_ref() != b"0" {
                let hash = xxhash_rust::xxh3::xxh3_128(&bytes);
                tag = HeaderValue::from_str(&format!("\"{hash:032x}\"")).ok();
            }
            let falsey = bytes.as_ref() == b"0";
            response.setContent(bytes);
            if falsey && method != Method::HEAD {
                return Ok(response);
            }
        }

        let headers = &mut response.parts_mut().headers;
        if let Some(tag) = tag {
            headers.insert(ETAG, tag);
        }
        if let Some(date) = &self.last_modified {
            headers.insert(LAST_MODIFIED, last_modified(date)?);
        }
        apply_cache_control(headers, &self.directives, self.public);
        if is_not_modified(request_headers, headers) {
            response.parts_mut().status = StatusCode::NOT_MODIFIED;
            response.setContent(axum::body::Bytes::new());
            response = response.withoutHeaders([
                ALLOW,
                CONTENT_ENCODING,
                CONTENT_LANGUAGE,
                CONTENT_LENGTH,
                HeaderName::from_static("content-md5"),
                CONTENT_TYPE,
                LAST_MODIFIED,
            ]);
        }
        Ok(response)
    }
}

pub(in crate::http) fn set_public_cache_control(headers: &mut HeaderMap) {
    apply_cache_control(headers, &Directives::new(), Some(true));
}

// Cookie 语言选择不允许被下游 public 指令重新开放给共享缓存。
pub(super) fn set_private_locale_cache_control(headers: &mut HeaderMap) {
    let existing = cache_directives(headers);
    if existing.contains_key("private")
        && existing.contains_key("no-store")
        && !existing.contains_key("public")
    {
        return;
    }
    let directives = BTreeMap::from([("no-store".into(), Some(String::new()))]);
    apply_cache_control(headers, &directives, Some(false));
}

fn cache_directives(headers: &HeaderMap) -> Directives {
    let mut directives = Directives::new();
    for value in headers.get_all(CACHE_CONTROL) {
        if let Ok(value) = value.to_str() {
            for directive in comma_list(value) {
                let (name, value) = match directive.split_once('=') {
                    Some((name, value)) => (name, Some(value.trim().to_owned())),
                    None => (directive, Some(String::new())),
                };
                directives.insert(name.trim().to_ascii_lowercase(), value);
            }
        }
    }
    directives
}

fn apply_cache_control(headers: &mut HeaderMap, configured: &Directives, public: Option<bool>) {
    let mut directives = cache_directives(headers);
    directives.extend(configured.clone());
    if let Some(public) = public {
        let (include, remove) = if public {
            ("public", "private")
        } else {
            ("private", "public")
        };
        directives.remove(remove);
        directives.insert(include.into(), Some(String::new()));
    }
    let mut policy = directives
        .iter()
        .filter_map(|(key, value)| {
            value.as_ref().map(|value| {
                if value.is_empty() {
                    key.clone()
                } else {
                    format!("{key}={value}")
                }
            })
        })
        .collect::<Vec<_>>()
        .join(", ");
    if policy.is_empty() {
        policy = if headers.contains_key(LAST_MODIFIED) || headers.contains_key(EXPIRES) {
            "private, must-revalidate".into()
        } else {
            "no-cache, private".into()
        };
    } else if !["public", "private", "s-maxage"]
        .iter()
        .any(|key| directives.get(*key).is_some_and(Option::is_some))
    {
        policy.push_str(", private");
    }
    if let Ok(value) = HeaderValue::from_str(&policy) {
        headers.insert(CACHE_CONTROL, value);
    }
}

impl Middleware for SetCacheHeaders {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let response = next.run(request).await;
            let mut response = self
                .apply(request.method(), request.headers(), response)
                .await?;
            if request
                .extensions()
                .get::<super::set_locale::PrivateLocaleResponse>()
                .is_some()
            {
                set_private_locale_cache_control(&mut response.parts_mut().headers);
            }
            Ok(response)
        })
    }
}

fn enabled(value: Option<&str>) -> bool {
    !matches!(value, Some("" | "0"))
}

fn quote_etag(value: &str) -> Option<HeaderValue> {
    if value.starts_with('"') || value.starts_with("W/\"") {
        HeaderValue::from_str(value).ok()
    } else {
        HeaderValue::from_str(&format!("\"{value}\"")).ok()
    }
}

fn last_modified(input: &str) -> Result<HeaderValue, HttpError> {
    let configured = Config::get("app.timezone")?;
    let timezone = configured
        .as_ref()
        .and_then(|value| value.as_str())
        .unwrap_or("UTC");
    let input = if calendar::is_numeric(input) {
        format!("@{input}")
    } else {
        input.to_owned()
    };
    let date = calendar::parse_input_date(
        input.into(),
        None,
        timezone,
        calendar::DateReference::Wall(jiff::Timestamp::now()),
    )
    .map_err(|error| {
        internal(std::io::Error::other(format!(
            "invalid cache last_modified: {error:?}"
        )))
    })?;
    let date = jiff::Timestamp::from_second(date.timestamp())
        .ok()
        .map(|date| date.to_zoned(jiff::tz::TimeZone::UTC))
        .filter(|date| (1..=9999).contains(&date.year()))
        .ok_or_else(|| internal(std::io::Error::other("cache last_modified is out of range")))?;
    HeaderValue::from_str(&date.strftime("%a, %d %b %Y %H:%M:%S GMT").to_string())
        .map_err(Into::into)
}

fn is_not_modified(request: &HeaderMap, response: &HeaderMap) -> bool {
    fn weak_value(value: &str) -> &str {
        value.strip_prefix("W/").unwrap_or(value)
    }
    let tags = request
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(comma_list);
    if let Some(tag) = response.get(ETAG).and_then(|tag| tag.to_str().ok()) {
        let mut tags = tags.peekable();
        if tags.peek().is_some() {
            return tags.any(|value| value == "*" || weak_value(value) == weak_value(tag));
        }
    }
    let timestamp = |headers: &HeaderMap, header: HeaderName| {
        let date = headers.get(header)?.to_str().ok()?;
        httpdate::parse_http_date(date)
            .ok()
            .and_then(|time| jiff::Timestamp::try_from(time).ok())
            .map(crate::support::timestamp_seconds)
            .or_else(|| {
                jiff::fmt::rfc2822::DateTimeParser::new()
                    .parse_timestamp(date)
                    .ok()
                    .map(crate::support::timestamp_seconds)
            })
    };
    timestamp(request, IF_MODIFIED_SINCE)
        .zip(timestamp(response, LAST_MODIFIED))
        .is_some_and(|(since, modified)| since >= modified)
}

fn comma_list(value: &str) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    let mut escaped = false;
    value
        .split(move |character| {
            if escaped {
                escaped = false;
            } else if quoted && character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = !quoted;
            } else if !quoted && character == ',' {
                return true;
            }
            false
        })
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

fn internal(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}
