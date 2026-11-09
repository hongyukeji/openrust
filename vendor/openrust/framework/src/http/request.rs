//! 有界请求输入、独立元信息与内容协商。

mod attributes;
mod conditional;
#[cfg(feature = "session")]
mod csrf_replay;
mod deserialize;
mod form;
mod input;
mod mime;
mod mime_map;
mod multipart;
mod normalize;
mod raw_body;
mod uploaded_file;
mod url;
mod url_query;
mod value;

pub(crate) fn deserialize_query<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> crate::Result<T> {
    deserialize::query(value)
}
pub use attributes::RequestAttributes;
pub use conditional::{InputOutcome, InputOutcomeMut, InputTruthy, Truthy};
#[cfg(feature = "session")]
pub(crate) use csrf_replay::prepare as prepare_csrf_request;
pub use input::Request;
pub use mime::{
    FileBinaryMimeTypeGuesser, MimeFile, MimeFileReader, MimeGuess, MimeTypeGuesser, MimeTypes,
};
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
pub use raw_body::{RawBodyReader, RequestBodyLimits};
pub use uploaded_file::{UploadedFile, UploadedFileReader};

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

/// 由受信运行时适配器提供的 Vapor 原始查询串。
///
/// 该值优先于 URI query 参与签名验证；普通客户端 HTTP 头不能设置此 extension。
#[derive(Clone)]
pub struct VaporRawQueryString(String);

impl VaporRawQueryString {
    /// 原样保存受信入口提供的原始查询字节文本，包括空字符串。
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
}

impl fmt::Debug for VaporRawQueryString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VaporRawQueryString([REDACTED])")
    }
}

/// 请求元信息快照和共享属性句柄；可与正文提取器同时使用。
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
    proxy: crate::http::proxy::ProxySnapshot,
    trusted_hosts: crate::http::trusted_hosts::TrustedHostsSnapshot,
    method: Method,
    uri: Uri,
    vapor_raw_query: Option<VaporRawQueryString>,
    headers: HeaderMap,
    acceptable: Vec<String>,
    cookies: serde_json::Value,
    attributes: RequestAttributes,
    route: crate::routing::route::RouteSlot,
    route_urls: Option<crate::routing::UrlGenerator>,
    precognition: crate::http::precognition::PrecognitionState,
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
        let parsed = match parts.extensions.get::<crate::http::cookie::ParsedCookies>() {
            Some(parsed) => parsed.clone(),
            None => crate::http::cookie::parse_headers(&parts.headers)?,
        };
        let mut headers = parts.headers.clone();
        if crate::http::proxy::platform_host(&parts.headers, &parts.uri) {
            headers.remove("x-forwarded-host");
        }
        if let Some(original) = parts
            .extensions
            .get::<crate::http::cookie::OriginalCookies>()
        {
            headers.remove(http::header::COOKIE);
            for value in &original.headers {
                headers.append(http::header::COOKIE, value.clone());
            }
        }
        let attributes = parts
            .extensions
            .get_or_insert_default::<RequestAttributes>()
            .clone();
        let route = parts
            .extensions
            .get::<crate::routing::route::RouteSlot>()
            .cloned()
            .unwrap_or_else(|| {
                let slot = crate::routing::route::RouteSlot::default();
                parts.extensions.insert(slot.clone());
                slot
            });
        let precognition = crate::http::precognition::ensure_state(&mut parts.extensions);
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
        let proxy = parts
            .extensions
            .get::<crate::http::proxy::ProxySnapshot>()
            .cloned()
            .unwrap_or_else(|| {
                crate::http::proxy::ProxySnapshot::capture(
                    &parts.headers,
                    &parts.uri,
                    &parts.extensions,
                    parts
                        .extensions
                        .get::<crate::foundation::ApplicationState>()
                        .map(|state| &state.config().server),
                )
            });
        let secure = proxy
            .0
            .as_ref()
            .map_err(std::sync::Arc::clone)
            .and_then(|origin| origin.secure().map_err(std::sync::Arc::new));
        Ok(Self {
            secure,
            proxy,
            trusted_hosts: parts
                .extensions
                .get::<crate::http::trusted_hosts::TrustedHostsSnapshot>()
                .cloned()
                .unwrap_or_default(),
            method: parts.method.clone(),
            uri: parts
                .extensions
                .get::<OriginalUri>()
                .map_or_else(|| parts.uri.clone(), |original| original.0.clone()),
            vapor_raw_query: parts.extensions.get::<VaporRawQueryString>().cloned(),
            headers,
            acceptable,
            cookies: parsed.value,
            attributes,
            route,
            route_urls: parts
                .extensions
                .get::<crate::routing::UrlGenerator>()
                .cloned(),
            precognition,
            #[cfg(feature = "session")]
            session: parts.extensions.get::<crate::session::Session>().cloned(),
            #[cfg(feature = "view")]
            view_errors,
        })
    }
}

impl RequestInfo {
    // 物理入口的认证快照早于路由的 Session middleware，外部扩展不是该快照的认证上下文。
    #[cfg(feature = "auth")]
    pub(crate) fn without_session(mut self) -> Self {
        self.session = None;
        self
    }

    /// 返回此请求共享的类型化属性袋。
    pub const fn attributes(&self) -> &RequestAttributes {
        &self.attributes
    }

    pub(crate) fn signing_raw_query(&self) -> Option<&str> {
        self.vapor_raw_query
            .as_ref()
            .map(|raw| raw.0.as_str())
            .or_else(|| self.uri.query())
    }
    /// 验证当前原始请求的绝对 URL 签名。
    pub fn has_valid_signature(&self) -> Result<bool> {
        self.route_urls()?.has_valid_signature(self, true, &[])
    }

    /// 验证当前原始请求的相对 URL 签名。
    pub fn has_valid_relative_signature(&self) -> Result<bool> {
        self.route_urls()?.has_valid_signature(self, false, &[])
    }

    /// 验证签名，同时忽略指定的原始查询字段名。
    pub fn has_valid_signature_while_ignoring(
        &self,
        ignore: &[&str],
        absolute: bool,
    ) -> Result<bool> {
        self.route_urls()?
            .has_valid_signature(self, absolute, ignore)
    }

    /// 按原始查询字段名回调决定忽略参数。
    pub fn has_valid_signature_ignoring(
        &self,
        absolute: bool,
        ignore: impl Fn(&str) -> bool,
    ) -> Result<bool> {
        self.route_urls()?
            .has_valid_signature_ignoring(self, absolute, ignore)
    }

    pub(crate) fn route_urls(&self) -> Result<&crate::routing::UrlGenerator> {
        self.route_urls
            .as_ref()
            .ok_or_else(crate::routing::url_generator::route_url_unavailable)
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
    pub fn route(&self) -> Result<Option<crate::routing::Route>> {
        self.route.route()
    }

    /// 取得当前路由名称；匿名路由或无路由返回 `None`。
    pub fn route_name(&self) -> Result<Option<String>> {
        Ok(self
            .route()?
            .and_then(|route| route.name().map(str::to_owned)))
    }

    /// 判断当前路由名是否匹配任一星号通配模式。
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
    pub fn get_route_resolver(&self) -> Result<crate::routing::RouteResolver> {
        self.route.resolver()
    }

    /// 为当前请求设置可信应用的路由解析器。
    pub fn set_route_resolver(&self, resolver: crate::routing::RouteResolver) -> Result<()> {
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
        let host = if let Some((host, _)) = self.proxy_context()?.host()? {
            host
        } else {
            authority(&self.headers, &self.uri)?
        };
        self.trusted_hosts.check(&host)?;
        Ok(host)
    }

    pub(crate) fn proxy_context(&self) -> Result<&crate::http::proxy::ProxyContext> {
        self.proxy
            .0
            .as_ref()
            .map(std::sync::Arc::as_ref)
            .map_err(|error| match error.as_ref() {
                Error::Config(crate::config::ConfigError::Invalid(message)) => {
                    Error::Config(crate::config::ConfigError::Invalid(message.clone()))
                }
                Error::BadRequest(message) => Error::BadRequest(message.clone()),
                _ => Error::internal(error.clone()),
            })
    }

    /// 返回当前请求中第一个非可信代理 IP；无 socket 信息的独立快照返回空值。
    pub fn client_ip(&self) -> Result<Option<std::net::IpAddr>> {
        Ok(self.proxy_context()?.client_ips()?.first().copied())
    }

    /// 返回按 Symfony 规则过滤并从最近非可信代理到原始客户端排列的 IP。
    pub fn client_ips(&self) -> Result<Vec<std::net::IpAddr>> {
        self.proxy_context()?.client_ips()
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

    /// 按当前请求的可接受类型检查任一候选类型。
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

    /// 用区分大小写的内容类型匹配检查 JSON 正文。
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

// 异常链在正文提取失败时仍需协商；复用同一有界 Accept 解析，但不重试错误提取器。
pub(crate) fn expects_json_headers(headers: &HeaderMap) -> bool {
    let Ok(acceptable) = parse_accept(headers) else {
        return false;
    };
    let wants_json = acceptable.first().is_some_and(|first| {
        let first = first.to_ascii_lowercase();
        first.contains("/json") || first.contains("+json")
    });
    let any = acceptable
        .first()
        .is_none_or(|first| first == "*/*" || first == "*");
    let ajax = headers
        .get("x-requested-with")
        .and_then(|value| value.to_str().ok())
        == Some("XMLHttpRequest");
    let pjax = headers
        .get("x-pjax")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.is_empty() && value != "0");
    wants_json || (ajax && !pjax && any)
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
