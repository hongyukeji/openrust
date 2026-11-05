//! Fortify 的本地表单视图；请求、Session 与具名路由继续使用同一框架机制。

use openrust::auth::fortify::{
    ConfirmPasswordViewResponse, LoginViewResponse, TwoFactorChallengeViewResponse,
};
use openrust::core::BoxFuture;
use openrust::http::axum::response::{Html, IntoResponse, Response};
use openrust::http::{Redirector, Request};
use openrust::support::{MessageValue, ViewErrorBag};
use openrust::{Error, Result};
use std::collections::BTreeMap;

/// 唯一示例的 Fortify 登录、密码确认与第二因子挑战页面。
pub(crate) struct ExampleFortifyViews {
    username: String,
}

impl ExampleFortifyViews {
    /// `username` 来自最终 `fortify.username` 配置，不另定凭据字段。
    pub(crate) fn new(username: impl Into<String>) -> Self {
        Self {
            username: username.into(),
        }
    }

    fn render(&self, request: &Request, page: Page) -> Result<Response> {
        let session = request.session()?;
        let token = session
            .token()?
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| {
                Error::custom(
                    "example_fortify_csrf_missing",
                    "Fortify 表单需要已启动 Session 的 CSRF token",
                )
            })?;
        let target = Redirector::from_request(request).route(page.route(), &BTreeMap::new())?;
        let forms = forms(page, &self.username, target.target_url(), &token);
        let errors = errors_html(&session.read_errors()?);
        Ok(Html(document(page.title(), &forms, &errors)).into_response())
    }
}

impl LoginViewResponse for ExampleFortifyViews {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        let response = self.render(request, Page::Login);
        Box::pin(async move { response })
    }
}

impl ConfirmPasswordViewResponse for ExampleFortifyViews {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        let response = self.render(request, Page::ConfirmPassword);
        Box::pin(async move { response })
    }
}

impl TwoFactorChallengeViewResponse for ExampleFortifyViews {
    fn to_response<'a>(&'a self, request: &'a Request) -> BoxFuture<'a, Result<Response>> {
        let response = self.render(request, Page::Challenge);
        Box::pin(async move { response })
    }
}

#[derive(Clone, Copy)]
enum Page {
    Login,
    ConfirmPassword,
    Challenge,
}

impl Page {
    fn route(self) -> &'static str {
        match self {
            Self::Login => "login.store",
            Self::ConfirmPassword => "password.confirm.store",
            Self::Challenge => "two-factor.login.store",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Login => "登录",
            Self::ConfirmPassword => "确认密码",
            Self::Challenge => "两步验证",
        }
    }
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn form(action: &str, token: &str, fields: &str) -> String {
    format!(
        "<form method=\"post\" action=\"{}\"><input type=\"hidden\" name=\"_token\" value=\"{}\">{fields}</form>",
        escape(action),
        escape(token),
    )
}

fn forms(page: Page, username: &str, action: &str, token: &str) -> String {
    match page {
        Page::Login => form(
            action,
            token,
            &format!(
                "<label for=\"account\">用户名／邮箱／手机号</label>\
             <input id=\"account\" name=\"{}\" type=\"text\" autocomplete=\"username\" required>\
             <label for=\"password\">密码</label>\
             <input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required>\
             <label><input name=\"remember\" type=\"checkbox\" value=\"1\">记住我</label>\
             <button type=\"submit\">登录</button>",
                escape(username),
            ),
        ),
        Page::ConfirmPassword => form(
            action,
            token,
            "<p>继续前请确认当前密码。</p>\
             <label for=\"password\">密码</label>\
             <input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required>\
             <button type=\"submit\">确认密码</button>",
        ),
        // 两个独立表单，各自只提交一种凭据，浏览器不会要求同时填两项。
        Page::Challenge => format!(
            "<p>请选择验证码或恢复码中的一种完成验证。</p>{}{}",
            form(
                action,
                token,
                "<label for=\"code\">验证器验证码</label>\
                 <input id=\"code\" name=\"code\" type=\"text\" inputmode=\"numeric\" autocomplete=\"one-time-code\" required>\
                 <button type=\"submit\">验证验证码</button>"
            ),
            form(
                action,
                token,
                "<label for=\"recovery-code\">恢复码</label>\
                 <input id=\"recovery-code\" name=\"recovery_code\" type=\"text\" autocomplete=\"off\" spellcheck=\"false\" required>\
                 <button type=\"submit\">使用恢复码</button>"
            ),
        ),
    }
}

fn error_items(message: &MessageValue, output: &mut String) {
    match message {
        MessageValue::Text(message) => {
            output.push_str("<li>");
            output.push_str(&escape(message));
            output.push_str("</li>");
        }
        MessageValue::Nested(messages) => {
            for (_, message) in messages {
                error_items(message, output);
            }
        }
    }
}

fn errors_html(errors: &ViewErrorBag) -> String {
    let mut items = String::new();
    // 只读命名错误包的消息，不读取旧输入，也不采用自定义消息包的HTML格式模板。
    for (_, bag) in errors.bags() {
        for (_, messages) in bag.as_bag().messages() {
            for (_, message) in messages {
                error_items(message, &mut items);
            }
        }
    }
    if items.is_empty() {
        String::new()
    } else {
        format!("<section role=\"alert\" aria-label=\"表单错误\"><ul>{items}</ul></section>")
    }
}

fn document(title: &str, forms: &str, errors: &str) -> String {
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{}</title></head><body><main><h1>{}</h1>{errors}{forms}</main></body></html>",
        escape(title),
        escape(title),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrust::config::{FrameworkConfig, NoConfig, SessionConfig};
    use openrust::core::{Application, BootContext, DefaultProviders, ServiceProvider};
    use openrust::http::{CookieServiceProvider, HttpServiceProvider, Path, RouteGroup};
    use openrust::session::{MemorySessionStore, SessionServiceProvider};
    use openrust::support::MessageBag;
    use openrust::testing::TestApp;
    use std::time::Duration;

    const TOKEN: &str = "csrf&<\"'>";
    const SECRET: &str = "old-password-code-recovery-must-never-appear";

    #[test]
    fn html_escapes_attribute_values_and_keeps_credentials_in_separate_forms() {
        let malicious = "\"><script>alert('x')</script>&";
        let login = forms(Page::Login, malicious, malicious, malicious);
        assert!(!login.contains("<script>"));
        assert!(login.contains("&quot;&gt;&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&amp;"));
        assert_eq!(login.matches("autocomplete=\"username\"").count(), 1);
        assert!(login.contains("name=\"remember\""));
        let confirm = forms(Page::ConfirmPassword, "account", "/confirm", TOKEN);
        assert!(!confirm.contains("name=\"account\""));
        assert!(!confirm.contains("name=\"remember\""));
        let challenge = forms(Page::Challenge, "account", "/challenge", TOKEN);
        let alternatives = challenge
            .split("</form>")
            .filter(|part| part.contains("<form"))
            .collect::<Vec<_>>();
        assert_eq!(alternatives.len(), 2);
        assert!(alternatives[0].contains("name=\"code\""));
        assert!(!alternatives[0].contains("name=\"recovery_code\""));
        assert!(alternatives[1].contains("name=\"recovery_code\""));
        assert!(!alternatives[1].contains("name=\"code\""));
    }

    struct ViewApp;
    impl Application for ViewApp {
        type Config = NoConfig;
        fn default_providers() -> DefaultProviders {
            DefaultProviders::empty()
        }
        fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
            vec![
                Box::new(HttpServiceProvider),
                Box::new(CookieServiceProvider::plain()),
                Box::new(
                    SessionServiceProvider::with_store(MemorySessionStore::default())
                        .with_cookie_config(SessionConfig {
                            secure: Some(false),
                            ..SessionConfig::default()
                        }),
                ),
                Box::new(ViewRoutes),
            ]
        }
    }

    struct ViewRoutes;
    impl ServiceProvider for ViewRoutes {
        fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.routes(|routes| {
                    routes.group(
                        RouteGroup::new()
                            .domain("auth.example.test")
                            .prefix("/auth-v2")
                            .middleware_alias("cookies")
                            .middleware_alias("session"),
                        |routes| {
                            routes
                                .get("/screen/{page}", screen)
                                .post("/sign-in", submitted)
                                .name("login.store")
                                .post("/confirm-now", submitted)
                                .name("password.confirm.store")
                                .post("/challenge-now", submitted)
                                .name("two-factor.login.store")
                        },
                    )
                })?;
                Ok(())
            })
        }
    }

    async fn submitted() -> &'static str {
        "unused"
    }

    async fn screen(Path(page): Path<String>, request: Request) -> Result<Response> {
        let session = request.session()?;
        session.put_typed("_token", TOKEN)?;
        session.put_typed(
            "_old_input",
            serde_json::json!({"password":SECRET,"code":SECRET,"recovery_code":SECRET}),
        )?;
        let mut errors = MessageBag::new();
        errors.add("account", "<script>bad & \"input\"</script>")?;
        session.flash_errors("login", &errors)?;
        let views = ExampleFortifyViews::new("account");
        let response = match page.as_str() {
            "login" => LoginViewResponse::to_response(&views, &request).await?,
            "confirm" => ConfirmPasswordViewResponse::to_response(&views, &request).await?,
            "challenge" => TwoFactorChallengeViewResponse::to_response(&views, &request).await?,
            _ => return Err(Error::NotFound("未知测试页面".into())),
        };
        assert!(
            session.token()? == Some(serde_json::json!(TOKEN)),
            "{page}: 视图不得替换当前Session token"
        );
        Ok(response)
    }

    #[tokio::test]
    async fn trait_views_use_named_overrides_session_token_and_escaped_named_errors() -> Result<()>
    {
        let mut framework = FrameworkConfig::default();
        // 具名路由继承app.url的scheme/port；显式给出无端口基址，同时保留不同domain的覆写验证。
        framework.app.url = "http://app.example.test".into();
        let source = figment::Figment::from(figment::providers::Serialized::defaults(framework));
        let mut app = TestApp::<ViewApp>::builder(source).build().await?;
        let outcome: Result<()> = async {
            for (page, path) in [
                ("login", "sign-in"),
                ("confirm", "confirm-now"),
                ("challenge", "challenge-now"),
            ] {
                let request =
                    openrust::http::axum::http::Request::get(format!("/auth-v2/screen/{page}"))
                        .header("host", "auth.example.test")
                        .body(openrust::http::axum::body::Body::empty())
                        .map_err(Error::internal)?;
                let response = app.request(request).await?;
                response.assert_status(openrust::http::axum::http::StatusCode::OK)?;
                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|value| value.to_str().ok());
                assert_eq!(
                    content_type,
                    Some("text/html; charset=utf-8"),
                    "{page}: 响应必须直接返回HTML，不能套JSON envelope"
                );
                let html = std::str::from_utf8(response.body()).map_err(Error::internal)?;
                let expected = format!("action=\"http://auth.example.test/auth-v2/{path}\"");
                assert!(
                    html.starts_with("<!doctype html>"),
                    "{page}: 缺少HTML文档起始标记"
                );
                // 只在失败时打印本夹具的合成action，避免整页HTML及token进入诊断输出。
                let actions = html
                    .split("action=\"")
                    .skip(1)
                    .filter_map(|part| part.split('"').next())
                    .collect::<Vec<_>>();
                assert!(
                    html.contains(&expected),
                    "{page}: 官方具名路由action不匹配；预期={expected}；实际={actions:?}"
                );
                assert!(
                    html.contains("value=\"csrf&amp;&lt;&quot;&#39;&gt;\""),
                    "{page}: 当前Session的合成CSRF token未完整转义"
                );
                assert!(
                    html.contains("&lt;script&gt;bad &amp; &quot;input&quot;&lt;/script&gt;"),
                    "{page}: 命名Session错误消息未出现或未完整转义"
                );
                assert!(
                    !html.contains("<script>"),
                    "{page}: 错误消息泄露为可执行HTML"
                );
                assert!(
                    !html.contains(SECRET),
                    "{page}: 密码、验证码或恢复码旧输入泄露"
                );
            }
            Ok(())
        }
        .await;
        let stopped = app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await;
        outcome?;
        stopped
    }
}
