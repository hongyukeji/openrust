//! 生成应用经真实 `Router`、账户 `Provider` 与受保护 `Session` 完成现代 `Fortify` 管理流程。
use figment::Figment;
use openrust::container::ServiceKey;
use openrust::events::{Event, Events, EventsServiceProvider};
use openrust::fortify::events::TwoFactorAuthenticationEnabled;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::http::axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use openrust::http::Json;
use openrust::routing::RouteGroup;
use openrust::mail::{MailDriver, Mailer};
use openrust::session::Session;
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::testing::TestApp;
use openrust::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const CSRF: &str = "/__generated/fortify-http/csrf";
const PASSWORD: &str = "generated-modern-fortify-password";
const TIMEOUT: Duration = Duration::from_secs(5);

fn phase(stage: &str, error: &Error) {
    eprintln!("生成Fortify测试失败 stage={stage} code={}", error.code());
}

fn fixture_error() -> Error {
    Error::custom(
        "generated_fortify_http_fixture",
        "生成应用Fortify HTTP测试数据缺失",
    )
}

#[derive(Clone, Default)]
struct CaptureMail(Arc<Mutex<Option<String>>>);
impl CaptureMail {
    fn verification_code(&self) -> Result<String> {
        self.0
            .lock()
            .map_err(|_| fixture_error())?
            .clone()
            .ok_or_else(fixture_error)
    }
}
impl MailDriver for CaptureMail {
    fn send(&self, message: openrust::mail::lettre::Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let formatted = message.formatted();
            let text = String::from_utf8_lossy(&formatted);
            let code: String = text
                .split("Your OpenRust verification code is: ")
                .nth(1)
                .ok_or_else(fixture_error)?
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if code.len() != 6 {
                return Err(fixture_error());
            }
            *self.0.lock().map_err(|_| fixture_error())? = Some(code);
            Ok(())
        })
    }
}
struct HttpSupport;
impl ServiceProvider for HttpSupport {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mail = CaptureMail::default();
            app.instance(Arc::new(mail.clone())).await?;
            app.instance(Arc::new(Mailer::new(mail))).await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes.group(RouteGroup::new().middleware_group("web"), |routes| {
                    routes.get(CSRF, csrf)
                })
            })?;
            Ok(())
        })
    }
}
async fn csrf(session: Session) -> Result<Json<Value>> {
    Ok(Json(json!({"token":session.token()?})))
}
struct FortifyApplication<A>(std::marker::PhantomData<fn() -> A>);
impl<A: ApplicationDefinition> ApplicationDefinition for FortifyApplication<A> {
    type Config = A::Config;
    fn config() -> openrust::config::ConfigManifest {
        A::config()
    }
    fn config_namespace() -> &'static str {
        A::config_namespace()
    }
    fn default_providers() -> openrust::support::DefaultProviders {
        A::default_providers()
    }
    fn package_providers(config: &Self::Config) -> Vec<openrust::foundation::PackageProvider> {
        A::package_providers(config)
    }
    fn additional_providers() -> Vec<Box<dyn ServiceProvider>> {
        A::additional_providers()
    }
    fn with_bootstrap_providers() -> bool {
        A::with_bootstrap_providers()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = A::providers(config);
        providers.push(Box::new(HttpSupport));
        providers
    }
    fn configure_providers(config: &Self::Config, builder: &mut openrust::foundation::ProviderBuilder) {
        A::configure_providers(config, builder);
    }
}

struct Reply {
    status: StatusCode,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Result<Value> {
        serde_json::from_slice(&self.body).map_err(Error::internal)
    }
    fn status(&self, expected: StatusCode, step: &str) {
        assert_eq!(self.status, expected, "生成Fortify步骤：{step}");
    }
    fn empty_json(&self, expected: StatusCode, step: &str) -> Result<()> {
        self.status(expected, step);
        assert_eq!(self.json()?, json!(""));
        Ok(())
    }
}
struct Browser<'a, A: ApplicationDefinition> {
    app: &'a TestApp<FortifyApplication<A>>,
    cookies: BTreeMap<String, String>,
    token: Option<String>,
}
impl<'a, A: ApplicationDefinition> Browser<'a, A> {
    fn new(app: &'a TestApp<FortifyApplication<A>>) -> Self {
        Self {
            app,
            cookies: BTreeMap::new(),
            token: None,
        }
    }
    async fn send(
        &mut self,
        method: Method,
        path: &str,
        input: Option<Value>,
        csrf: bool,
    ) -> Result<Reply> {
        let mut request = Request::builder()
            .method(method)
            .uri(format!("https://localhost{path}"))
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 42101))))
            .header(header::HOST, "localhost")
            .header(header::ACCEPT, "application/json")
            .header(header::REFERER, "https://localhost/settings");
        if !self.cookies.is_empty() {
            request = request.header(
                header::COOKIE,
                self.cookies
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        if csrf && let Some(token) = &self.token {
            request = request.header("x-csrf-token", token);
        }
        let body = match input {
            Some(value) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let response = openrust::tokio::time::timeout(
            TIMEOUT,
            self.app
                .raw_response(request.body(body).map_err(Error::internal)?),
        )
        .await
        .map_err(Error::internal)??;
        for cookie in response.headers().get_all(header::SET_COOKIE) {
            let cookie = cookie.to_str().map_err(Error::internal)?;
            let Some((name, value)) = cookie
                .split(';')
                .next()
                .and_then(|pair| pair.split_once('='))
            else {
                return Err(fixture_error());
            };
            if value.is_empty() || cookie.to_ascii_lowercase().contains("max-age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.into(), value.into());
            }
        }
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .map_err(Error::internal)?
            .to_vec();
        Ok(Reply { status, body })
    }
    async fn get(&mut self, path: &str) -> Result<Reply> {
        self.send(Method::GET, path, None, false).await
    }
    async fn post(&mut self, path: &str, input: Value) -> Result<Reply> {
        self.send(Method::POST, path, Some(input), true).await
    }
    async fn refresh_csrf(&mut self) -> Result<()> {
        let response = self.get(CSRF).await?;
        response.status(StatusCode::OK, "取得真实CSRF");
        let envelope = response.json()?;
        assert_eq!(envelope["success"], true);
        self.token = Some(
            envelope["data"]["token"]
                .as_str()
                .ok_or_else(fixture_error)?
                .into(),
        );
        Ok(())
    }
    async fn login(&mut self, email: &str, challenged: bool) -> Result<()> {
        let response = self
            .post("/login", json!({"account":email,"password":PASSWORD}))
            .await?;
        response.status(StatusCode::OK, "组合账号密码登录");
        assert_eq!(response.json()?, json!({"two_factor":challenged}));
        if !challenged {
            self.refresh_csrf().await?;
        }
        Ok(())
    }
    async fn logout(&mut self) -> Result<()> {
        let response = self.post("/logout", json!({})).await?;
        response.status(StatusCode::NO_CONTENT, "退出");
        assert_eq!(response.body, [] as [u8; 0]);
        self.refresh_csrf().await
    }
}

async fn register<A: ApplicationDefinition>(browser: &mut Browser<'_, A>, email: &str) -> Result<()> {
    let response = browser
        .post(
            "/api/auth/register",
            json!({"email":email,"password":PASSWORD}),
        )
        .await?;
    response.status(StatusCode::OK, "实际注册");
    let registered = response.json()?;
    assert_eq!(registered["success"], true);
    assert_eq!(registered["data"]["status"], "accepted");
    let mail = browser.app.app()?.state().resolve::<CaptureMail>().await?;
    let response = browser
        .post(
            "/api/auth/verify-email",
            json!({"email":email,"code":mail.verification_code()?}),
        )
        .await?;
    response.status(StatusCode::OK, "实际邮件验证码验证");
    assert_eq!(response.json()?["data"]["status"], "verified");
    Ok(())
}

async fn confirm_password<A: ApplicationDefinition>(browser: &mut Browser<'_, A>) -> Result<()> {
    browser
        .post("/user/confirm-password", json!({"password":PASSWORD}))
        .await?
        .empty_json(StatusCode::CREATED, "确认密码")
}

async fn enable_and_confirm<A: ApplicationDefinition>(browser: &mut Browser<'_, A>) -> Result<()> {
    let denied = browser
        .send(
            Method::POST,
            "/user/two-factor-authentication",
            Some(json!({})),
            false,
        )
        .await?;
    denied.status(
        StatusCode::from_u16(419).map_err(Error::internal)?,
        "缺少CSRF禁止启用",
    );
    browser
        .post("/user/two-factor-authentication", json!({}))
        .await?
        .status(StatusCode::LOCKED, "确认密码前管理被拒绝");
    confirm_password(browser).await?;
    browser
        .post("/user/two-factor-authentication", json!({}))
        .await?
        .empty_json(StatusCode::OK, "启用现代双因素")?;
    let response = browser.get("/user/two-factor-secret-key").await?;
    response.status(StatusCode::OK, "读取新账户真实secret");
    let secret = response.json()?["secretKey"]
        .as_str()
        .ok_or_else(fixture_error)?
        .to_owned();
    assert_eq!(secret.len(), 16);
    browser
        .post(
            "/user/confirmed-two-factor-authentication",
            json!({"code":authenticator_code(&secret)?}),
        )
        .await?
        .empty_json(StatusCode::OK, "实际TOTP确认")?;
    Ok(())
}

async fn recovery_codes<A: ApplicationDefinition>(browser: &mut Browser<'_, A>) -> Result<Vec<String>> {
    let response = browser.get("/user/two-factor-recovery-codes").await?;
    response.status(StatusCode::OK, "读取恢复码");
    let codes: Vec<String> = serde_json::from_value(response.json()?).map_err(Error::internal)?;
    assert_eq!(codes.len(), 8);
    Ok(codes)
}
async fn rotate_and_disable<A: ApplicationDefinition>(
    browser: &mut Browser<'_, A>,
    email: &str,
) -> Result<()> {
    let original = recovery_codes(browser).await?;
    browser
        .post("/user/two-factor-recovery-codes", json!({}))
        .await?
        .empty_json(StatusCode::OK, "轮换恢复码")?;
    let replacement = recovery_codes(browser).await?;
    assert!(original.iter().all(|old| !replacement.contains(old)));
    browser.logout().await?;
    browser.login(email, true).await?;
    let code = replacement.first().ok_or_else(fixture_error)?;
    let response = browser
        .post("/two-factor-challenge", json!({"recovery_code":code}))
        .await?;
    response.status(StatusCode::NO_CONTENT, "轮换后恢复码真实挑战");
    assert_eq!(response.body, [] as [u8; 0]);
    browser.refresh_csrf().await?;
    browser
        .get("/user/two-factor-recovery-codes")
        .await?
        .status(StatusCode::LOCKED, "新登录尚未确认管理密码");
    confirm_password(browser).await?;
    assert_eq!(recovery_codes(browser).await?.len(), 8);
    browser
        .send(
            Method::DELETE,
            "/user/two-factor-authentication",
            Some(json!({})),
            true,
        )
        .await?
        .empty_json(StatusCode::OK, "禁用现代因子")?;
    browser.logout().await?;
    browser.login(email, false).await
}

// 独立RFC6238客户端，只产生输入验证码，不替代服务端Provider或写入证明。
fn authenticator_code(encoded: &str) -> Result<String> {
    use hmac::{Hmac, Mac, digest::KeyInit};
    let (mut bits, mut buffer, mut secret) = (0_u8, 0_u32, Vec::new());
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err(fixture_error()),
        };
        buffer = ((buffer << 5) | u32::from(value)) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            secret.push(u8::try_from((buffer >> bits) & 0xff).map_err(Error::internal)?);
        }
    }
    let step = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(Error::internal)?
        .as_secs()
        / 30;
    let mut mac =
        <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(&secret).map_err(Error::internal)?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    Ok(format!("{:06}", binary % 1_000_000))
}

/// 使用调用方隔离数据库的配置，运行生成应用的现代 `Fortify` 真实 `Router` 管理闭环。
///
/// 调用方管理数据库；保留生产认证绑定，并观察 CSRF、邮件和双因素启用事件。
pub async fn exercise<A: ApplicationDefinition>(input: Figment) -> Result<()> {
    // TestApp默认testing会跳过CSRF；明确启用local以执行真实保护，绝不移除CSRF中间件。
    let entries = input
        .extract::<BTreeMap<String, Value>>()
        .map_err(Error::internal)?;
    let sources = openrust::config::ConfigLoader::application_code_sources_with_overrides(
        ".",
        A::config_namespace(),
        Some("local"),
        &A::config(),
        entries,
    )?
    .merge_ordered_entries([("app", json!({"env":"local","url":"https://localhost"}))])?;
    let enabled = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&enabled);
    let mut builder = TestApp::<FortifyApplication<A>>::builder(sources);
    builder.configure_providers(move |providers| {
        let mut events = EventsServiceProvider::new();
        let observed = Arc::clone(&observed);
        events.listen::<TwoFactorAuthenticationEnabled, _>(move |_, context| {
            assert_eq!(context.event_name(), TwoFactorAuthenticationEnabled::NAME);
            observed.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(()) })
        });
        providers.force_provider(events);
    });
    let mut app = builder.build().await?;
    let outcome: Result<()> = async {
        let state = app.app()?.state();
        let typed = state.resolve::<Events>().await?;
        let named = state.resolve_key::<Events>(ServiceKey::named("events")).await?;
        assert!(Arc::ptr_eq(&typed, &named));
        assert_eq!(
            app.app()?.config().app.env,
            openrust::config::Environment::Local
        );
        let email = format!(
            "fortify-{}-{}@example.test",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(Error::internal)?
                .as_nanos()
        );
        let mut browser = Browser::new(&app);
        browser
            .refresh_csrf()
            .await
            .inspect_err(|error| phase("csrf", error))?;
        register(&mut browser, &email)
            .await
            .inspect_err(|error| phase("register", error))?;
        browser
            .login(&email, false)
            .await
            .inspect_err(|error| phase("login", error))?;
        enable_and_confirm(&mut browser)
            .await
            .inspect_err(|error| phase("enable", error))?;
        assert_eq!(enabled.load(Ordering::Relaxed), 1);
        rotate_and_disable(&mut browser, &email)
            .await
            .inspect_err(|error| phase("rotate_disable", error))
    }
    .await;
    let shutdown = app
        .shutdown(openrust::tokio::time::Instant::now() + TIMEOUT)
        .await;
    outcome?;
    shutdown
}
