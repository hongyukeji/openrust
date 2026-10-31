//! 有界请求输入、独立元信息与 Laravel 风格内容协商。

mod conditional;
mod deserialize;
mod form;
mod input;
mod normalize;
mod url;
mod url_query;
mod value;

pub(crate) fn deserialize_query<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> crate::Result<T> {
    deserialize::query(value)
}
pub use conditional::{InputOutcome, InputOutcomeMut, InputTruthy, Truthy};
pub use input::Request;
pub(crate) fn parse_query(
    raw: Option<&str>,
    policy: &InputNormalization,
) -> crate::Result<serde_json::Value> {
    input::parse_query(raw, policy)
}
pub(crate) fn policy(parts: &http::request::Parts) -> InputNormalization {
    input::policy(parts)
}
pub use normalize::InputNormalization;

pub(crate) fn validate_json_depth(raw: &[u8]) -> crate::Result<()> {
    input::json_depth(raw)
}
pub(crate) fn body_error(error: &axum::extract::rejection::BytesRejection) -> crate::Error {
    input::body_error(error)
}

use std::collections::BTreeMap;
use std::fmt;

use axum::extract::{FromRequestParts, OriginalUri};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header::HeaderName, request::Parts};

use crate::core::{Error, Result};
use crate::http::authority::authority;

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_HEADER_VALUES: usize = 128;
const MAX_ACCEPT_BYTES: usize = 16 * 1024;
const MAX_ACCEPT_ITEMS: usize = 128;

/// 请求的只读快照；可与正文提取器同时使用。
///
/// 快照限制为 64 KiB / 128 个头值，Accept 另限制为 16 KiB / 128 项；
/// 超限以 431 拒绝。Debug 隐藏头、令牌及 URI。
///
/// ```
/// use openrust::http::{Json, RequestInfo};
///
/// async fn inspect(info: RequestInfo, Json(payload): Json<String>) -> String {
///     format!("{}:{}", info.path(), payload)
/// }
/// ```
#[derive(Clone)]
pub struct RequestInfo {
    secure: std::result::Result<bool, std::sync::Arc<Error>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    acceptable: Vec<String>,
    cookies: serde_json::Value,
    route: super::route::RouteSlot,
    route_urls: Option<super::RouteUrlGenerator>,
    precognition: super::precognition::PrecognitionState,
    #[cfg(feature = "session")]
    session: Option<crate::session::Session>,
    #[cfg(feature = "view")]
    view_errors: std::sync::Arc<crate::view::RequestViewErrors>,
}

impl fmt::Debug for RequestInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestInfo")
            .finish_non_exhaustive()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestInfo {
    type Rejection = Error;

    // Axum 的提取器契约要求异步签名，快照无需等待 IO。
    #[allow(clippy::unused_async_trait_impl)]
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self> {
        validate_header_size(&parts.headers)?;
        let acceptable = parse_accept(&parts.headers)?;
        let parsed = match parts.extensions.get::<super::cookie::ParsedCookies>() {
            Some(parsed) => parsed.clone(),
            None => super::cookie::parse_headers(&parts.headers)?,
        };
        let mut headers = parts.headers.clone();
        if let Some(original) = parts.extensions.get::<super::cookie::OriginalCookies>() {
            headers.remove(http::header::COOKIE);
            for value in &original.headers {
                headers.append(http::header::COOKIE, value.clone());
            }
        }
        let route = parts
            .extensions
            .get::<super::route::RouteSlot>()
            .cloned()
            .unwrap_or_else(|| {
                let slot = super::route::RouteSlot::default();
                parts.extensions.insert(slot.clone());
                slot
            });
        let precognition = super::precognition::ensure_state(&mut parts.extensions);
        #[cfg(feature = "view")]
        let view_errors = if let Some(errors) = parts
            .extensions
            .get::<std::sync::Arc<crate::view::RequestViewErrors>>()
        {
            std::sync::Arc::clone(errors)
        } else {
            let errors = std::sync::Arc::new(crate::view::RequestViewErrors::empty());
            parts.extensions.insert(std::sync::Arc::clone(&errors));
            errors
        };
        Ok(Self {
            secure: super::transport::is_secure_parts(&parts.headers, &parts.extensions)
                .map_err(std::sync::Arc::new),
            method: parts.method.clone(),
            uri: parts
                .extensions
                .get::<OriginalUri>()
                .map_or_else(|| parts.uri.clone(), |original| original.0.clone()),
            headers,
            acceptable,
            cookies: parsed.value,
            route,
            route_urls: parts.extensions.get::<super::RouteUrlGenerator>().cloned(),
            precognition,
            #[cfg(feature = "session")]
            session: parts.extensions.get::<crate::session::Session>().cloned(),
            #[cfg(feature = "view")]
            view_errors,
        })
    }
}

impl RequestInfo {
    pub(crate) fn route_urls(&self) -> Result<&super::RouteUrlGenerator> {
        self.route_urls
            .as_ref()
            .ok_or_else(super::url::route_url_unavailable)
    }

    /// 在当前请求共享的错误包中添加或替换命名包，随后渲染立即可见。
    ///
    /// 已存在于 Session 的对象会同步修改，但不延长原 flash 寿命；显式
    /// `Session::flash_errors` 才标记下次请求可读。缺失回退只影响本请求，
    /// Session 整包替换后旧对象保持独立。重入或并发访问时返回错误。
    #[cfg(feature = "view")]
    pub fn put_view_error_bag(
        &self,
        name: impl Into<crate::support::ArrayKey>,
        bag: Box<dyn crate::support::MessageBagContract>,
    ) -> Result<()> {
        self.view_errors.put(name, bag)
    }

    #[cfg(feature = "view")]
    pub(crate) fn view_errors(&self) -> std::sync::Arc<crate::view::RequestViewErrors> {
        std::sync::Arc::clone(&self.view_errors)
    }
    /// 请求头首值严格等于 `true` 时表示尝试 Precognition；这不等于已确认。
    pub fn is_attempting_precognition(&self) -> Result<bool> {
        Ok(self.header("precognition")? == Some("true"))
    }

    /// 仅当受信 Precognition middleware 确认本请求时返回 `true`。
    pub fn is_precognitive(&self) -> bool {
        self.precognition.is_confirmed()
    }

    /// 取得当前实际匹配的路由；无业务命中返回 `None`。
    pub fn route(&self) -> Result<Option<super::Route>> {
        self.route.route()
    }

    /// 取得当前路由名称；匿名路由或无路由返回 `None`。
    pub fn route_name(&self) -> Result<Option<String>> {
        Ok(self
            .route()?
            .and_then(|route| route.name().map(str::to_owned)))
    }

    /// 判断当前路由名是否匹配任一 Laravel 通配符模式。
    pub fn route_is(&self, patterns: &[&str]) -> Result<bool> {
        Ok(self.route()?.is_some_and(|route| route.named(patterns)))
    }

    /// 取得当前路由的已绑定参数。
    pub fn route_parameter(&self, name: &str) -> Result<Option<serde_json::Value>> {
        Ok(self
            .route()?
            .and_then(|route| route.parameter(name).cloned()))
    }

    /// 有路由但缺少参数时返回默认值；无路由仍返回 `None`。
    pub fn route_parameter_or(
        &self,
        name: &str,
        default: serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        Ok(self.route()?.map(|route| route.parameter_or(name, default)))
    }

    /// 取得当前路由解析器，可由可信应用包装或替换。
    pub fn get_route_resolver(&self) -> Result<super::RouteResolver> {
        self.route.resolver()
    }

    /// 为当前请求设置可信应用的路由解析器。
    pub fn set_route_resolver(&self, resolver: super::RouteResolver) -> Result<()> {
        self.route.set_resolver(resolver)
    }
    /// 是否安装本请求的Session句柄，不隐式创建会话。
    #[cfg(feature = "session")]
    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }

    /// 取得同一请求的Session；未装配Provider时安全失败。
    #[cfg(feature = "session")]
    pub fn session(&self) -> Result<crate::session::Session> {
        self.session.clone().ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_layer_missing",
                "Request未装配Session",
            )
        })
    }

    /// 从当前请求的 Session 读取上次提交的输入；未装配 Session 时视为缺失。
    #[cfg(feature = "session")]
    pub fn old(&self, key: &str) -> Result<Option<crate::support::DataValue>> {
        self.session
            .as_ref()
            .map_or(Ok(None), |session| session.get_old_input(Some(key)))
    }

    /// 只有旧输入缺失时返回默认值；显式 null 不使用默认值。
    #[cfg(feature = "session")]
    pub fn old_or(
        &self,
        key: &str,
        default: crate::support::DataValue,
    ) -> Result<crate::support::DataValue> {
        Ok(self.old(key)?.unwrap_or(default))
    }

    /// 返回整个旧输入 bag；未装配 Session 时返回空数组。
    #[cfg(feature = "session")]
    pub fn old_all(&self) -> Result<crate::support::DataValue> {
        self.session.as_ref().map_or_else(
            || Ok(crate::support::DataValue::Array(Vec::new())),
            |session| {
                Ok(session
                    .get_old_input(None)?
                    .unwrap_or_else(|| crate::support::DataValue::Array(Vec::new())))
            },
        )
    }

    /// 显式把指定旧输入转为纯 JSON；对象存在时返回安全错误。
    #[cfg(feature = "session")]
    pub fn old_json(&self, key: &str) -> Result<Option<serde_json::Value>> {
        self.old(key)?.map(|value| value.to_json()).transpose()
    }

    /// 显式纯 JSON 默认值；只在键缺失时采用默认值。
    #[cfg(feature = "session")]
    pub fn old_or_json(&self, key: &str, default: serde_json::Value) -> Result<serde_json::Value> {
        Ok(self.old_json(key)?.unwrap_or(default))
    }

    /// 显式把完整旧输入转为纯 JSON；对象存在时返回安全错误。
    #[cfg(feature = "session")]
    pub fn old_all_json(&self) -> Result<serde_json::Value> {
        self.old_all()?.to_json()
    }

    /// 按完整名称读取 Cookie；名称中的点不作为路径。
    pub fn cookie(&self, name: &str) -> Option<serde_json::Value> {
        let value = match &self.cookies {
            serde_json::Value::Array(values) => name
                .parse::<usize>()
                .ok()
                .filter(|index| index.to_string() == name)
                .and_then(|index| values.get(index)),
            value => value.get(name),
        };
        value.filter(|value| !value.is_null()).cloned()
    }

    /// 缺失或解密失败为 null 时返回默认值。
    pub fn cookie_or(&self, name: &str, default: serde_json::Value) -> serde_json::Value {
        self.cookie(name).unwrap_or(default)
    }

    /// 返回 Cookie 输入树的独立快照。
    pub fn cookies(&self) -> serde_json::Value {
        self.cookies.clone()
    }

    /// 判断 Cookie 是否存在且非 null；空字符串仍存在。
    pub fn has_cookie(&self, name: &str) -> bool {
        self.cookie(name).is_some()
    }

    /// 返回不含首尾斜线且未经百分号解码的路径；根路径返回 `/`。
    pub fn path(&self) -> &str {
        let path = self.uri.path().trim_matches('/');
        if path.is_empty() { "/" } else { path }
    }

    /// 返回路由接收前保留的 URI。
    pub const fn uri(&self) -> &Uri {
        &self.uri
    }

    /// 返回 HTTP 方法。
    pub const fn method(&self) -> &Method {
        &self.method
    }

    /// 忽略 ASCII 大小写比较 HTTP 方法。
    pub fn is_method(&self, method: &str) -> bool {
        self.method.as_str().eq_ignore_ascii_case(method)
    }

    /// 校验 Host 与绝对 URI authority，并返回不含端口的主机名。
    pub fn host(&self) -> Result<String> {
        authority(&self.headers, &self.uri)
    }

    /// 返回首个同名文本头；非法名称或非 UTF-8 值返回稳定的 400 错误。
    pub fn header(&self, name: &str) -> Result<Option<&str>> {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid_header())?;
        self.headers
            .get(&name)
            .map(|value| std::str::from_utf8(value.as_bytes()).map_err(|_| invalid_header()))
            .transpose()
    }

    /// 判断是否存在同名头，包括二进制值。
    pub fn has_header(&self, name: &str) -> bool {
        HeaderName::from_bytes(name.as_bytes()).is_ok_and(|name| self.headers.contains_key(name))
    }

    /// 返回只读头集合，包括不能表示为 UTF-8 文本的值。
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// 从首个 Authorization 的最后一个大小写不敏感 `Bearer ` 后提取至逗号。
    pub fn bearer_token(&self) -> Result<Option<&str>> {
        let Some(header) = self.header("authorization")? else {
            return Ok(None);
        };
        let position = header
            .as_bytes()
            .windows(7)
            .rposition(|window| window.eq_ignore_ascii_case(b"Bearer "));
        Ok(position.map(|position| {
            let token = &header[position + 7..];
            token.split_once(',').map_or(token, |(token, _)| token)
        }))
    }

    /// 返回按质量降序排列的规范 Accept 条目；同质量保留声明顺序。
    pub fn acceptable_content_types(&self) -> &[String] {
        &self.acceptable
    }

    /// 按 Laravel `accepts` 的方向检查任一候选类型。
    pub fn accepts(&self, content_types: &[&str]) -> bool {
        if self.acceptable.is_empty() {
            return true;
        }
        self.acceptable.iter().any(|accept| {
            let accept = strip_params(accept).to_ascii_lowercase();
            accept == "*/*"
                || accept == "*"
                || content_types.iter().any(|content_type| {
                    let content_type = content_type.to_ascii_lowercase();
                    matches_type(&accept, &content_type)
                        || accept
                            == format!("{}/*", content_type.split('/').next().unwrap_or_default())
                })
        })
    }

    /// 按 Accept 优先级返回原候选；支持 Symfony 默认格式别名。
    pub fn prefers<'a>(&self, content_types: &[&'a str]) -> Option<&'a str> {
        for accept in &self.acceptable {
            let accept = strip_params(accept).to_ascii_lowercase();
            if accept == "*/*" || accept == "*" {
                return content_types.first().copied();
            }
            for content_type in content_types {
                let resolved = mime_for_format(content_type).unwrap_or(content_type);
                let resolved = resolved.to_ascii_lowercase();
                if matches_type(&resolved, &accept)
                    || accept == format!("{}/*", resolved.split('/').next().unwrap_or_default())
                {
                    return Some(content_type);
                }
            }
        }
        None
    }

    /// 判断 Accept 为空或首个条目是全通配符。
    pub fn accepts_any_content_type(&self) -> bool {
        self.acceptable
            .first()
            .is_none_or(|first| first == "*/*" || first == "*")
    }

    /// 判断是否接受 JSON。
    pub fn accepts_json(&self) -> bool {
        self.accepts(&["application/json"])
    }
    /// 判断是否接受 HTML。
    pub fn accepts_html(&self) -> bool {
        self.accepts(&["text/html"])
    }
    /// 判断是否接受 Markdown。
    pub fn accepts_markdown(&self) -> bool {
        self.accepts(&["text/markdown"])
    }

    /// 判断首选 Accept 条目是否包含 `/json` 或 `+json`。
    pub fn wants_json(&self) -> bool {
        self.acceptable.first().is_some_and(|first| {
            let first = first.to_ascii_lowercase();
            first.contains("/json") || first.contains("+json")
        })
    }

    /// 判断首选 Accept 条目是否以 `text/markdown` 开始。
    pub fn wants_markdown(&self) -> bool {
        self.acceptable
            .first()
            .is_some_and(|first| first.to_ascii_lowercase().starts_with("text/markdown"))
    }

    /// 判断请求是否期望 JSON，包括 AJAX 且非 PJAX 的通配请求。
    pub fn expects_json(&self) -> Result<bool> {
        Ok((self.ajax()? && !self.pjax()? && self.accepts_any_content_type()) || self.wants_json())
    }

    /// 按 Laravel 的大小写敏感内容类型检查 JSON 正文。
    pub fn is_json(&self) -> Result<bool> {
        Ok(self
            .header("content-type")?
            .is_some_and(|value| value.contains("/json") || value.contains("+json")))
    }

    /// 精确比较 `X-Requested-With: XMLHttpRequest`。
    pub fn ajax(&self) -> Result<bool> {
        Ok(self.header("x-requested-with")? == Some("XMLHttpRequest"))
    }

    /// 按 PHP 布尔转换判断 X-PJAX：空值和 `0` 为假。
    pub fn pjax(&self) -> Result<bool> {
        Ok(self
            .header("x-pjax")?
            .is_some_and(|value| !value.is_empty() && value != "0"))
    }

    /// 按 Symfony 默认格式表解析首个可识别 Accept 类型。
    pub fn format(&self, default: &str) -> String {
        self.acceptable
            .iter()
            .find_map(|value| format_for_mime(value))
            .unwrap_or(default)
            .to_owned()
    }
}

fn invalid_header() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "http_request_header_invalid",
        "request header is not valid text",
    )
}

fn oversized_headers() -> Error {
    Error::custom_with_status(
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
        "http_request_headers_too_large",
        "request headers exceed inspection limit",
    )
}

fn validate_header_size(headers: &HeaderMap) -> Result<()> {
    let mut size = 0_usize;
    let mut count = 0_usize;
    for (name, value) in headers {
        count += 1;
        size = size
            .saturating_add(name.as_str().len())
            .saturating_add(value.len());
        if count > MAX_HEADER_VALUES || size > MAX_HEADER_BYTES {
            return Err(oversized_headers());
        }
    }
    Ok(())
}

#[derive(Debug)]
struct AcceptItem {
    key: String,
    quality: f64,
    index: usize,
}

fn parse_accept(headers: &HeaderMap) -> Result<Vec<String>> {
    let Some(raw) = headers.get("accept") else {
        return Ok(Vec::new());
    };
    if raw.len() > MAX_ACCEPT_BYTES {
        return Err(oversized_headers());
    }
    let raw = std::str::from_utf8(raw.as_bytes()).map_err(|_| invalid_header())?;
    let items = split_outside_quotes(raw, ',');
    if items.len() > MAX_ACCEPT_ITEMS {
        return Err(oversized_headers());
    }
    let mut deduplicated = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        let segments = split_outside_quotes(item, ';');
        let value = segments.first().map_or("", |value| value.trim());
        if value.is_empty() {
            continue;
        }
        let mut params = BTreeMap::new();
        let mut quality = 1.0_f64;
        for segment in segments.iter().skip(1) {
            let (name, value) = segment
                .split_once('=')
                .map_or((segment.as_str(), None), |(name, value)| {
                    (name, Some(value))
                });
            let name = name.trim().to_ascii_lowercase();
            let value = Some(value.map_or_else(|| "1".to_owned(), |value| unquote(value.trim())));
            if name == "q" {
                quality = php_float(value.as_deref().unwrap_or_default());
            } else if !name.is_empty() {
                params.insert(name, value);
            }
        }
        let mut key = unquote(value);
        for (name, value) in params {
            key.push(';');
            key.push_str(&name);
            if let Some(value) = value {
                key.push('=');
                if value
                    .chars()
                    .any(|character| character.is_whitespace() || ";,=".contains(character))
                {
                    key.push('"');
                    for character in value.chars() {
                        if character == '"' || character == '\\' {
                            key.push('\\');
                        }
                        key.push(character);
                    }
                    key.push('"');
                } else {
                    key.push_str(&value);
                }
            }
        }
        deduplicated.insert(
            key.clone(),
            AcceptItem {
                key,
                quality,
                index,
            },
        );
    }
    let mut items: Vec<_> = deduplicated.into_values().collect();
    items.sort_by(|left, right| {
        // PHP 比较将正零与负零视为相等，此时仍按原声明位置排序。
        right
            .quality
            .partial_cmp(&left.quality)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.index.cmp(&right.index))
    });
    Ok(items.into_iter().map(|item| item.key).collect())
}

fn split_outside_quotes(input: &str, separator: char) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    for (index, character) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            _ if character == separator && !quoted => {
                chunks.push(input[start..index].trim().to_owned());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    chunks.push(input[start..].trim().to_owned());
    chunks
}

// Symfony HeaderUtils 将引号内的转义还原；未加引号的反斜线保持原样。
fn unquote(value: &str) -> String {
    let Some(value) = value.strip_prefix('"') else {
        return value.to_owned();
    };
    let value = value.strip_suffix('"').unwrap_or(value);
    let mut result = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            result.push(characters.next().unwrap_or('\\'));
        } else {
            result.push(character);
        }
    }
    result
}

// PHP 的 float 转换接受数值前缀，且不把 NaN/INF 文本作为特殊数字。
fn php_float(value: &str) -> f64 {
    let value = value.trim_start();
    let bytes = value.as_bytes();
    let mut end = usize::from(
        bytes
            .first()
            .is_some_and(|byte| matches!(byte, b'+' | b'-')),
    );
    let mut digits = 0;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
        digits += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return 0.0;
    }
    if bytes
        .get(end)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        let exponent = end;
        end += 1;
        if bytes
            .get(end)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            end += 1;
        }
        let first_digit = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == first_digit {
            end = exponent;
        }
    }
    value[..end].parse().unwrap_or(0.0)
}

fn strip_params(value: &str) -> &str {
    value
        .split_once(';')
        .map_or(value, |(value, _)| value.trim())
}

fn matches_type(actual: &str, requested: &str) -> bool {
    actual == requested
        || actual.split_once('/').is_some_and(|(main, suffix)| {
            requested
                .split_once(&format!("{main}/"))
                .is_some_and(|(_, rest)| {
                    rest.rfind(&format!("+{suffix}"))
                        .is_some_and(|index| index > 0)
                })
        })
}

const FORMATS: &[(&str, &[&str])] = &[
    ("html", &["text/html", "application/xhtml+xml"]),
    ("txt", &["text/plain"]),
    (
        "js",
        &[
            "application/javascript",
            "application/x-javascript",
            "text/javascript",
        ],
    ),
    ("css", &["text/css"]),
    ("json", &["application/json", "application/x-json"]),
    ("jsonld", &["application/ld+json"]),
    ("xml", &["text/xml", "application/xml", "application/x-xml"]),
    ("rdf", &["application/rdf+xml"]),
    ("atom", &["application/atom+xml"]),
    ("rss", &["application/rss+xml"]),
    (
        "form",
        &["application/x-www-form-urlencoded", "multipart/form-data"],
    ),
    ("soap", &["application/soap+xml"]),
    ("problem", &["application/problem+json"]),
    ("hal", &["application/hal+json", "application/hal+xml"]),
    ("jsonapi", &["application/vnd.api+json"]),
    ("yaml", &["text/yaml", "application/x-yaml"]),
    ("wbxml", &["application/vnd.wap.wbxml"]),
    ("pdf", &["application/pdf"]),
    ("csv", &["text/csv"]),
];

fn mime_for_format(format: &str) -> Option<&'static str> {
    FORMATS
        .iter()
        .find(|(name, _)| *name == format)
        .map(|(_, types)| types[0])
}

fn format_for_mime(mime: &str) -> Option<&'static str> {
    let canonical = strip_params(mime);
    if let Some((format, _)) = FORMATS.iter().find(|(_, types)| types.contains(&canonical)) {
        return Some(format);
    }
    let suffix = canonical.strip_prefix("application/")?.rsplit_once('+')?.1;
    Some(match suffix {
        "json" => "json",
        "xml" | "wbxml" => "xml",
        "xhtml" => "html",
        "cbor" => "cbor",
        "zip" => "zip",
        "ber" | "der" => "asn1",
        "tlv" => "tlv",
        "yaml" => "yaml",
        _ => return None,
    })
}
