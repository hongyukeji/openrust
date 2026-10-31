//! 绑定当前请求 URL 与输入的 Laravel 风格重定向响应。

use std::collections::BTreeMap;

use serde_json::Value;

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

#[cfg(feature = "session")]
use crate::support::{ArrayKey, MessageBag, MessageInput, MessageProvider};
use crate::{Error, Result};

use super::{Request, RequestInfo, RouteAction};

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

    /// 按当前应用的路由名称生成目标，以默认 302 和空自定义头建立响应。
    ///
    /// 参数形态沿用 `RouteUrlGenerator` 的字符串映射；缺少应用路由生成器、
    /// 未知名称或缺少必需参数均沿原错误传播。普通命名重定向不要求 Session。
    pub fn route(
        &self,
        name: &str,
        parameters: &BTreeMap<String, String>,
    ) -> Result<RedirectResponse<'r>> {
        self.route_with(name, parameters, StatusCode::FOUND, HeaderMap::new())
    }

    /// 按名称、参数、状态与自定义头建立重定向，保留当前请求的闪存借用链。
    pub fn route_with(
        &self,
        name: &str,
        parameters: &BTreeMap<String, String>,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<RedirectResponse<'r>> {
        let target = self.info.route_urls()?.route(name, parameters)?;
        self.to_with(&target, status, headers)
    }

    /// 按实际登记的控制器动作生成目标，默认 302 和空自定义头，不要求 Session。
    ///
    /// 字符串、二元组及裸 invokable 身份沿当前请求的生成器解析；
    /// 动作或必需参数错误先传播，不退回命名路由或任意 URL。
    pub fn action(
        &self,
        action: impl Into<RouteAction>,
        parameters: &BTreeMap<String, String>,
    ) -> Result<RedirectResponse<'r>> {
        self.action_with(action, parameters, StatusCode::FOUND, HeaderMap::new())
    }

    /// 按动作、参数、状态与自定义头建立响应，保留原请求的输入和错误闪存借用。
    pub fn action_with(
        &self,
        action: impl Into<RouteAction>,
        parameters: &BTreeMap<String, String>,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<RedirectResponse<'r>> {
        let target = self.info.route_urls()?.action(action, parameters)?;
        self.to_with(&target, status, headers)
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
        headers: HeaderMap,
    ) -> Result<RedirectResponse<'r>> {
        self.to_with_scheme(target, status, headers, None)
    }

    /// 按当前请求路径刷新，以默认302及空自定义头建立响应，不保留query。
    pub fn refresh(&self) -> Result<RedirectResponse<'r>> {
        self.refresh_with(StatusCode::FOUND, HeaderMap::new())
    }

    /// 按当前请求的原编码路径刷新，保留显式状态和自定义头。
    pub fn refresh_with(
        &self,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<RedirectResponse<'r>> {
        self.to_with(self.info.path(), status, headers)
    }

    /// 以默认302及空自定义头直接使用目标，不生成绝对URL或改写协议。
    ///
    /// 相对及特殊目标保持原值；空目标、非法状态与不安全Location头仍报错。
    pub fn away(&self, target: &str) -> Result<RedirectResponse<'r>> {
        self.away_with(target, StatusCode::FOUND, HeaderMap::new())
    }

    /// 原样使用目标，保留显式状态、自定义头及当前请求的闪存借用链。
    pub fn away_with(
        &self,
        target: &str,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<RedirectResponse<'r>> {
        Self::create_response(
            target.to_owned(),
            status,
            headers,
            #[cfg(feature = "session")]
            self.info,
            #[cfg(feature = "session")]
            self.request,
        )
    }

    /// 显式选择相对目标的协议；合法绝对或特殊 URL 保留原值。
    ///
    /// `None` 沿当前请求协议，`Some(true/false)` 分别使用 HTTPS/HTTP。
    /// 该选项不修改请求快照、应用配置或命名路由生成器。
    pub fn to_with_scheme(
        &self,
        target: &str,
        status: StatusCode,
        headers: HeaderMap,
        secure: Option<bool>,
    ) -> Result<RedirectResponse<'r>> {
        let location = absolute_target(self.info, target, secure)?;
        Self::create_response(
            location,
            status,
            headers,
            #[cfg(feature = "session")]
            self.info,
            #[cfg(feature = "session")]
            self.request,
        )
    }

    // 与上游createRedirect一样直接消费目标，back不再经过URL生成器二次绝对化。
    fn create_response(
        location: String,
        status: StatusCode,
        mut headers: HeaderMap,
        #[cfg(feature = "session")] info: &'r RequestInfo,
        #[cfg(feature = "session")] request: Option<&'r Request>,
    ) -> Result<RedirectResponse<'r>> {
        // 对齐Symfony构造顺序：HTTP范围、空目标、重定向状态，再建立安全头值。
        if status.as_u16() >= 600 {
            return Err(Error::custom("redirect_status_invalid", "重定向状态码无效"));
        }
        if location.is_empty() {
            return Err(Error::custom(
                "redirect_location_invalid",
                "重定向目标不能为空",
            ));
        }
        if !matches!(status.as_u16(), 201 | 301 | 302 | 303 | 307 | 308) {
            return Err(Error::custom("redirect_status_invalid", "重定向状态码无效"));
        }
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
            info,
            #[cfg(feature = "session")]
            request,
            #[cfg(not(feature = "session"))]
            request_lifetime: std::marker::PhantomData,
            status,
            location,
            headers,
        })
    }

    /// 对相对目标选择 HTTPS，以默认302及空自定义头建立响应。
    pub fn secure(&self, target: &str) -> Result<RedirectResponse<'r>> {
        self.secure_with(target, StatusCode::FOUND, HeaderMap::new())
    }

    /// 对相对目标选择 HTTPS，保留显式状态与自定义头。
    pub fn secure_with(
        &self,
        target: &str,
        status: StatusCode,
        headers: HeaderMap,
    ) -> Result<RedirectResponse<'r>> {
        self.to_with_scheme(target, status, headers, Some(true))
    }

    /// 按 Referer、Session previous、fallback、请求根的顺序寻找目标。
    ///
    /// Session中的相对previous保持相对目标；不再次通过`to`绝对化。
    pub fn back(&self, fallback: Option<&str>) -> Result<RedirectResponse<'r>> {
        let previous = self.previous_value(fallback)?;
        if previous.is_array() || previous.is_object() {
            return Err(Error::custom(
                "redirect_previous_url_invalid",
                "Session previous URL 类型无效",
            ));
        }
        Self::create_response(
            crate::support::scalars::string(&previous),
            StatusCode::FOUND,
            HeaderMap::new(),
            #[cfg(feature = "session")]
            self.info,
            #[cfg(feature = "session")]
            self.request,
        )
    }

    // previous只有Referer/fallback经过to；Session真值保持类型，供guest原样保存。
    fn previous_value(&self, fallback: Option<&str>) -> Result<Value> {
        let referrer = self.info.header("referer")?;
        if let Some(value) = referrer.filter(|value| php_truthy(value)) {
            return absolute_target(self.info, value, None).map(Value::String);
        }
        #[cfg(feature = "session")]
        if self.info.has_session()
            && let Some(value) = self.info.session()?.previous_url()?
            && !php_false_json(&value)
        {
            return Ok(value);
        }
        absolute_target(
            self.info,
            fallback.filter(|value| php_truthy(value)).unwrap_or(""),
            None,
        )
        .map(Value::String)
    }

    /// 保存当前可返回地址，再以默认302跳转；意图地址是普通Session属性。
    #[cfg(feature = "session")]
    pub fn guest(&self, target: &str) -> Result<RedirectResponse<'r>> {
        self.guest_with(target, StatusCode::FOUND, HeaderMap::new(), None)
    }

    /// 先保存意图地址，再校验目标响应；后续响应失败不撤销已完成的保存。
    #[cfg(feature = "session")]
    pub fn guest_with(
        &self,
        target: &str,
        status: StatusCode,
        headers: HeaderMap,
        secure: Option<bool>,
    ) -> Result<RedirectResponse<'r>> {
        let intended = if self.info.is_method("GET")
            && self.info.route()?.is_some()
            && !self.info.expects_json()?
        {
            Value::String(self.info.full_url()?)
        } else {
            self.previous_value(None)?
        };
        if !php_false_json(&intended) {
            self.set_intended_url(intended)?;
        }
        self.to_with_scheme(target, status, headers, secure)
    }

    /// 消费一次意图地址；只有属性缺失才回退到 `/`。
    #[cfg(feature = "session")]
    pub fn intended(&self) -> Result<RedirectResponse<'r>> {
        self.intended_or("/")
    }

    /// 消费一次意图地址，只有缺失才使用给定JSON默认值。
    #[cfg(feature = "session")]
    pub fn intended_or(
        &self,
        default: impl Into<serde_json::Value>,
    ) -> Result<RedirectResponse<'r>> {
        self.intended_with(default, StatusCode::FOUND, HeaderMap::new(), None)
    }

    /// 先消费再转换目标并建立响应；转换或响应失败也不恢复已消费的值。
    ///
    /// null/false变为空目标，true变为`1`，数值沿现有PHP标量转换；
    /// 数组/对象返回错误，区别于属性缺失时才使用的默认值。
    #[cfg(feature = "session")]
    pub fn intended_with(
        &self,
        default: impl Into<serde_json::Value>,
        status: StatusCode,
        headers: HeaderMap,
        secure: Option<bool>,
    ) -> Result<RedirectResponse<'r>> {
        let value = self
            .info
            .session()?
            .pull_or("url.intended", default.into())?;
        if value.is_array() || value.is_object() {
            return Err(Error::custom(
                "redirect_intended_url_invalid",
                "Session intended URL 必须是标量或null",
            ));
        }
        self.to_with_scheme(
            &crate::support::scalars::string(&value),
            status,
            headers,
            secure,
        )
    }

    /// 原样读取意图地址；缺失与显式null保持可区分。
    #[cfg(feature = "session")]
    pub fn get_intended_url(&self) -> Result<Option<serde_json::Value>> {
        self.info.session()?.get("url.intended")
    }

    /// 存储普通Session属性，不提前验证URL；返回自身供继续组合。
    #[cfg(feature = "session")]
    pub fn set_intended_url(&self, value: impl Into<serde_json::Value>) -> Result<&Self> {
        self.info.session()?.put("url.intended", value.into())?;
        Ok(self)
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

fn absolute_target(info: &RequestInfo, target: &str, secure: Option<bool>) -> Result<String> {
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
    let root = match secure {
        None => info.root()?,
        Some(secure) => format!(
            "{}://{}",
            if secure { "https" } else { "http" },
            info.http_host()?
        ),
    };
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

    fn named_urls(path: &str) -> Result<super::super::RouteUrlGenerator> {
        super::super::RouteUrlGenerator::new(
            "https://configured.example.test/base",
            vec![super::super::routes::NamedRouteDefinition {
                name: "done".into(),
                path: path.into(),
                domain: None,
            }],
        )
    }

    #[tokio::test]
    async fn refresh_uses_original_encoded_path_without_query_and_keeps_options() -> Result<()> {
        // 只改path来源，不能误用full_url保留query或decoded_path解码路径。
        for (uri, target) in [
            ("/?token=drop", "http://example.test"),
            (
                "/draft/a%2Fb/?token=drop",
                "http://example.test/draft/a%2Fb",
            ),
        ] {
            let (mut parts, _) = HttpRequest::get(uri)
                .header("host", "example.test")
                .body(Body::empty())
                .expect("request")
                .into_parts();
            let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
            let redirect = Redirector::from_info(&info);
            assert_eq!(redirect.refresh()?.target_url(), target);
            let mut headers = HeaderMap::new();
            headers.insert("x-refresh-proof", HeaderValue::from_static("kept"));
            let response = redirect
                .refresh_with(StatusCode::SEE_OTHER, headers)?
                .into_response();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[header::LOCATION], target);
            assert_eq!(response.headers()["x-refresh-proof"], "kept");
            assert_eq!(info.uri().to_string(), uri);
        }
        Ok(())
    }

    #[tokio::test]
    async fn away_preserves_raw_targets_and_shared_html_status_header_rules() -> Result<()> {
        // away必须跳过URL生成，尤其不能把相对目标或任意特殊scheme改成当前root。
        let info = info(None).await?;
        let redirect = Redirector::from_info(&info);
        for target in [
            "relative/path",
            "https://outside.test/x",
            "http://outside.test/x",
            "//cdn.test/x",
            "#fragment",
            "mailto:a@example.test",
            "foo:bar",
        ] {
            let response = redirect.away(target)?.into_response();
            assert_eq!(response.status(), StatusCode::FOUND);
            assert_eq!(response.headers()[header::LOCATION], target);
        }
        let mut headers = HeaderMap::new();
        headers.insert("x-away-proof", HeaderValue::from_static("kept"));
        headers.insert(header::LOCATION, HeaderValue::from_static("ignored"));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("ignored"));
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=60"),
        );
        let response = redirect
            .away_with(
                "relative?tag=<b>&quote='\"",
                StatusCode::MOVED_PERMANENTLY,
                headers,
            )?
            .into_response();
        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers()[header::LOCATION],
            "relative?tag=<b>&quote='\""
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, max-age=60"
        );
        assert_eq!(response.headers()["x-away-proof"], "kept");
        let body = to_bytes(response.into_body(), 16384)
            .await
            .map_err(Error::internal)?;
        assert!(
            String::from_utf8_lossy(&body)
                .contains("relative?tag=&lt;b&gt;&amp;quote=&#039;&quot;")
        );
        assert!(
            !redirect
                .away_with("relative", StatusCode::MOVED_PERMANENTLY, HeaderMap::new())?
                .into_response()
                .headers()
                .contains_key(header::CACHE_CONTROL)
        );
        Ok(())
    }

    #[tokio::test]
    async fn shared_redirect_constructor_rejects_empty_status_and_unsafe_location_in_order()
    -> Result<()> {
        // 对应Symfony parent状态范围→空目标→redirect状态；不把away当安全校验逃生口。
        let info = info(None).await?;
        let redirect = Redirector::from_info(&info);
        for (target, status, code) in [
            ("", 302, "redirect_location_invalid"),
            ("", 200, "redirect_location_invalid"),
            ("", 999, "redirect_status_invalid"),
            ("relative", 200, "redirect_status_invalid"),
            ("relative", 304, "redirect_status_invalid"),
            ("/bad\r\nX-Test: injected", 302, "redirect_location_invalid"),
            ("/bad\r\nX-Test: injected", 200, "redirect_status_invalid"),
        ] {
            assert_eq!(
                redirect
                    .away_with(
                        target,
                        StatusCode::from_u16(status).map_err(Error::internal)?,
                        HeaderMap::new()
                    )
                    .err()
                    .expect("invalid redirect")
                    .code(),
                code
            );
        }
        assert_eq!(redirect.to("")?.target_url(), "http://example.test");
        let (mut invalid_parts, _) = HttpRequest::get("/")
            .header("host", "bad host")
            .body(Body::empty())
            .expect("request")
            .into_parts();
        let invalid_info = RequestInfo::from_request_parts(&mut invalid_parts, &()).await?;
        let invalid_redirect = Redirector::from_info(&invalid_info);
        // generator先于response：同时无效时先传播URL来源错误，away则不读取来源。
        assert_eq!(
            invalid_redirect
                .to_with("/relative", StatusCode::OK, HeaderMap::new())
                .err()
                .expect("generation before response")
                .code(),
            "http_domain_authority_invalid"
        );
        assert_eq!(invalid_redirect.away("relative")?.target_url(), "relative");
        assert_eq!(
            redirect
                .refresh_with(StatusCode::OK, HeaderMap::new())
                .err()
                .expect("bad status")
                .code(),
            "redirect_status_invalid"
        );
        for status in [201, 301, 302, 303, 307, 308] {
            assert_eq!(
                redirect
                    .away_with(
                        "relative",
                        StatusCode::from_u16(status).map_err(Error::internal)?,
                        HeaderMap::new()
                    )?
                    .into_response()
                    .status()
                    .as_u16(),
                status
            );
        }
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn refresh_and_away_keep_original_request_input_and_errors_borrows() -> Result<()> {
        // 两个新入口必须保留from_request上下文，不以仅Location正确掩盖闪存借用丢失。
        for refresh in [true, false] {
            let session = intent_session()?;
            let mut incoming = HttpRequest::post("/draft?token=drop")
                .header("host", "example.test")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"draft"}"#))
                .expect("request");
            incoming.extensions_mut().insert(session.clone());
            let request = Request::from_request(incoming, &()).await?;
            let redirector = Redirector::from_request(&request);
            let mut response = if refresh {
                redirector.refresh()?
            } else {
                redirector.away("/outside")?
            };
            response
                .with_input()?
                .with_errors(RedirectErrors::Text("invalid"), "default")?;
            assert_eq!(
                session.get_old_input(Some("title"))?,
                Some(serde_json::json!("draft"))
            );
            assert_eq!(
                session.read_errors()?.default_first(None, None)?,
                Some("invalid".into())
            );
            assert_eq!(
                response.target_url(),
                if refresh {
                    "http://example.test/draft"
                } else {
                    "/outside"
                }
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn explicit_scheme_changes_only_relative_redirects_and_keeps_headers() -> Result<()> {
        let info = info(None).await?;
        let redirect = Redirector::from_info(&info);
        for target in ["", "/done?x=1", "relative/path"] {
            let expected = redirect.to(target)?.target_url().replace("http:", "https:");
            assert_eq!(redirect.secure(target)?.target_url(), expected);
            assert_eq!(
                redirect
                    .to_with_scheme(target, StatusCode::FOUND, HeaderMap::new(), Some(false))?
                    .target_url(),
                redirect.to(target)?.target_url()
            );
        }
        for target in [
            "http://external.test/x",
            "https://external.test/x",
            "//cdn.test/x",
            "#anchor",
            "mailto:a@example.test",
            "ftp://files.test/x",
        ] {
            assert_eq!(redirect.secure(target)?.target_url(), target);
        }
        let mut headers = HeaderMap::new();
        headers.insert("x-scheme-proof", HeaderValue::from_static("kept"));
        let response = redirect
            .secure_with("/done", StatusCode::TEMPORARY_REDIRECT, headers)?
            .into_response();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.headers()["x-scheme-proof"], "kept");
        assert_eq!(
            response.headers()[header::LOCATION],
            "https://example.test/done"
        );
        let (mut parts, _) = HttpRequest::get("/")
            .header("host", "example.test:443")
            .body(Body::empty())
            .expect("request")
            .into_parts();
        parts
            .extensions
            .insert(super::super::transport::RequestSecurity::new(true));
        let tls = RequestInfo::from_request_parts(&mut parts, &()).await?;
        let tls_redirect = Redirector::from_info(&tls);
        assert_eq!(
            tls_redirect.to("/done")?.target_url(),
            "https://example.test/done"
        );
        assert_eq!(
            tls_redirect
                .to_with_scheme("/done", StatusCode::FOUND, HeaderMap::new(), Some(false))?
                .target_url(),
            "http://example.test/done"
        );
        assert_eq!(tls.scheme()?, "https");
        assert_eq!(info.scheme()?, "http");
        assert_eq!(
            redirect.to("/done")?.target_url(),
            "http://example.test/done"
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    fn intent_session() -> Result<crate::session::Session> {
        crate::session::Session::new(
            None,
            std::sync::Arc::new(crate::session::MemorySessionStore::default()),
            None,
        )
    }

    #[cfg(feature = "session")]
    async fn intent_info(
        session: &crate::session::Session,
        method: &str,
        matched: bool,
        headers: &[(&str, &str)],
    ) -> Result<RequestInfo> {
        let mut incoming = HttpRequest::builder()
            .method(method)
            .uri("/protected?b=two&a=one")
            .header("host", "example.test");
        for (name, value) in headers {
            incoming = incoming.header(*name, *value);
        }
        let (mut parts, _) = incoming.body(Body::empty()).expect("request").into_parts();
        parts.extensions.insert(session.clone());
        let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
        if matched {
            info.set_route_resolver(std::sync::Arc::new(|| {
                Ok(Some(super::super::Route::new(
                    None,
                    "protected",
                    None,
                    vec!["GET".into()],
                    vec![],
                )))
            }))?;
        }
        Ok(info)
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn guest_selects_real_route_html_get_and_previous_for_other_requests() -> Result<()> {
        // 匿名真实Route也记录full URL；HEAD、JSON、AJAX及无Route必须走previous。
        type GuestCase = (
            &'static str,
            bool,
            &'static [(&'static str, &'static str)],
            bool,
        );
        let cases: &[GuestCase] = &[
            ("GET", true, &[], true),
            ("GET", false, &[], false),
            ("HEAD", true, &[], false),
            ("POST", true, &[], false),
            ("GET", true, &[("accept", "application/json")], false),
            (
                "GET",
                true,
                &[("x-requested-with", "XMLHttpRequest")],
                false,
            ),
            (
                "GET",
                true,
                &[("x-requested-with", "XMLHttpRequest"), ("x-pjax", "true")],
                true,
            ),
            (
                "GET",
                true,
                &[
                    ("x-requested-with", "XMLHttpRequest"),
                    ("accept", "text/html"),
                ],
                true,
            ),
        ];
        for (method, matched, headers, use_current) in cases {
            let session = intent_session()?;
            session.set_previous_url("/previous")?;
            let info = intent_info(&session, method, *matched, headers).await?;
            let redirect = Redirector::from_info(&info);
            assert_eq!(
                redirect.guest("/login")?.target_url(),
                "http://example.test/login"
            );
            let expected = if *use_current {
                info.full_url()?
            } else {
                "/previous".into()
            };
            assert_eq!(
                redirect.get_intended_url()?,
                Some(serde_json::json!(expected)),
                "{method} {headers:?}"
            );
        }
        let session = intent_session()?;
        session.set_previous_url("/session-previous")?;
        let info = intent_info(
            &session,
            "POST",
            false,
            &[("referer", "https://outside.test/return")],
        )
        .await?;
        let redirect = Redirector::from_info(&info);
        redirect.guest("/login")?;
        assert_eq!(
            redirect.get_intended_url()?,
            Some(serde_json::json!("https://outside.test/return"))
        );
        assert_eq!(
            redirect.back(None)?.target_url(),
            "https://outside.test/return"
        );
        let empty = intent_session()?;
        let info = intent_info(&empty, "HEAD", false, &[("referer", "0")]).await?;
        Redirector::from_info(&info).guest("/login")?;
        assert_eq!(
            empty.get("url.intended")?,
            Some(serde_json::json!("http://example.test"))
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn guest_preserves_raw_previous_until_intended_uses_new_host_and_scheme() -> Result<()> {
        for (value, suffix) in [
            (serde_json::json!("/draft"), Some("/draft")),
            (serde_json::json!(9), Some("/9")),
            (serde_json::json!(true), Some("/1")),
            (serde_json::json!(["draft"]), None),
            (serde_json::json!({"route":"/draft"}), None),
        ] {
            let session = intent_session()?;
            session.put("_previous.url", value.clone())?;
            let first = intent_info(&session, "POST", false, &[]).await?;
            let redirect = Redirector::from_info(&first);
            assert_eq!(
                redirect.guest("/login")?.target_url(),
                "http://example.test/login"
            );
            assert_eq!(redirect.get_intended_url()?, Some(value.clone()));
            // back直接建立响应：Session相对值/标量不先绝对化，数组在响应边界拒绝。
            if suffix.is_some() {
                let response = redirect.back(None)?.into_response();
                assert_eq!(
                    response.headers()[header::LOCATION]
                        .to_str()
                        .map_err(Error::internal)?,
                    crate::support::scalars::string(&value)
                );
            } else {
                assert_eq!(
                    redirect.back(None).err().expect("array response").code(),
                    "redirect_previous_url_invalid"
                );
            }
            let (mut parts, _) = HttpRequest::get("/complete")
                .header("host", "next.example.test:8443")
                .body(Body::empty())
                .expect("request")
                .into_parts();
            parts.extensions.insert(session.clone());
            let next = RequestInfo::from_request_parts(&mut parts, &()).await?;
            let next_redirect = Redirector::from_info(&next);
            let result = next_redirect.intended_with(
                "/fallback",
                StatusCode::FOUND,
                HeaderMap::new(),
                Some(true),
            );
            if let Some(suffix) = suffix {
                assert_eq!(
                    result?.target_url(),
                    format!("https://next.example.test:8443{suffix}")
                );
            } else {
                assert_eq!(
                    result.err().expect("array after pull").code(),
                    "redirect_intended_url_invalid"
                );
            }
            assert_eq!(next_redirect.get_intended_url()?, None);
            assert_eq!(
                next_redirect.intended_or("/fallback")?.target_url(),
                "http://next.example.test:8443/fallback"
            );
        }
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn previous_empty_php_arrays_and_false_scalars_use_root_fallback() -> Result<()> {
        // JSON对象在Session模型中承载PHP关联数组，空对象与空序列均为假值。
        for value in [
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::Value::Null,
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!("0"),
            serde_json::json!(""),
        ] {
            let session = intent_session()?;
            session.put("_previous.url", value)?;
            let info = intent_info(&session, "HEAD", true, &[]).await?;
            let redirect = Redirector::from_info(&info);
            redirect.guest("/login")?;
            assert_eq!(
                redirect.get_intended_url()?,
                Some(serde_json::json!("http://example.test"))
            );
            assert_eq!(redirect.back(None)?.target_url(), "http://example.test");
            assert_eq!(
                redirect.back(Some("/fallback"))?.target_url(),
                "http://example.test/fallback"
            );
        }
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn intended_preserves_raw_scalars_defaults_and_consumes_invalid_values() -> Result<()> {
        let session = intent_session()?;
        let info = intent_info(&session, "GET", false, &[]).await?;
        let redirect = Redirector::from_info(&info);
        assert_eq!(redirect.intended()?.target_url(), "http://example.test");
        assert_eq!(
            redirect.intended_or("/fallback")?.target_url(),
            "http://example.test/fallback"
        );
        for (value, suffix) in [
            (serde_json::Value::Null, ""),
            (serde_json::json!(false), ""),
            (serde_json::json!(0), "/0"),
            (serde_json::json!(""), ""),
            (serde_json::json!(true), "/1"),
            (serde_json::json!(1.25), "/1.25"),
            (
                serde_json::json!("https://external.test/return"),
                "external",
            ),
        ] {
            redirect.set_intended_url(value.clone())?;
            assert_eq!(redirect.get_intended_url()?, Some(value.clone()));
            let expected = if suffix == "external" {
                "https://external.test/return".into()
            } else {
                format!("http://example.test{suffix}")
            };
            assert_eq!(redirect.intended_or("/fallback")?.target_url(), expected);
            assert_eq!(redirect.get_intended_url()?, None);
            assert_eq!(redirect.intended_or(value)?.target_url(), expected);
        }
        for value in [serde_json::json!([]), serde_json::json!({"x":1})] {
            redirect.set_intended_url(value)?;
            assert_eq!(
                redirect.intended().err().expect("invalid scalar").code(),
                "redirect_intended_url_invalid"
            );
            assert_eq!(redirect.get_intended_url()?, None);
        }
        redirect.set_intended_url("/once")?;
        let mut headers = HeaderMap::new();
        headers.insert("x-intent-proof", HeaderValue::from_static("kept"));
        let response = redirect
            .intended_with("/fallback", StatusCode::SEE_OTHER, headers, Some(true))?
            .into_response();
        assert_eq!(
            response.headers()[header::LOCATION],
            "https://example.test/once"
        );
        assert_eq!(response.headers()["x-intent-proof"], "kept");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            redirect.intended_or("/fallback")?.target_url(),
            "http://example.test/fallback"
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn intended_order_keeps_writes_and_consumption_after_response_failure() -> Result<()> {
        let session = intent_session()?;
        let info = intent_info(&session, "GET", true, &[]).await?;
        let redirect = Redirector::from_info(&info);
        assert_eq!(
            redirect
                .guest_with("/login", StatusCode::OK, HeaderMap::new(), None)
                .err()
                .expect("bad status")
                .code(),
            "redirect_status_invalid"
        );
        assert_eq!(
            redirect.get_intended_url()?,
            Some(serde_json::json!(info.full_url()?))
        );
        assert_eq!(
            redirect
                .intended_with("/fallback", StatusCode::OK, HeaderMap::new(), None)
                .err()
                .expect("bad status")
                .code(),
            "redirect_status_invalid"
        );
        assert_eq!(redirect.get_intended_url()?, None);
        redirect.set_intended_url("/bad\r\nX-Test: bad")?;
        assert_eq!(
            redirect.intended().err().expect("bad header").code(),
            "redirect_location_invalid"
        );
        assert_eq!(redirect.get_intended_url()?, None);
        assert_eq!(
            redirect
                .guest("/bad\r\nX-Test: bad")
                .err()
                .expect("bad header")
                .code(),
            "redirect_location_invalid"
        );
        assert_eq!(
            redirect.get_intended_url()?,
            Some(serde_json::json!(info.full_url()?))
        );
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn intended_plain_attribute_survives_flash_aging_and_keeps_flash_chain() -> Result<()> {
        let store = std::sync::Arc::new(crate::session::MemorySessionStore::default());
        let session = crate::session::Session::new(None, store.clone(), None)?;
        session.start().await?;
        session.flash("notice", "once")?;
        let mut incoming = HttpRequest::post("/form")
            .header("host", "example.test")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"draft"}"#))
            .expect("request");
        incoming.extensions_mut().insert(session.clone());
        let request = Request::from_request(incoming, &()).await?;
        let redirector = Redirector::from_request(&request);
        redirector.set_intended_url("/protected")?;
        redirector
            .guest("/login")?
            .with_input()?
            .with_errors(RedirectErrors::Text("invalid"), "default")?;
        assert_eq!(
            session.get_old_input(Some("title"))?,
            Some(serde_json::json!("draft"))
        );
        assert_eq!(session.read_errors()?.count(), 1);
        // guest取previous根，随后setter覆盖；两次保存老化后普通意图属性仍在。
        redirector.set_intended_url("/protected")?;
        session.save().await?;
        let next = crate::session::Session::new(Some(session.id()?), store.clone(), None)?;
        next.start().await?;
        assert_eq!(next.get("notice")?, Some(serde_json::json!("once")));
        next.save().await?;
        let last = crate::session::Session::new(Some(next.id()?), store, None)?;
        last.start().await?;
        assert_eq!(last.get("notice")?, None);
        let info = intent_info(&last, "GET", false, &[]).await?;
        let redirect = Redirector::from_info(&info);
        assert_eq!(
            redirect.intended()?.target_url(),
            "http://example.test/protected"
        );
        assert_eq!(redirect.intended()?.target_url(), "http://example.test");
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn intent_session_missing_closed_and_budget_errors_keep_atomic_state() -> Result<()> {
        let info = info(None).await?;
        let redirect = Redirector::from_info(&info);
        for error in [
            redirect.guest("/login").err(),
            redirect.intended().err(),
            redirect.get_intended_url().err(),
            redirect.set_intended_url("/x").err(),
        ] {
            assert_eq!(
                error.expect("missing Session").code(),
                "session_layer_missing"
            );
        }
        assert!(redirect.secure("/login").is_ok());
        let session = crate::session::Session::with_options(
            None,
            std::sync::Arc::new(crate::session::MemorySessionStore::default()),
            None,
            512,
            "session".into(),
        )?;
        let info = intent_info(&session, "GET", true, &[]).await?;
        let redirect = Redirector::from_info(&info);
        redirect.set_intended_url("/original")?;
        let before = session.all()?;
        assert_eq!(
            redirect
                .set_intended_url("x".repeat(1024))
                .err()
                .expect("budget")
                .code(),
            "session_limits_exceeded"
        );
        assert_eq!(session.all()?, before);
        let (mut parts, _) = HttpRequest::get(format!("/protected?token={}", "x".repeat(1024)))
            .header("host", "example.test")
            .body(Body::empty())
            .expect("request")
            .into_parts();
        parts.extensions.insert(session.clone());
        let long_info = RequestInfo::from_request_parts(&mut parts, &()).await?;
        long_info.set_route_resolver(info.get_route_resolver()?)?;
        assert_eq!(
            Redirector::from_info(&long_info)
                .guest_with("/login", StatusCode::OK, HeaderMap::new(), None)
                .err()
                .expect("budget before response validation")
                .code(),
            "session_limits_exceeded"
        );
        assert_eq!(session.all()?, before);
        session.close();
        assert_eq!(
            redirect.intended().err().expect("closed").code(),
            "session_closed"
        );
        assert_eq!(
            redirect.guest("/login").err().expect("closed").code(),
            "session_closed"
        );
        assert_eq!(
            redirect.get_intended_url()?,
            Some(serde_json::json!("/original"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn named_redirect_requires_captured_application_generator() -> Result<()> {
        let info = info(None).await?;
        let redirect = Redirector::from_info(&info);
        let error = redirect
            .route("done", &BTreeMap::new())
            .err()
            .expect("missing generator");
        assert_eq!(error.code(), "http_route_url_unavailable");
        assert_eq!(
            redirect.to("/done")?.target_url(),
            "http://example.test/done"
        );
        Ok(())
    }

    #[tokio::test]
    async fn named_redirect_keeps_request_snapshot_generator_after_extension_replacement()
    -> Result<()> {
        let (mut parts, _) = HttpRequest::get("/")
            .body(Body::empty())
            .expect("request")
            .into_parts();
        parts.extensions.insert(named_urls("/first")?);
        let first = RequestInfo::from_request_parts(&mut parts, &()).await?;
        let copied = first.clone();
        parts.extensions.insert(named_urls("/second")?);
        let second = RequestInfo::from_request_parts(&mut parts, &()).await?;
        parts.extensions.remove::<super::super::RouteUrlGenerator>();
        for (info, target) in [
            (&first, "https://configured.example.test/base/first"),
            (&copied, "https://configured.example.test/base/first"),
            (&second, "https://configured.example.test/base/second"),
        ] {
            assert_eq!(
                Redirector::from_info(info)
                    .route("done", &BTreeMap::new())?
                    .target_url(),
                target
            );
        }
        Ok(())
    }

    #[cfg(feature = "session")]
    #[tokio::test]
    async fn named_redirect_preserves_request_input_and_error_flash_borrows() -> Result<()> {
        let session = crate::session::Session::new(
            None,
            std::sync::Arc::new(crate::session::MemorySessionStore::default()),
            None,
        )?;
        let mut incoming = HttpRequest::post("/form")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"title":"draft"}"#))
            .expect("request");
        incoming.extensions_mut().insert(session.clone());
        incoming.extensions_mut().insert(named_urls("/done")?);
        let request = Request::from_request(incoming, &()).await?;
        let mut redirect = Redirector::from_request(&request).route("done", &BTreeMap::new())?;
        redirect
            .with_input()?
            .with_errors(RedirectErrors::Text("invalid"), "default")?;
        assert_eq!(
            session.get_old_input(Some("title"))?,
            Some(serde_json::json!("draft"))
        );
        assert_eq!(
            session.read_errors()?.default_first(None, None)?,
            Some("invalid".into())
        );
        assert_eq!(
            redirect.target_url(),
            "https://configured.example.test/base/done"
        );
        assert_eq!(redirect.into_response().status(), StatusCode::FOUND);
        Ok(())
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
