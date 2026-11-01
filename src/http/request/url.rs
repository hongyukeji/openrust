//! 请求地址快照与 Laravel 星号匹配。

use super::{Error, RequestInfo, Result, StatusCode, url_query};

impl RequestInfo {
    /// 从实际连接或可信代理快照返回协议；绝对 URI 不声明 TLS。
    pub fn scheme(&self) -> Result<&'static str> {
        self.secure
            .as_ref()
            .map(|secure| if *secure { "https" } else { "http" })
            .map_err(|error| match error.as_ref() {
                Error::Config(crate::config::ConfigError::Invalid(message)) => {
                    Error::Config(crate::config::ConfigError::Invalid(message.clone()))
                }
                Error::BadRequest(message) => Error::BadRequest(message.clone()),
                _ => Error::internal(error.clone()),
            })
    }

    /// 返回显式端口，缺失时采用实际协议的默认端口。
    pub fn port(&self) -> Result<i64> {
        let (_, direct_port) =
            crate::http::authority::authority_with_port(&self.headers, &self.uri)?;
        let proxy = self.proxy_context()?;
        if let Some(port) = proxy.port()? {
            return Ok(port);
        }
        let proxy_host = proxy.host()?;
        let port = if let Some((_, port)) = proxy_host {
            port
        } else {
            direct_port.map(i64::from)
        };
        if let Some(port) = port {
            return Ok(port);
        }
        Ok(if self.scheme()? == "https" { 443 } else { 80 })
    }

    /// 返回严格校验的主机与非默认端口；IPv6 保留方括号。
    pub fn http_host(&self) -> Result<String> {
        let host = self.host()?;
        let port = self.port()?;
        let default = if self.scheme()? == "https" { 443 } else { 80 };
        Ok(match port {
            port if port != default => format!("{host}:{port}"),
            _ => host,
        })
    }

    /// 返回协议、主机和可信代理前缀；路径信息仍是应用内层路径。
    pub fn root(&self) -> Result<String> {
        Ok(format!(
            "{}://{}{}",
            self.scheme()?,
            self.http_host()?,
            self.base_url()?
        ))
    }

    /// 返回可信代理声明的应用基础路径；没有可信前缀时为空串。
    pub fn base_url(&self) -> Result<String> {
        self.proxy_context()?.prefix()
    }

    /// 返回不含查询串且去掉尾斜线的请求 URL。
    pub fn url(&self) -> Result<String> {
        bounded_url(format!(
            "{}{}",
            self.root()?,
            self.uri.path().trim_end_matches('/')
        ))
    }

    /// 从原始 URI 查询串规范化完整 URL，不读取可变输入 bag。
    pub fn full_url(&self) -> Result<String> {
        self.url_with_query_string(&url_query::canonical(self.uri.query())?)
    }

    pub(super) fn query_separator(&self) -> &'static str {
        if self.uri.path() == "/" { "/?" } else { "?" }
    }

    pub(super) fn url_with_query_string(&self, query: &str) -> Result<String> {
        let url = self.url()?;
        bounded_url(if query.is_empty() {
            url
        } else {
            format!("{url}{}{query}", self.query_separator())
        })
    }

    /// 百分号解码路径；加号保留字面，非法 UTF-8 返回 400。
    pub fn decoded_path(&self) -> Result<String> {
        let bytes = self.path().as_bytes();
        let mut output = Vec::with_capacity(bytes.len());
        let mut position = 0;
        while position < bytes.len() {
            if bytes[position] == b'%'
                && position + 2 < bytes.len()
                && let (Some(high), Some(low)) =
                    (hex(bytes[position + 1]), hex(bytes[position + 2]))
            {
                output.push(high * 16 + low);
                position += 3;
            } else {
                output.push(bytes[position]);
                position += 1;
            }
        }
        String::from_utf8(output).map_err(|_| {
            Error::custom_with_status(
                StatusCode::BAD_REQUEST,
                "http_request_path_encoding_invalid",
                "request path must decode to valid UTF-8",
            )
        })
    }

    /// 解码后按斜线拆分，只移除空段。
    pub fn segments(&self) -> Result<Vec<String>> {
        Ok(self
            .decoded_path()?
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// 读取从 1 开始的路径段；0 和越界返回 None。
    pub fn segment(&self, index: usize) -> Result<Option<String>> {
        let segments = self.segments()?;
        Ok(index
            .checked_sub(1)
            .and_then(|index| segments.get(index))
            .cloned())
    }

    /// 以区分大小写的 Laravel 星号模式匹配解码路径。
    pub fn is(&self, patterns: &[&str]) -> Result<bool> {
        matches_any(&self.decoded_path()?, patterns)
    }

    /// 以区分大小写的 Laravel 星号模式匹配规范完整 URL。
    pub fn full_url_is(&self, patterns: &[&str]) -> Result<bool> {
        matches_any(&self.full_url()?, patterns)
    }

    /// 任意首个预取头完整值等于 prefetch 时为真，不接受参数或列表。
    pub fn prefetch(&self) -> Result<bool> {
        for name in ["x-moz", "purpose", "sec-purpose"] {
            if self
                .header(name)?
                .is_some_and(|value| value.eq_ignore_ascii_case("prefetch"))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// 分段查找星号之间的字面串，不使用可能指数回溯的正则。
fn matches_any(value: &str, patterns: &[&str]) -> Result<bool> {
    if patterns.len() > 128
        || patterns.iter().map(|pattern| pattern.len()).sum::<usize>() > 16 * 1024
    {
        return Err(Error::custom_with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "http_request_url_patterns_too_large",
            "request URL patterns exceed resource limits",
        ));
    }
    Ok(patterns.iter().any(|pattern| wildcard(value, pattern)))
}

fn wildcard(mut value: &str, pattern: &str) -> bool {
    let mut pieces = pattern.split('*').peekable();
    let Some(first) = pieces.next() else {
        return value.is_empty();
    };
    if pieces.peek().is_none() {
        return value == first;
    }
    let Some(rest) = value.strip_prefix(first) else {
        return false;
    };
    value = rest;
    while let Some(piece) = pieces.next() {
        if pieces.peek().is_none() {
            return value.ends_with(piece);
        }
        if let Some(position) = value.find(piece) {
            value = &value[position + piece.len()..];
        } else {
            return false;
        }
    }
    true
}

pub(super) fn bounded_url(url: String) -> Result<String> {
    if url.len() > 128 * 1024 {
        return Err(Error::custom_with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "http_request_url_too_large",
            "request URL exceeds resource limit",
        ));
    }
    Ok(url)
}

#[cfg(test)]
// 测试构造或严格行为断言失败时立即中止本用例，生产代码不允许 expect。
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use axum::extract::FromRequestParts;
    use axum::extract::connect_info::ConnectInfo;

    async fn info(uri: &str, host: &str, secure: bool) -> RequestInfo {
        let request = axum::http::Request::builder()
            .uri(uri)
            .header("host", host)
            .body(());
        let Ok(request) = request else {
            unreachable!("static request fixture");
        };
        let (mut parts, ()) = request.into_parts();
        parts
            .extensions
            .insert(crate::http::transport::RequestSecurity::new(secure));
        RequestInfo::from_request_parts(&mut parts, &())
            .await
            .expect("request snapshot")
    }

    #[tokio::test]
    async fn invalid_trusted_host_keeps_port_scheme_and_prefix_independent() {
        let request = axum::http::Request::builder()
            .uri("/path")
            .header("host", "internal.test")
            .header("x-forwarded-host", "bad/host")
            .header("x-forwarded-prefix", "/edge")
            .body(())
            .expect("static request fixture");
        let (mut parts, ()) = request.into_parts();
        parts.extensions.insert(ConnectInfo(
            "10.0.0.1:8080"
                .parse::<std::net::SocketAddr>()
                .expect("static socket"),
        ));
        let config = crate::config::ServerConfig {
            trusted_proxy_ranges: Some(vec!["REMOTE_ADDR".into()]),
            ..crate::config::ServerConfig::default()
        };
        let snapshot = crate::http::proxy::ProxySnapshot::capture(
            &parts.headers,
            &parts.uri,
            &parts.extensions,
            Some(&config),
        );
        parts.extensions.insert(snapshot);
        let info = RequestInfo::from_request_parts(&mut parts, &())
            .await
            .expect("request snapshot");
        assert_eq!(
            info.host().expect_err("bad host must fail").status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(info.port().expect("independent port"), 80);
        assert_eq!(info.scheme().expect("independent scheme"), "http");
        assert_eq!(info.base_url().expect("independent prefix"), "/edge");
    }

    #[tokio::test]
    async fn explicit_port_does_not_evaluate_conflicting_scheme() {
        let request = axum::http::Request::builder()
            .uri("/path")
            .header("host", "internal.test:8080")
            .header("x-forwarded-proto", "https")
            .header("forwarded", "proto=http")
            .body(())
            .expect("static request fixture");
        let (mut parts, ()) = request.into_parts();
        parts.extensions.insert(ConnectInfo(
            "10.0.0.1:8080"
                .parse::<std::net::SocketAddr>()
                .expect("static socket"),
        ));
        let config = crate::config::ServerConfig {
            trusted_proxy_ranges: Some(vec!["REMOTE_ADDR".into()]),
            trusted_proxy_headers: crate::http::TrustedProxyHeaders::PROTO
                | crate::http::TrustedProxyHeaders::FORWARDED,
            ..crate::config::ServerConfig::default()
        };
        parts
            .extensions
            .insert(crate::http::proxy::ProxySnapshot::capture(
                &parts.headers,
                &parts.uri,
                &parts.extensions,
                Some(&config),
            ));
        let info = RequestInfo::from_request_parts(&mut parts, &())
            .await
            .expect("request snapshot");
        assert!(info.scheme().is_err());
        assert_eq!(info.port().expect("explicit port"), 8080);
    }

    #[tokio::test]
    async fn addresses_keep_ports_and_original_encoded_path() {
        let info = info("/a%2Fb/+/%E4%B8%AD/?b=2&a=1", "Example.COM:443", true).await;
        assert_eq!(
            info.root().expect("测试构造或成功行为必须有效"),
            "https://example.com"
        );
        assert_eq!(
            info.url().expect("测试构造或成功行为必须有效"),
            "https://example.com/a%2Fb/+/%E4%B8%AD"
        );
        assert_eq!(
            info.full_url().expect("测试构造或成功行为必须有效"),
            "https://example.com/a%2Fb/+/%E4%B8%AD?a=1&b=2"
        );
        assert_eq!(
            info.segments().expect("测试构造或成功行为必须有效"),
            ["a", "b", "+", "中"]
        );
        assert_eq!(info.segment(0).expect("测试构造或成功行为必须有效"), None);
        assert_eq!(
            info.segment(2)
                .expect("测试构造或成功行为必须有效")
                .as_deref(),
            Some("b")
        );
        assert!(info.is(&["a/*/中"]).expect("测试构造或成功行为必须有效"));
        assert!(!info.is(&["a/?/*"]).expect("测试构造或成功行为必须有效"));
        let root = self::info("/?z=1", "[::1]:8080", false).await;
        assert_eq!(
            root.full_url().expect("测试构造或成功行为必须有效"),
            "http://[::1]:8080/?z=1"
        );
    }

    #[tokio::test]
    async fn absolute_https_does_not_declare_secure_transport() {
        let info = info("https://example.com:443/", "example.com:443", false).await;
        assert_eq!(info.scheme().expect("测试构造或成功行为必须有效"), "http");
        assert_eq!(
            info.http_host().expect("测试构造或成功行为必须有效"),
            "example.com:443"
        );
        let invalid = self::info("/%FF", "example.com", false).await;
        assert_eq!(
            invalid.decoded_path().expect_err("该负例必须拒绝").status(),
            StatusCode::BAD_REQUEST
        );
        let conflict = self::info("http://other.test/", "example.com", false).await;
        assert_eq!(
            conflict.root().expect_err("该负例必须拒绝").status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn wildcard_treats_only_stars_as_special_and_enforces_suffix() {
        assert!(wildcard("abc/def", "a*/d*f"));
        assert!(!wildcard("abc/def", "a*de"));
        assert!(!wildcard("abc", "a?c"));
        assert!(wildcard("abc", "**"));
        assert!(!wildcard("a", "a*a"));
    }
}

#[cfg(test)]
// 测试构造或严格行为断言失败时立即中止本用例，生产代码不允许 expect。
#[allow(clippy::expect_used)]
mod extraction_tests {
    use super::*;
    use axum::extract::{FromRequest, FromRequestParts, OriginalUri};
    use serde_json::json;

    #[tokio::test]
    async fn original_uri_and_current_query_are_distinct_sources() {
        let request = axum::http::Request::builder()
            .uri("/mounted?old=1")
            .header("host", "example.test")
            .body(axum::body::Body::empty())
            .expect("测试构造或成功行为必须有效");
        let mut request = request;
        request.extensions_mut().insert(OriginalUri(
            "/original?z=2&a=1"
                .parse()
                .expect("测试构造或成功行为必须有效"),
        ));
        let mut request = super::super::input::Request::from_request(request, &())
            .await
            .expect("测试构造或成功行为必须有效");
        request
            .replace(json!({"current":"value"}))
            .expect("测试构造或成功行为必须有效");
        assert_eq!(
            request.full_url().expect("测试构造或成功行为必须有效"),
            "http://example.test/original?a=1&z=2"
        );
        assert_eq!(
            request
                .full_url_with_query(&json!({"added":true}))
                .expect("测试构造或成功行为必须有效"),
            "http://example.test/original?current=value&added=1"
        );
        assert_eq!(
            request
                .full_url_without_query(&["current"])
                .expect("测试构造或成功行为必须有效"),
            "http://example.test/original"
        );
        request
            .replace(json!({}))
            .expect("测试构造或成功行为必须有效");
        // 固定上游在当前 bag 为空时，明确使用 fullUrl().question.Arr::query。
        assert_eq!(
            request
                .full_url_with_query(&json!({"added":true}))
                .expect("测试构造或成功行为必须有效"),
            "http://example.test/original?a=1&z=2?added=1"
        );
        assert_eq!(
            request
                .full_url_with_query(&json!({}))
                .expect("测试构造或成功行为必须有效"),
            "http://example.test/original?a=1&z=2?"
        );
        request
            .replace(json!({"only":null}))
            .expect("测试构造或成功行为必须有效");
        assert_eq!(
            request
                .full_url_without_query(&[])
                .expect("测试构造或成功行为必须有效"),
            "http://example.test/original?"
        );
    }

    #[tokio::test]
    async fn cached_transport_configuration_error_keeps_diagnostics() {
        let request = axum::http::Request::builder()
            .uri("/")
            .header("host", "example.test")
            .body(())
            .expect("测试构造或成功行为必须有效");
        let (mut parts, ()) = request.into_parts();
        let mut info = RequestInfo::from_request_parts(&mut parts, &())
            .await
            .expect("测试构造或成功行为必须有效");
        // 直接注入生产 helper 可产生的失败快照，避免绕过启动验证装配无效应用。
        let source = Error::Config(crate::config::ConfigError::Invalid(
            "server.trusted_proxy_ranges contains invalid CIDR".to_owned(),
        ));
        let diagnostic = source.to_string();
        info.secure = Err(std::sync::Arc::new(source));
        assert_eq!(info.path(), "/");
        for error in [
            info.scheme().expect_err("该负例必须拒绝"),
            info.root().expect_err("该负例必须拒绝"),
            info.clone().url().expect_err("该负例必须拒绝"),
        ] {
            assert_eq!(error.code(), "configuration_error");
            assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(error.to_string(), diagnostic);
        }
    }

    #[tokio::test]
    async fn untrusted_proxy_and_prefetch_lists_do_not_change_request_identity() {
        let request = axum::http::Request::builder()
            .uri("/")
            .header("host", "example.test:80")
            .header("x-forwarded-proto", "https")
            .header("purpose", "prefetch, navigate")
            .header("sec-purpose", "prefetch;prerender")
            .body(())
            .expect("测试构造或成功行为必须有效");
        let (mut parts, ()) = request.into_parts();
        let info = RequestInfo::from_request_parts(&mut parts, &())
            .await
            .expect("测试构造或成功行为必须有效");
        assert_eq!(info.scheme().expect("测试构造或成功行为必须有效"), "http");
        assert_eq!(
            info.http_host().expect("测试构造或成功行为必须有效"),
            "example.test"
        );
        assert!(!info.prefetch().expect("测试构造或成功行为必须有效"));
        parts.headers.insert(
            "x-moz",
            "PREFETCH".parse().expect("测试构造或成功行为必须有效"),
        );
        let info = RequestInfo::from_request_parts(&mut parts, &())
            .await
            .expect("测试构造或成功行为必须有效");
        assert!(info.prefetch().expect("测试构造或成功行为必须有效"));
    }
}
