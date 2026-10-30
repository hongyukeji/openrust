//! 绑定当前请求 URL 与输入的 Laravel 风格重定向响应。

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

#[cfg(feature = "session")]
use crate::support::{ArrayKey, MessageBag, MessageInput, MessageProvider};
use crate::{Error, Result};

use super::{Request, RequestInfo};

/// 将已验证的当前请求转换为重定向目标。
pub struct Redirector<'r> {
    info: &'r RequestInfo,
    #[cfg(feature = "session")]
    request: Option<&'r Request>,
}

impl<'r> Redirector<'r> {
    /// 仅借用请求元信息；普通重定向不要求 Session 或请求正文。
    pub fn from_info(info: &'r RequestInfo) -> Self {
        Self {
            info,
            #[cfg(feature = "session")]
            request: None,
        }
    }

    /// 借用唯一规范化输入仓储，供手动 flash 使用。
    pub fn from_request(request: &'r Request) -> Self {
        Self {
            info: request.info(),
            #[cfg(feature = "session")]
            request: Some(request),
        }
    }

    /// 以默认 302 建立绝对目标响应。
    pub fn to(&self, target: &str) -> Result<RedirectResponse<'r>> {
        self.to_with(target, StatusCode::FOUND, HeaderMap::new())
    }

    /// 按固定上游允许的状态与自定义头建立响应。
    pub fn to_with(
        &self,
        target: &str,
        status: StatusCode,
        mut headers: HeaderMap,
    ) -> Result<RedirectResponse<'r>> {
        if !matches!(status.as_u16(), 201 | 301 | 302 | 303 | 307 | 308) {
            return Err(Error::custom("redirect_status_invalid", "重定向状态码无效"));
        }
        let location = absolute_target(self.info, target)?;
        let header_value = HeaderValue::from_str(&location)
            .map_err(|_| Error::custom("redirect_location_invalid", "Location 头值无效"))?;
        headers.insert(header::LOCATION, header_value);
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        if status != StatusCode::MOVED_PERMANENTLY && !headers.contains_key(header::CACHE_CONTROL) {
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache, private"),
            );
        }
        Ok(RedirectResponse {
            #[cfg(feature = "session")]
            info: self.info,
            #[cfg(feature = "session")]
            request: self.request,
            #[cfg(not(feature = "session"))]
            request_lifetime: std::marker::PhantomData,
            status,
            location,
            headers,
        })
    }

    /// 按 Referer、Session previous、fallback、请求根的顺序寻找目标。
    pub fn back(&self, fallback: Option<&str>) -> Result<RedirectResponse<'r>> {
        let referrer = self.info.header("referer")?;
        if let Some(value) = referrer.filter(|value| php_truthy(value)) {
            return self.to(value);
        }
        #[cfg(feature = "session")]
        if self.info.has_session()
            && let Some(value) = self.info.session()?.previous_url()?
        {
            if let Some(url) = value.as_str() {
                if php_truthy(url) {
                    return self.to(url);
                }
            } else if matches!(value, serde_json::Value::Bool(true))
                || value
                    .as_number()
                    .is_some_and(|number| number.as_f64() != Some(0.0))
            {
                return self.to(&crate::support::scalars::string(&value));
            } else if !php_false_json(&value) {
                return Err(Error::custom(
                    "redirect_previous_url_invalid",
                    "Session previous URL 类型无效",
                ));
            }
        }
        if let Some(value) = fallback.filter(|value| php_truthy(value)) {
            return self.to(value);
        }
        self.to("")
    }
}

/// 保留对当前请求的只读借用；最终转换为独立 HTTP 响应。
pub struct RedirectResponse<'r> {
    #[cfg(feature = "session")]
    info: &'r RequestInfo,
    #[cfg(feature = "session")]
    request: Option<&'r Request>,
    #[cfg(not(feature = "session"))]
    request_lifetime: std::marker::PhantomData<&'r RequestInfo>,
    status: StatusCode,
    location: String,
    headers: HeaderMap,
}

impl RedirectResponse<'_> {
    /// 已校验的绝对或上游接受的特殊目标。
    pub fn target_url(&self) -> &str {
        &self.location
    }

    /// 从同一真实 Request 在调用时读取规范化输入并 flash。
    #[cfg(feature = "session")]
    pub fn with_input(&mut self) -> Result<&mut Self> {
        let request = self.request.ok_or_else(|| {
            Error::custom(
                "redirect_request_missing",
                "需要完整 Request 才能 flash 输入",
            )
        })?;
        self.info.session()?.flash_input(request.all())?;
        Ok(self)
    }

    /// 仅 flash 指定输入字段。
    #[cfg(feature = "session")]
    pub fn only_input(&mut self, keys: &[&str]) -> Result<&mut Self> {
        let request = self.request.ok_or_else(|| {
            Error::custom(
                "redirect_request_missing",
                "需要完整 Request 才能 flash 输入",
            )
        })?;
        self.info.session()?.flash_input(request.only(keys)?)?;
        Ok(self)
    }

    /// flash 除指定字段外的输入。
    #[cfg(feature = "session")]
    pub fn except_input(&mut self, keys: &[&str]) -> Result<&mut Self> {
        let request = self.request.ok_or_else(|| {
            Error::custom(
                "redirect_request_missing",
                "需要完整 Request 才能 flash 输入",
            )
        })?;
        self.info.session()?.flash_input(request.except(keys)?)?;
        Ok(self)
    }

    /// 闪存公开消息来源，保持其他命名错误包不变。
    #[cfg(feature = "session")]
    pub fn with_errors(
        &mut self,
        source: RedirectErrors<'_>,
        name: impl Into<ArrayKey>,
    ) -> Result<&mut Self> {
        let session = self.info.session()?;
        match source {
            RedirectErrors::Provider(provider) => {
                session.flash_errors(name, provider.message_bag())?;
            }
            RedirectErrors::Text(text) => {
                let bag = MessageBag::try_from_messages([(
                    ArrayKey::Integer(0),
                    MessageInput::One(text.to_owned()),
                )])?;
                session.flash_errors(name, &bag)?;
            }
            RedirectErrors::Entries(entries) => {
                let bag = MessageBag::try_from_messages(entries.iter().cloned())?;
                session.flash_errors(name, &bag)?;
            }
        }
        Ok(self)
    }
}

impl IntoResponse for RedirectResponse<'_> {
    fn into_response(self) -> Response {
        let escaped = self
            .location
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#039;");
        let body = format!(
            "<!DOCTYPE html>\n<html>\n    <head>\n        <meta charset=\"UTF-8\" />\n        <meta http-equiv=\"refresh\" content=\"0;url='{escaped}'\" />\n\n        <title>Redirecting to {escaped}</title>\n    </head>\n    <body>\n        Redirecting to <a href=\"{escaped}\">{escaped}</a>.\n    </body>\n</html>"
        );
        let mut response = (self.status, body).into_response();
        *response.headers_mut() = self.headers;
        response
    }
}

/// `with_errors` 的三种公开来源。
#[cfg(feature = "session")]
#[derive(Clone, Copy)]
pub enum RedirectErrors<'a> {
    /// 保留公开 Provider 返回的真实 custom 包。
    Provider(&'a dyn MessageProvider),
    /// 单条字符串消息。
    Text(&'a str),
    /// 有序、保键的类型化字段消息。
    Entries(&'a [(ArrayKey, MessageInput)]),
}

fn php_truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
}

#[cfg(feature = "session")]
fn php_false_json(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(false) => true,
        serde_json::Value::Number(number) => number.as_f64() == Some(0.0),
        serde_json::Value::String(value) => !php_truthy(value),
        serde_json::Value::Array(values) => values.is_empty(),
        serde_json::Value::Object(values) => values.is_empty(),
        serde_json::Value::Bool(true) => false,
    }
}

fn absolute_target(info: &RequestInfo, target: &str) -> Result<String> {
    if target.is_empty() {
        return info.root();
    }
    if target.starts_with('#')
        || target.starts_with("//")
        || target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("tel:")
        || target.starts_with("sms:")
        || filter_valid_absolute_url(target)
    {
        return Ok(target.to_owned());
    }
    let root = info.root()?;
    let (path, query) = target
        .split_once('?')
        .map_or((target, None), |(path, query)| (path, Some(query)));
    let path = path.trim_matches('/');
    let mut result = if path.is_empty() {
        root
    } else {
        format!("{root}/{path}")
    };
    if let Some(query) = query {
        result.push('?');
        result.push_str(query);
    }
    Ok(result)
}

// Laravel UrlGenerator::isValidUrl 的 FILTER_VALIDATE_URL 分支要求完整 URL。
// `foo:bar` 虽可由 URL crate 解析，却不是固定 PHP filter 的有效目标。
fn filter_valid_absolute_url(target: &str) -> bool {
    if target.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return false;
    }
    if target.starts_with("file:") && target.len() > "file:".len() {
        return url::Url::parse(target).is_ok_and(|url| url.scheme() == "file");
    }
    let Some((scheme, authority_and_path)) = target.split_once("://") else {
        return false;
    };
    // URL parser 会把 `ftp:///a` 归一成带 host 的 URL；PHP filter 拒绝原始空 authority。
    let raw_authority = authority_and_path
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    if raw_authority.is_empty()
        || authority_and_path.starts_with('/')
        || !raw_authority.is_ascii()
        || !scheme
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        || !scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    {
        return false;
    }
    url::Url::parse(target).is_ok_and(|url| url.has_host())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    #[cfg(feature = "session")]
    use axum::extract::FromRequest;
    use axum::{
        body::{Body, to_bytes},
        extract::FromRequestParts,
        http::Request as HttpRequest,
    };

    use super::*;

    async fn info(referer: Option<&str>) -> Result<RequestInfo> {
        let mut request = HttpRequest::builder()
            .uri("/current")
            .header("host", "example.test");
        if let Some(referer) = referer {
            request = request.header("referer", referer);
        }
        let (mut parts, _) = request.body(Body::empty()).expect("request").into_parts();
        RequestInfo::from_request_parts(&mut parts, &()).await
    }

    #[tokio::test]
    async fn default_redirect_has_html_location_cache_and_owned_body() -> Result<()> {
        let info = info(None).await?;
        let response = Redirector::from_info(&info)
            .to("/next?name=a&b")?
            .into_response();
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response.headers()[header::LOCATION],
            "http://example.test/next?name=a&b"
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "no-cache, private"
        );
        let body = to_bytes(response.into_body(), 16 * 1024)
            .await
            .map_err(Error::internal)?;
        assert!(String::from_utf8_lossy(&body).contains("a&amp;b"));
        Ok(())
    }

    #[tokio::test]
    async fn previous_truthy_and_status_boundary() -> Result<()> {
        let info = info(Some("0")).await?;
        assert_eq!(
            Redirector::from_info(&info)
                .back(Some("/fallback"))?
                .target_url(),
            "http://example.test/fallback"
        );
        assert_eq!(
            Redirector::from_info(&info).back(None)?.target_url(),
            "http://example.test"
        );
        assert_eq!(
            Redirector::from_info(&info).to("")?.target_url(),
            "http://example.test"
        );
        for status in [201, 301, 302, 303, 307, 308] {
            let response = Redirector::from_info(&info)
                .to_with(
                    "/x",
                    StatusCode::from_u16(status).map_err(Error::internal)?,
                    HeaderMap::new(),
                )?
                .into_response();
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(
                response.headers().contains_key(header::CACHE_CONTROL),
                status != 301
            );
        }
        for status in [200, 304] {
            assert!(
                Redirector::from_info(&info)
                    .to_with(
                        "/x",
                        StatusCode::from_u16(status).map_err(Error::internal)?,
                        HeaderMap::new()
                    )
                    .is_err()
            );
        }
        assert!(
            Redirector::from_info(&info)
                .to("/invalid\r\nheader:bad")
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn filter_valid_absolute_urls_match_fixed_php_cases() -> Result<()> {
        let info = info(None).await?;
        let redirect = Redirector::from_info(&info);
        assert_eq!(
            redirect.to("ftp://example.test/file")?.target_url(),
            "ftp://example.test/file"
        );
        assert_eq!(
            redirect.to("ftps://example.test/file")?.target_url(),
            "ftps://example.test/file"
        );
        assert_eq!(
            redirect.to("custom://example.test/file")?.target_url(),
            "custom://example.test/file"
        );
        for absolute in [
            "HtTp://example.test/a",
            "FtP://example.test/a",
            "custom+v1://host.example.test/a",
            "file:///tmp/a",
            "file:/tmp/a",
            "file:tmp/a",
            "ftp://bad_host.test/a",
            "custom://bad_host/a",
            "ftp://-bad.test/a",
            "custom://host./a",
            "custom://[::1]/a",
            "https://example.test/a b",
        ] {
            assert_eq!(redirect.to(absolute)?.target_url(), absolute);
        }
        assert_eq!(
            redirect.to("relative/path")?.target_url(),
            "http://example.test/relative/path"
        );
        assert_eq!(
            redirect.to("foo:bar")?.target_url(),
            "http://example.test/foo:bar"
        );
        assert_eq!(
            redirect.to("ftp://")?.target_url(),
            "http://example.test/ftp:"
        );
        for (invalid, expected) in [
            ("ftp:///a", "http://example.test/ftp:///a"),
            ("FILE:///tmp/a", "http://example.test/FILE:///tmp/a"),
            (
                "custom://bücher.test/a",
                "http://example.test/custom://bücher.test/a",
            ),
            ("ftp://bad host/a", "http://example.test/ftp://bad host/a"),
            (
                "urn:isbn:0451450523",
                "http://example.test/urn:isbn:0451450523",
            ),
            (
                "data:text/plain,hello",
                "http://example.test/data:text/plain,hello",
            ),
            (
                "javascript:alert(1)",
                "http://example.test/javascript:alert(1)",
            ),
        ] {
            assert_eq!(redirect.to(invalid)?.target_url(), expected);
        }
        assert!(
            redirect
                .to("ftp://example.test/file\r\nX-Test: value")
                .is_err()
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn borrowed_request_flashes_input_and_named_errors() -> Result<()> {
        let session = crate::session::Session::new(
            None,
            std::sync::Arc::new(crate::session::MemorySessionStore::default()),
            None,
        )?;
        let mut incoming = HttpRequest::builder()
            .method("POST")
            .uri("/form")
            .header("host", "example.test")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"email":"a@example.test","password":"manual"}"#,
            ))
            .expect("request");
        incoming.extensions_mut().insert(session.clone());
        let request = Request::from_request(incoming, &()).await?;
        let mut redirect = Redirector::from_request(&request).to("/done")?;
        redirect
            .with_input()?
            .with_errors(RedirectErrors::Text("invalid"), "default")?;
        assert_eq!(
            session.get_old_input(Some("password"))?,
            Some(serde_json::json!("manual"))
        );
        assert_eq!(
            session
                .get("_old_input")?
                .and_then(|value| value.get("password").cloned()),
            Some(serde_json::json!("manual"))
        );
        assert_eq!(session.read_errors()?.count(), 1);
        assert_eq!(redirect.into_response().status(), StatusCode::FOUND);
        Ok(())
    }
}
