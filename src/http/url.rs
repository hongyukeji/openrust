//! 命名路由与控制器动作的共享 URL 生成。

use hmac::{Hmac, KeyInit, Mac};
use sha2_11::Sha256;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use subtle::ConstantTimeEq;

use crate::core::{Error, Result};
use crate::http::domain::DomainPattern;
use crate::http::routes::{ActionRouteDefinition, NamedRouteDefinition};
use axum::extract::FromRequestParts;
use axum::http::{StatusCode, request::Parts};

#[derive(Clone)]
struct NamedRoute {
    path: String,
    domain: Option<String>,
}

/// 根据 Laravel 式命名路由或动作身份生成绝对 URL。
///
/// 在 HTTP handler 中可直接提取。域参数按单个 DNS 标签校验，路径参数按路径段编码；
/// 未用于域或路径的参数成为 query 参数。scheme 和端口来自 `app.url`；
/// host 使用已登记的域模板，无域模板时来自 `app.url`，不采信当前请求的 `Host`。
#[derive(Clone)]
pub struct RouteUrlGenerator {
    inner: Arc<RouteUrlInner>,
    get_paths: Arc<BTreeSet<String>>,
    root_controller_namespace: Option<String>,
    key_resolver: Option<Arc<dyn Fn() -> Result<UrlSigningKeys> + Send + Sync>>,
}

/// 当前签名密钥与仅用于验签的旧密钥；Debug 不输出内容。
#[derive(Clone)]
pub struct UrlSigningKeys {
    current: Vec<u8>,
    previous: Vec<Vec<u8>>,
}

impl std::fmt::Debug for UrlSigningKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UrlSigningKeys([REDACTED])")
    }
}

impl UrlSigningKeys {
    /// 原样接收当前与旧密钥字节，不解码 `base64:` 前缀。
    pub fn new(current: impl Into<Vec<u8>>, previous: Vec<Vec<u8>>) -> Self {
        Self {
            current: current.into(),
            previous,
        }
    }
}

struct RouteUrlInner {
    base_url: Option<::url::Url>,
    routes: BTreeMap<String, NamedRoute>,
    actions: BTreeMap<String, NamedRoute>,
}

impl RouteUrlGenerator {
    pub(crate) fn empty() -> Self {
        Self {
            get_paths: Arc::new(BTreeSet::new()),
            inner: Arc::new(RouteUrlInner {
                base_url: None,
                routes: BTreeMap::new(),
                actions: BTreeMap::new(),
            }),
            root_controller_namespace: None,
            key_resolver: None,
        }
    }

    pub(crate) fn new(base_url: &str, routes: Vec<NamedRouteDefinition>) -> Result<Self> {
        Self::new_with_actions(base_url, routes, Vec::new())
    }

    pub(crate) fn new_with_actions(
        base_url: &str,
        routes: Vec<NamedRouteDefinition>,
        actions: Vec<ActionRouteDefinition>,
    ) -> Result<Self> {
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
            validate_url_domain(&path, domain.as_deref())?;
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
            validate_url_path(&path)?;
        }

        let mut indexed = BTreeMap::new();
        for ActionRouteDefinition {
            action,
            path,
            domain,
        } in actions
        {
            crate::http::action::validate_identity(&action)?;
            validate_url_domain(&path, domain.as_deref())?;
            validate_url_path(&path)?;
            // 上游用原始 controller 检查存在，却按 trim 后的键保存。
            // 因此带首尾反斜线的后项可覆盖同一规范键，不能简化为 entry.or_insert。
            if !indexed.contains_key(&action) {
                indexed.insert(
                    action.trim_matches('\\').to_owned(),
                    NamedRoute { path, domain },
                );
            }
        }

        Ok(Self {
            get_paths: Arc::new(BTreeSet::new()),
            inner: Arc::new(RouteUrlInner {
                base_url: Some(base_url),
                routes: named,
                actions: indexed,
            }),
            root_controller_namespace: None,
            key_resolver: None,
        })
    }

    /// 判断当前路由集合是否登记指定名称，不尝试生成其必需参数。
    pub fn has(&self, name: &str) -> bool {
        self.inner.routes.contains_key(name)
    }

    /// 查询真实GET声明的路径，包含未命名路由，不以URL生成成功代替路由存在。
    pub fn has_get_path(&self, path: &str) -> bool {
        self.get_paths.contains(path)
    }

    pub(crate) fn set_get_paths(&mut self, paths: Vec<String>) {
        self.get_paths = Arc::new(paths.into_iter().collect());
    }

    /// 按名称生成URL；缺失的域或路径参数报错，额外参数按字典序加入query。
    pub fn route(&self, name: &str, parameters: &BTreeMap<String, String>) -> Result<String> {
        let route = self.inner.routes.get(name).ok_or_else(|| {
            generation_error(
                "http_named_route_not_found",
                "named route was not registered",
            )
        })?;

        self.render(route, parameters)
    }

    /// 生成永久命名路由签名 URL；`absolute=false` 时只签路径和查询串。
    pub fn signed_route(
        &self,
        name: &str,
        parameters: &BTreeMap<String, String>,
        absolute: bool,
    ) -> Result<String> {
        self.signed_route_at(name, parameters, None, absolute)
    }

    /// 按当前时间加秒数生成临时签名 URL；到期秒数为 Unix 时间戳。
    pub fn temporary_signed_route(
        &self,
        name: &str,
        seconds: i64,
        parameters: &BTreeMap<String, String>,
        absolute: bool,
    ) -> Result<String> {
        let now = unix_now()?;
        let expires = now.checked_add(seconds).ok_or_else(|| {
            generation_error("url_signing_expiry_invalid", "签名 URL 到期时间超出范围")
        })?;
        self.signed_route_at(name, parameters, Some(expires), absolute)
    }

    /// 用显式 Unix 时间戳生成临时链接；供日期时间对象的 Rust 调用方转换后使用。
    pub fn signed_route_at(
        &self,
        name: &str,
        parameters: &BTreeMap<String, String>,
        expires: Option<i64>,
        absolute: bool,
    ) -> Result<String> {
        if parameters.contains_key("signature") || parameters.contains_key("expires") {
            return Err(generation_error(
                "url_signing_parameter_reserved",
                "signature 和 expires 是签名 URL 保留参数",
            ));
        }
        let keys = self.signing_keys()?;
        let mut parameters = parameters.clone();
        if let Some(expires) = expires {
            parameters.insert("expires".to_owned(), expires.to_string());
        }
        let mut unsigned = self.route(name, &parameters)?;
        if let Some((path, query)) = unsigned.split_once('?') {
            unsigned = format!("{path}?{}", query.replace('+', "%20"));
        }
        if !absolute {
            let parsed = ::url::Url::parse(&unsigned)
                .map_err(|_| generation_error("url_base_invalid", "签名 URL 的绝对地址无效"))?;
            parsed.path().clone_into(&mut unsigned);
            if let Some(query) = parsed.query() {
                unsigned.push('?');
                unsigned.push_str(query);
            }
        }
        let signature = signature(&unsigned, &keys.current)?;
        unsigned.push(if unsigned.contains('?') { '&' } else { '?' });
        unsigned.push_str("signature=");
        unsigned.push_str(&signature);
        Ok(unsigned)
    }

    /// 对保留原始 URI 与查询字节的请求验证绝对或相对签名。
    pub fn has_valid_signature(
        &self,
        request: &crate::http::RequestInfo,
        absolute: bool,
        ignore: &[&str],
    ) -> Result<bool> {
        self.has_valid_signature_ignoring(request, absolute, |key| ignore.contains(&key))
    }

    /// 使用未解码的原始查询字段名回调选择忽略项。
    #[allow(
        clippy::cast_precision_loss,
        reason = "PHP 的 numeric expires 与当前 Unix 秒比较；现实时间远低于 f64 精确整数上限"
    )]
    pub fn has_valid_signature_ignoring(
        &self,
        request: &crate::http::RequestInfo,
        absolute: bool,
        ignore: impl Fn(&str) -> bool,
    ) -> Result<bool> {
        let query = request.uri().query().unwrap_or("");
        let mut supplied = None;
        let mut expires = None;
        for (key, value) in ::url::form_urlencoded::parse(query.as_bytes()) {
            if key == "signature" {
                supplied = Some(value.into_owned());
            } else if key.starts_with("signature[") {
                return Ok(false);
            } else if key == "expires" {
                expires = Some(value.into_owned());
            } else if key.starts_with("expires[") {
                return Ok(false);
            }
        }
        let Some(supplied) = supplied else {
            return Ok(false);
        };
        let mut message = if absolute {
            format!(
                "{}{}",
                request.root()?,
                request.uri().path().trim_end_matches('/')
            )
        } else {
            format!("/{}", request.path())
        };
        let raw = request
            .signing_raw_query()
            .unwrap_or("")
            .split('&')
            .filter(|part| {
                let key = part.split_once('=').map_or(*part, |(key, _)| key);
                key != "signature" && !ignore(key)
            })
            .collect::<Vec<_>>()
            .join("&");
        if !raw.is_empty() {
            message.push('?');
            message.push_str(&raw);
        }
        let keys = self.signing_keys()?;
        let valid = std::iter::once(&keys.current)
            .chain(keys.previous.iter())
            .try_fold(false, |matched, key| {
                let expected = signature(&message, key)?;
                Ok::<bool, Error>(
                    matched | bool::from(expected.as_bytes().ct_eq(supplied.as_bytes())),
                )
            })?;
        if !valid {
            return Ok(false);
        }
        if let Some(expires) = expires.filter(|expires| !expires.is_empty() && expires != "0") {
            let now = unix_now()?;
            if expires
                .parse::<f64>()
                .is_ok_and(|timestamp| (now as f64) > timestamp)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 按动作身份生成绝对 URL；参数、域编码及错误与命名路由共用同一内核。
    ///
    /// 无首反斜线的字符串使用本实例根命名空间；绝对字符串和二元组绕过它。
    /// 裸 invokable 身份不会自动追加 `@__invoke`，也不实例化控制器。
    pub fn action(
        &self,
        action: impl Into<crate::http::RouteAction>,
        parameters: &BTreeMap<String, String>,
    ) -> Result<String> {
        let action = action
            .into()
            .query(self.root_controller_namespace.as_deref())?;
        let route = self.inner.actions.get(&action).ok_or_else(|| {
            generation_error(
                "http_route_action_not_found",
                format!("动作 `{action}` 未登记"),
            )
        })?;
        self.render(route, parameters)
    }

    /// 设置本实例查询动作的根命名空间；空字符串清空，原样保留其余合法输入。
    ///
    /// 已有 clone 与请求快照不受后续修改影响；不可变路由索引仍共享同一 Arc。
    /// 这是 Rust 所有权适配，不模拟 PHP 跨引用共享可变对象。
    pub fn set_root_controller_namespace(
        &mut self,
        namespace: impl Into<String>,
    ) -> Result<&mut Self> {
        let namespace = namespace.into();
        crate::http::action::validate_namespace(&namespace)?;
        self.root_controller_namespace = (!namespace.is_empty()).then_some(namespace);
        Ok(self)
    }

    /// 替换本实例的签名密钥解析器；已有 clone 和请求快照保持原解析器。
    pub fn set_key_resolver(
        &mut self,
        resolver: impl Fn() -> Result<UrlSigningKeys> + Send + Sync + 'static,
    ) -> &mut Self {
        self.key_resolver = Some(Arc::new(resolver));
        self
    }

    /// 克隆 URL 生成器并只替换克隆的密钥解析器。
    #[must_use]
    pub fn with_key_resolver(
        &self,
        resolver: impl Fn() -> Result<UrlSigningKeys> + Send + Sync + 'static,
    ) -> Self {
        let mut cloned = self.clone();
        cloned.set_key_resolver(resolver);
        cloned
    }

    pub(crate) fn signing_keys(&self) -> Result<UrlSigningKeys> {
        self.key_resolver.as_ref().ok_or_else(|| {
            Error::custom(
                "url_signing_key_unavailable",
                "app.key is unavailable for URL signing",
            )
        })?()
    }

    fn render(&self, route: &NamedRoute, parameters: &BTreeMap<String, String>) -> Result<String> {
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

fn signature(message: &str, key: &[u8]) -> Result<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| generation_error("url_signing_key_invalid", "签名密钥无效"))?;
    mac.update(message.as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(encoded)
}

fn unix_now() -> Result<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| generation_error("url_signing_time_invalid", "系统时间早于 Unix 纪元"))?
            .as_secs(),
    )
    .map_err(|_| generation_error("url_signing_time_invalid", "系统时间超出 Unix 范围"))
}

fn validate_url_domain(path: &str, domain: Option<&str>) -> Result<()> {
    if let Some(pattern) = domain {
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
    Ok(())
}

fn validate_url_path(path: &str) -> Result<()> {
    let mut normalized_path = path;
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
    Ok(())
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

#[cfg(test)]
mod action_tests {
    use super::*;
    use crate::http::{RequestInfo, VaporRawQueryString, transport::RequestSecurity};
    use axum::body::Body;
    use axum::extract::{FromRequestParts, OriginalUri};
    use axum::http::Request;

    fn oracle_cases() -> Result<Vec<serde_json::Value>> {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/laravel_signed_url.json"))
                .map_err(Error::internal)?;
        assert_eq!(
            fixture["baseline"],
            "d52c3b3129e4c8e213c97cdc68f94b0b7d27f181"
        );
        fixture["cases"]
            .as_array()
            .cloned()
            .ok_or_else(|| Error::custom("signed_oracle_invalid", "Laravel fixture cases missing"))
    }

    fn oracle_case<'a>(cases: &'a [serde_json::Value], id: &str) -> Result<&'a serde_json::Value> {
        cases
            .iter()
            .find(|case| case["caseid"] == id)
            .ok_or_else(|| {
                Error::custom(
                    "signed_oracle_invalid",
                    format!("Laravel fixture case {id} missing"),
                )
            })
    }

    fn oracle_urls(key: Vec<u8>, previous: Vec<Vec<u8>>) -> Result<RouteUrlGenerator> {
        let mut urls = RouteUrlGenerator::new(
            "https://example.test",
            vec![NamedRouteDefinition {
                name: "article.show".into(),
                path: "/article/{article}".into(),
                domain: None,
            }],
        )?;
        let keys = UrlSigningKeys::new(key, previous);
        urls.set_key_resolver(move || Ok(keys.clone()));
        Ok(urls)
    }

    async fn oracle_info(url: &str, vapor_query: Option<&str>) -> Result<RequestInfo> {
        let path = url.strip_prefix("https://example.test").unwrap_or(url);
        let request = Request::builder()
            .uri(path)
            .header("host", "example.test")
            .body(Body::empty())
            .map_err(Error::internal)?;
        let (mut parts, _) = request.into_parts();
        parts.extensions.insert(RequestSecurity::new(true));
        if let Some(raw) = vapor_query {
            parts.extensions.insert(VaporRawQueryString::new(raw));
        }
        RequestInfo::from_request_parts(&mut parts, &()).await
    }

    fn definition(action: &str, path: &str, domain: Option<&str>) -> ActionRouteDefinition {
        ActionRouteDefinition {
            action: action.into(),
            path: path.into(),
            domain: domain.map(str::to_owned),
        }
    }

    #[test]
    fn action_and_named_route_share_domain_path_query_and_parameter_errors() -> Result<()> {
        // 动作不能另做一套漏编码、漏缺参检查的URL拼接器。
        let urls = RouteUrlGenerator::new_with_actions(
            "https://configured.test:8443/base",
            vec![NamedRouteDefinition {
                name: "draft".into(),
                path: "/draft/{id}/".into(),
                domain: Some("{tenant}.example.test".into()),
            }],
            vec![definition(
                "Draft@show",
                "/draft/{id}/",
                Some("{tenant}.example.test"),
            )],
        )?;
        let parameters = BTreeMap::from([
            ("tenant".into(), "ACME".into()),
            ("id".into(), "a/b ü".into()),
            ("a".into(), "x&y".into()),
            ("z".into(), "last".into()),
        ]);
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            urls.route("draft", &parameters)?
        );
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            "https://acme.example.test:8443/base/draft/a%2Fb%20%C3%BC/?a=x%26y&z=last"
        );
        for (key, value, code) in [
            ("id", ".", "http_route_parameter_invalid"),
            ("tenant", "not.one.label", "http_route_parameter_invalid"),
        ] {
            let mut invalid = parameters.clone();
            invalid.insert(key.into(), value.into());
            assert_eq!(
                urls.action("Draft@show", &invalid)
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some(code)
            );
        }
        let mut missing = parameters;
        missing.remove("id");
        assert_eq!(
            urls.action("Draft@show", &missing)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_parameter_missing")
        );
        assert_eq!(
            urls.action("unknown", &missing)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        Ok(())
    }

    #[tokio::test]
    async fn action_namespace_mutation_is_local_to_clones_and_captured_request() -> Result<()> {
        // 请求捕获与clone共享不可变索引，而非共享可变查询namespace。
        let mut urls = RouteUrlGenerator::new_with_actions(
            "https://configured.test",
            Vec::new(),
            vec![
                definition("App\\Draft@show", "/app", None),
                definition("Draft@show", "/absolute", None),
                definition("App\\Invokable", "/invoke", None),
            ],
        )?;
        let before = urls.clone();
        urls.set_root_controller_namespace("App")?;
        let after = urls.clone();
        assert!(Arc::ptr_eq(&before.inner, &urls.inner));
        assert!(Arc::ptr_eq(&after.inner, &urls.inner));
        let (mut parts, _) = Request::get("/captured")
            .header("host", "malicious.test")
            .body(Body::empty())
            .map_err(|error| Error::custom("test_request_invalid", error.to_string()))?
            .into_parts();
        parts.extensions.insert(urls.clone());
        let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
        urls.set_root_controller_namespace("")?;
        let parameters = BTreeMap::new();
        assert_eq!(
            before.action("Draft@show", &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            after.action("Draft@show", &parameters)?,
            "https://configured.test/app"
        );
        assert_eq!(
            info.route_urls()?.action("Draft@show", &parameters)?,
            "https://configured.test/app"
        );
        assert!(Arc::ptr_eq(&info.route_urls()?.inner, &urls.inner));
        assert_eq!(
            after.action(("Draft", "show"), &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            after.action("\\Draft@show", &parameters)?,
            "https://configured.test/absolute"
        );
        assert_eq!(
            after.action("Invokable", &parameters)?,
            "https://configured.test/invoke"
        );
        assert_eq!(
            after
                .action("App\\Draft@show", &parameters)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        urls.set_root_controller_namespace("App")?;
        assert!(urls.set_root_controller_namespace("Bad\n").is_err());
        assert_eq!(
            urls.action("Draft@show", &parameters)?,
            "https://configured.test/app"
        );
        Ok(())
    }

    #[test]
    fn action_lookup_preserves_raw_index_asymmetry_and_query_validation() -> Result<()> {
        let urls = RouteUrlGenerator::new_with_actions(
            "http://configured.test",
            Vec::new(),
            vec![
                definition("Draft@show", "/first", None),
                definition("\\Draft@show\\", "/second", None),
                definition("Draft@show", "/third", None),
                definition("Invokable", "/invoke", None),
            ],
        )?;
        let parameters = BTreeMap::new();
        assert_eq!(
            urls.action("\\Draft@show\\", &parameters)?,
            "http://configured.test/second"
        );
        assert_eq!(
            urls.action(("Draft", "show"), &parameters)?,
            "http://configured.test/second"
        );
        assert_eq!(
            urls.action("Invokable", &parameters)?,
            "http://configured.test/invoke"
        );
        for action in [
            crate::http::RouteAction::from(""),
            crate::http::RouteAction::from(("Draft", "")),
            crate::http::RouteAction::from("Draft\r@show"),
        ] {
            assert_eq!(
                urls.action(action, &parameters)
                    .err()
                    .map(|error| error.code().to_owned())
                    .as_deref(),
                Some("http_route_action_invalid")
            );
        }
        assert_eq!(
            RouteUrlGenerator::empty()
                .action("Draft@show", &parameters)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("http_route_action_not_found")
        );
        Ok(())
    }

    #[test]
    fn signed_generation_matches_fixed_laravel_php_oracle() -> Result<()> {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/laravel_signed_url.json"))
                .map_err(Error::internal)?;
        assert_eq!(
            fixture["baseline"],
            "d52c3b3129e4c8e213c97cdc68f94b0b7d27f181"
        );
        let cases = fixture["cases"].as_array().ok_or_else(|| {
            Error::custom("signed_oracle_invalid", "Laravel fixture cases missing")
        })?;
        for caseid in [
            "generate_raw_base64_prefix",
            "generate_raw_whitespace",
            "generate_key_missing_null",
            "generate_key_empty",
            "relative_and_absolute",
            "query_reordering_and_encoding",
        ] {
            let case = cases
                .iter()
                .find(|case| case["caseid"] == caseid)
                .ok_or_else(|| {
                    Error::custom(
                        "signed_oracle_invalid",
                        format!("Laravel fixture case {caseid} missing"),
                    )
                })?;
            let key = case["input"]["key"].as_str().unwrap_or_default().to_owned();
            let mut urls = RouteUrlGenerator::new(
                "https://example.test",
                vec![NamedRouteDefinition {
                    name: "article.show".into(),
                    path: "/article/{article}".into(),
                    domain: None,
                }],
            )?;
            urls.set_key_resolver(move || Ok(UrlSigningKeys::new(key.as_bytes(), Vec::new())));
            let mut parameters = BTreeMap::from([("article".into(), "42".into())]);
            if caseid == "generate_raw_base64_prefix" {
                parameters.insert("a".into(), "first".into());
                parameters.insert("z".into(), "last".into());
            } else if caseid == "relative_and_absolute" {
                parameters.insert("a".into(), "one".into());
                parameters.insert("b".into(), "two".into());
            } else if caseid == "query_reordering_and_encoding" {
                parameters.insert("a".into(), "x/y".into());
                parameters.insert("z".into(), "a b".into());
            }
            let generated = urls.signed_route(
                "article.show",
                &parameters,
                caseid != "relative_and_absolute",
            )?;
            assert_eq!(generated, case["expected"]["url"], "{caseid}");
        }
        Ok(())
    }

    #[test]
    fn signed_key_resolver_is_instance_local_and_plain_routes_need_no_key() -> Result<()> {
        let mut urls = RouteUrlGenerator::new(
            "https://example.test",
            vec![NamedRouteDefinition {
                name: "article.show".into(),
                path: "/article/{article}".into(),
                domain: None,
            }],
        )?;
        let params = BTreeMap::from([("article".into(), "42".into())]);
        assert_eq!(
            urls.route("article.show", &params)?,
            "https://example.test/article/42"
        );
        assert_eq!(
            urls.signed_route("article.show", &params, true)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("url_signing_key_unavailable")
        );
        urls.set_key_resolver(|| Ok(UrlSigningKeys::new(b"first".to_vec(), Vec::new())));
        let before = urls.clone();
        let after =
            urls.with_key_resolver(|| Ok(UrlSigningKeys::new(b"second".to_vec(), Vec::new())));
        assert_eq!(
            before.signed_route("article.show", &params, true)?,
            urls.signed_route("article.show", &params, true)?
        );
        assert_ne!(
            before.signed_route("article.show", &params, true)?,
            after.signed_route("article.show", &params, true)?
        );
        let reserved = BTreeMap::from([
            ("article".into(), "42".into()),
            ("signature".into(), "x".into()),
        ]);
        assert_eq!(
            urls.signed_route("article.show", &reserved, true)
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("url_signing_parameter_reserved")
        );
        Ok(())
    }

    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "固定 PHP 27 案按验签责任逐项覆盖，输入和断言同处便于审计"
    )]
    async fn signed_verification_matches_all_expressible_fixed_php_oracle_cases() -> Result<()> {
        let cases = oracle_cases()?;
        for id in [
            "generate_raw_base64_prefix",
            "generate_raw_whitespace",
            "generate_key_missing_null",
            "generate_key_empty",
            "relative_and_absolute",
            "query_reordering_and_encoding",
        ] {
            let case = oracle_case(&cases, id)?;
            let key = case["input"]["key"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec();
            let url = case["expected"]["url"]
                .as_str()
                .ok_or_else(|| Error::custom("signed_oracle_invalid", "URL 缺失"))?;
            let info = oracle_info(url, None).await?;
            let relative = id == "relative_and_absolute";
            let expected_key = if relative {
                "valid_relative"
            } else if id == "query_reordering_and_encoding" {
                "valid_original"
            } else {
                "valid"
            };
            assert_eq!(
                oracle_urls(key, Vec::new())?.has_valid_signature(&info, !relative, &[])?,
                case["expected"][expected_key].as_bool().unwrap_or(false),
                "{id}"
            );
            if relative {
                assert_eq!(
                    oracle_urls(b"raw-key".to_vec(), Vec::new())?.has_valid_signature(
                        &info,
                        true,
                        &[]
                    )?,
                    case["expected"]["valid_absolute"].as_bool().unwrap_or(true),
                    "{id} absolute"
                );
            }
            if id == "generate_raw_base64_prefix" {
                assert_eq!(
                    oracle_urls(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ".to_vec(), Vec::new())?
                        .has_valid_signature(&info, true, &[])?,
                    case["expected"]["valid_with_decoded_key"]
                        .as_bool()
                        .unwrap_or(true)
                );
            }
            if id == "generate_raw_whitespace" {
                assert_eq!(
                    oracle_urls(b"raw key".to_vec(), Vec::new())?.has_valid_signature(
                        &info,
                        true,
                        &[]
                    )?,
                    case["expected"]["valid_with_trimmed_key"]
                        .as_bool()
                        .unwrap_or(true)
                );
            }
        }
        let case = oracle_case(&cases, "query_reordering_and_encoding")?;
        let reordered = case["expected"]["reordered_url"]
            .as_str()
            .ok_or_else(|| Error::custom("signed_oracle_invalid", "reordered URL 缺失"))?;
        assert_eq!(
            oracle_urls(b"raw-key".to_vec(), Vec::new())?.has_valid_signature(
                &oracle_info(reordered, None).await?,
                true,
                &[]
            )?,
            case["expected"]["valid_reordered"]
                .as_bool()
                .unwrap_or(true)
        );
        let case = oracle_case(&cases, "sorted_mixed_query_values")?;
        let url = case["expected"]["url"]
            .as_str()
            .ok_or_else(|| Error::custom("signed_oracle_invalid", "mixed URL 缺失"))?;
        assert_eq!(
            oracle_urls(b"raw-key".to_vec(), Vec::new())?.has_valid_signature(
                &oracle_info(url, None).await?,
                true,
                &[]
            )?,
            case["expected"]["valid"].as_bool().unwrap_or(false)
        );
        let case = oracle_case(&cases, "previous_key_zero")?;
        let signed_by_zero = oracle_urls(b"0".to_vec(), Vec::new())?.signed_route(
            "article.show",
            &BTreeMap::from([("article".into(), "42".into())]),
            true,
        )?;
        assert_eq!(
            oracle_urls(b"active".to_vec(), vec![b"0".to_vec()])?.has_valid_signature(
                &oracle_info(&signed_by_zero, None).await?,
                true,
                &[]
            )?,
            case["expected"]["valid_with_previous_zero"]
                .as_bool()
                .unwrap_or(false)
        );
        for id in ["ignore_list_raw_name", "ignore_encoded_name_is_raw"] {
            let case = oracle_case(&cases, id)?;
            let url = case["expected"]["url"]
                .as_str()
                .ok_or_else(|| Error::custom("signed_oracle_invalid", "ignore URL 缺失"))?;
            let info = oracle_info(url, None).await?;
            let urls = oracle_urls(b"raw-key".to_vec(), Vec::new())?;
            let first = &["utm"][..];
            let second = if id == "ignore_list_raw_name" {
                &["utm", "keep"][..]
            } else {
                &["%75tm"][..]
            };
            assert_eq!(
                urls.has_valid_signature(&info, true, first)?,
                case["expected"][if id == "ignore_list_raw_name" {
                    "ignore_utm"
                } else {
                    "ignore_decoded_name"
                }]
                .as_bool()
                .unwrap_or(true),
                "{id} first"
            );
            assert_eq!(
                urls.has_valid_signature(&info, true, second)?,
                case["expected"][if id == "ignore_list_raw_name" {
                    "ignore_both"
                } else {
                    "ignore_raw_name"
                }]
                .as_bool()
                .unwrap_or(false),
                "{id} second"
            );
        }
        for id in ["query_empty_segment", "ignore_predicate"] {
            let case = oracle_case(&cases, id)?;
            let url = case["expected"]["url"]
                .as_str()
                .ok_or_else(|| Error::custom("signed_oracle_invalid", "raw URL 缺失"))?;
            let info = oracle_info(url, None).await?;
            let urls = oracle_urls(b"raw-key".to_vec(), Vec::new())?;
            let valid = if id == "ignore_predicate" {
                urls.has_valid_signature_ignoring(&info, true, |name| name.starts_with('x'))?
            } else {
                urls.has_valid_signature(&info, true, &[])?
            };
            let expected_key = if id == "ignore_predicate" {
                "valid"
            } else {
                "correct"
            };
            assert_eq!(
                valid,
                case["expected"][expected_key].as_bool().unwrap_or(false),
                "{id}"
            );
        }
        let repeated = "https://example.test/article/42?foo=one&foo=two";
        let repeated_url = format!("{repeated}&signature={}", signature(repeated, b"raw-key")?);
        let urls = oracle_urls(b"raw-key".to_vec(), Vec::new())?;
        assert!(urls.has_valid_signature(&oracle_info(&repeated_url, None).await?, true, &[])?);
        assert!(
            !urls.has_valid_signature(
                &oracle_info(
                    &repeated_url.replace("foo=one&foo=two", "foo=two&foo=one"),
                    None
                )
                .await?,
                true,
                &[]
            )?
        );
        let case = oracle_case(&cases, "raw_query_vapor_override")?;
        let url = case["expected"]["url"]
            .as_str()
            .ok_or_else(|| Error::custom("signed_oracle_invalid", "Vapor URL 缺失"))?;
        let raw = case["expected"]["raw_query"]
            .as_str()
            .ok_or_else(|| Error::custom("signed_oracle_invalid", "Vapor raw query 缺失"))?;
        let urls = oracle_urls(b"raw-key".to_vec(), Vec::new())?;
        assert!(!urls.has_valid_signature(&oracle_info(url, None).await?, true, &[])?);
        assert_eq!(
            urls.has_valid_signature(&oracle_info(url, Some(raw)).await?, true, &[])?,
            case["expected"]["valid_with_override"]
                .as_bool()
                .unwrap_or(false)
        );
        for id in [
            "signature_array",
            "signature_empty",
            "signature_uppercase",
            "signature_short",
        ] {
            let case = oracle_case(&cases, id)?;
            let signature = if id == "signature_array" {
                "signature%5B0%5D=bad".to_owned()
            } else {
                format!(
                    "signature={}",
                    case["input"]["signature"].as_str().unwrap_or_default()
                )
            };
            let info = oracle_info(
                &format!("https://example.test/article/42?{signature}"),
                None,
            )
            .await?;
            assert_eq!(
                oracle_urls(b"raw-key".to_vec(), Vec::new())?.has_valid_signature(
                    &info,
                    true,
                    &[]
                )?,
                case["expected"]["correct"].as_bool().unwrap_or(true),
                "{id}"
            );
        }
        for id in [
            "expiry_absent",
            "expiry_empty",
            "expiry_zero",
            "expiry_array",
            "expiry_past",
            "expiry_future",
            "expiry_nonnumeric",
        ] {
            let case = oracle_case(&cases, id)?;
            let expiry = if id == "expiry_array" {
                Some("expires%5B0%5D=1".to_owned())
            } else {
                case["input"]["expires"]
                    .as_str()
                    .map(|value| format!("expires={value}"))
            };
            let unsigned = match expiry {
                Some(expiry) => format!("https://example.test/article/42?{expiry}"),
                None => "https://example.test/article/42".to_owned(),
            };
            let separator = if unsigned.contains('?') { '&' } else { '?' };
            let url = format!(
                "{unsigned}{separator}signature={}",
                signature(&unsigned, b"raw-key")?
            );
            let valid = oracle_urls(b"raw-key".to_vec(), Vec::new())?.has_valid_signature(
                &oracle_info(&url, None).await?,
                true,
                &[],
            )?;
            assert_eq!(
                valid,
                case["expected"]["not_expired"].as_bool().unwrap_or(false),
                "{id}"
            );
        }
        let case = oracle_case(&cases, "temporary_expiry_fixed_timestamps")?;
        for (url_key, valid_key) in [("future_url", "future_valid"), ("past_url", "past_valid")] {
            let url = case["expected"][url_key]
                .as_str()
                .ok_or_else(|| Error::custom("signed_oracle_invalid", "temporary URL 缺失"))?;
            assert_eq!(
                oracle_urls(b"raw-key".to_vec(), Vec::new())?.has_valid_signature(
                    &oracle_info(url, None).await?,
                    true,
                    &[]
                )?,
                case["expected"][valid_key].as_bool().unwrap_or(false),
                "{url_key}"
            );
        }
        for id in ["reserved_signature", "reserved_expires"] {
            let case = oracle_case(&cases, id)?;
            let name = id.strip_prefix("reserved_").unwrap_or_default();
            let params =
                BTreeMap::from([("article".into(), "42".into()), (name.into(), "1".into())]);
            assert!(
                oracle_urls(b"raw-key".to_vec(), Vec::new())?
                    .signed_route("article.show", &params, true)
                    .is_err(),
                "{id}"
            );
            assert_eq!(case["error"]["class"], "InvalidArgumentException");
        }
        Ok(())
    }

    #[tokio::test]
    async fn signed_validation_uses_trusted_original_uri_not_forwarded_prefix_header() -> Result<()>
    {
        let mut urls = RouteUrlGenerator::new(
            "https://example.test/prefix",
            vec![NamedRouteDefinition {
                name: "article.show".into(),
                path: "/article/{article}".into(),
                domain: None,
            }],
        )?;
        urls.set_key_resolver(|| Ok(UrlSigningKeys::new(b"raw-key".to_vec(), Vec::new())));
        let signed = urls.signed_route(
            "article.show",
            &BTreeMap::from([("article".into(), "42".into())]),
            true,
        )?;
        let external = signed
            .strip_prefix("https://example.test")
            .ok_or_else(|| Error::custom("signed_oracle_invalid", "external URI 缺失"))?;
        let internal = external
            .strip_prefix("/prefix")
            .ok_or_else(|| Error::custom("signed_oracle_invalid", "internal URI 缺失"))?;
        let request = Request::builder()
            .uri(internal)
            .header("host", "example.test")
            .header("x-forwarded-prefix", "/prefix")
            .body(Body::empty())
            .map_err(Error::internal)?;
        let (mut parts, _) = request.into_parts();
        parts.extensions.insert(RequestSecurity::new(true));
        let without_original = RequestInfo::from_request_parts(&mut parts, &()).await?;
        assert!(!urls.has_valid_signature(&without_original, true, &[])?);
        parts
            .extensions
            .insert(OriginalUri(external.parse().map_err(Error::internal)?));
        let trusted_original = RequestInfo::from_request_parts(&mut parts, &()).await?;
        assert!(urls.has_valid_signature(&trusted_original, true, &[])?);
        Ok(())
    }
}
