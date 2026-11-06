//! 显式JWT API生成应用的真实路由与因子流程；所有响应均禁止Set-Cookie。
use figment::Figment;
use openrust::foundation::ApplicationDefinition;
use openrust::http::axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use openrust::mail::{MailDriver, Mailer};
use openrust::testing::TestApp;
use openrust::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
const PASSWORD: &str = "generated-api-real-password";
const PREFIX: &str = "/api/auth";
const BUDGET: Duration = Duration::from_secs(10);
fn missing() -> Error {
    Error::custom("generated_jwt_api_fixture", "生成JWT API测试状态不匹配")
}
#[derive(Clone, Default)]
struct CaptureMail(Arc<Mutex<Option<String>>>);
impl MailDriver for CaptureMail {
    fn send(
        &self,
        message: openrust::mail::lettre::Message,
    ) -> openrust::support::BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let bytes = message.formatted();
            let text = String::from_utf8_lossy(&bytes);
            let code: String = text
                .split("Your OpenRust verification code is: ")
                .nth(1)
                .ok_or_else(missing)?
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if code.len() != 6 {
                return Err(missing());
            }
            *self.0.lock().map_err(|_| missing())? = Some(code);
            Ok(())
        })
    }
}
struct Reply {
    status: StatusCode,
    value: Option<Value>,
}
impl Reply {
    fn status(&self, expected: StatusCode, step: &str) {
        assert_eq!(self.status, expected, "JWT API步骤：{step}");
    }
    fn json(&self) -> Result<&Value> {
        self.value.as_ref().ok_or_else(missing)
    }
    fn text(&self, key: &str) -> Result<String> {
        self.json()?[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(missing)
    }
}
struct Client<'a, A: ApplicationDefinition> {
    app: &'a TestApp<A>,
    username: String,
}
impl<A: ApplicationDefinition> Client<'_, A> {
    async fn send(
        &self,
        method: Method,
        path: &str,
        input: Option<Value>,
        access: Option<&str>,
        management: Option<&str>,
        peer: u8,
    ) -> Result<Reply> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "localhost")
            .header(header::ACCEPT, "application/json")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, peer], 42100))));
        if let Some(access) = access {
            request = request.header(header::AUTHORIZATION, format!("Bearer {access}"));
        }
        if let Some(management) = management {
            request = request.header("x-fortify-management", management);
        }
        let body = match input {
            Some(input) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(input.to_string())
            }
            None => Body::empty(),
        };
        let response = openrust::tokio::time::timeout(
            BUDGET,
            self.app
                .raw_response(request.body(body).map_err(Error::internal)?),
        )
        .await
        .map_err(Error::internal)??;
        assert_eq!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .count(),
            0,
            "JWT API响应不得设置Cookie"
        );
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .map_err(Error::internal)?;
        Ok(Reply {
            status,
            value: serde_json::from_slice(&bytes).ok(),
        })
    }
    async fn call(
        &self,
        method: Method,
        path: &str,
        input: Option<Value>,
        access: Option<&str>,
        management: Option<&str>,
    ) -> Result<Reply> {
        self.send(
            method,
            &format!("{PREFIX}{path}"),
            input,
            access,
            management,
            71,
        )
        .await
    }
    async fn login(&self, email: &str) -> Result<Reply> {
        let mut fields = serde_json::Map::new();
        fields.insert(self.username.clone(), Value::from(email));
        fields.insert("password".into(), Value::from(PASSWORD));
        fields.insert("remember".into(), Value::Bool(true));
        let response = self
            .call(
                Method::POST,
                "/login",
                Some(Value::Object(fields)),
                None,
                None,
            )
            .await?;
        response.status(StatusCode::OK, "真实密码登录");
        Ok(response)
    }
    async fn user(&self, access: &str, expected: StatusCode) -> Result<()> {
        self.call(Method::GET, "/user", None, Some(access), None)
            .await?
            .status(expected, "当前JWT资源身份");
        Ok(())
    }
    async fn business_user(&self, access: &str, expected: StatusCode) -> Result<Reply> {
        let response = self
            .send(Method::GET, "/api/me", None, Some(access), None, 71)
            .await?;
        response.status(expected, "业务资源使用具名 api Guard");
        Ok(response)
    }
    async fn authorize(&self, access: &str, password: &str) -> Result<Reply> {
        self.call(
            Method::POST,
            "/two-factor/authorize",
            Some(json!({"password":password})),
            Some(access),
            None,
        )
        .await
    }
    async fn complete(
        &self,
        challenge: &str,
        code: Option<&str>,
        recovery: Option<&str>,
    ) -> Result<Reply> {
        self.call(
            Method::POST,
            "/two-factor-challenge",
            Some(json!({"challenge":challenge,"code":code,"recovery_code":recovery})),
            None,
            None,
        )
        .await
    }
}
async fn register<A: ApplicationDefinition>(
    client: &Client<'_, A>,
    mail: &CaptureMail,
    email: &str,
) -> Result<()> {
    let reply = client
        .call(
            Method::POST,
            "/register",
            Some(json!({"email":email,"password":PASSWORD})),
            None,
            None,
        )
        .await?;
    reply.status(StatusCode::OK, "真实注册");
    assert_eq!(reply.json()?["data"]["status"], "accepted");
    let code = mail
        .0
        .lock()
        .map_err(|_| missing())?
        .clone()
        .ok_or_else(missing)?;
    let verified = client
        .call(
            Method::POST,
            "/verify-email",
            Some(json!({"email":email,"code":code})),
            None,
            None,
        )
        .await?;
    verified.status(StatusCode::OK, "真实邮件确认");
    assert_eq!(verified.json()?["data"]["status"], "verified");
    Ok(())
}
async fn unbound_credentials<A: ApplicationDefinition>(
    client: &Client<'_, A>,
    email: &str,
    user: i64,
) -> Result<()> {
    let jwt = client
        .app
        .app()?
        .state()
        .resolve::<openrust::auth::JwtTokens>()
        .await?;
    let raw = jwt.issue_access_token(user)?;
    client.user(&raw, StatusCode::UNAUTHORIZED).await?;
    client.business_user(&raw, StatusCode::UNAUTHORIZED).await?;
    let flows = client
        .app
        .app()?
        .state()
        .resolve::<openrust::auth::AuthFlows>()
        .await?;
    let unbound = flows
        .login(email, PASSWORD, "generated-unbound-profile")
        .await?;
    client
        .user(unbound.access_token(), StatusCode::UNAUTHORIZED)
        .await?;
    client
        .business_user(unbound.access_token(), StatusCode::UNAUTHORIZED)
        .await?;
    Ok(())
}
async fn enable_confirm<A: ApplicationDefinition>(
    client: &Client<'_, A>,
    email: &str,
    access: &str,
) -> Result<(String, Vec<String>)> {
    client
        .authorize(access, "incorrect-password")
        .await?
        .status(StatusCode::UNAUTHORIZED, "管理错误密码");
    let grant = client.authorize(access, PASSWORD).await?;
    grant.status(StatusCode::OK, "真实密码管理授权");
    let grant = grant.text("management_token")?;
    client.user(&grant, StatusCode::UNAUTHORIZED).await?;
    client
        .call(
            Method::POST,
            "/two-factor",
            Some(json!({})),
            None,
            Some("invalid"),
        )
        .await?
        .status(StatusCode::UNAUTHORIZED, "错误管理grant");
    let enabled = client
        .call(
            Method::POST,
            "/two-factor",
            Some(json!({})),
            None,
            Some(&grant),
        )
        .await?;
    enabled.status(StatusCode::OK, "启用待确认因子");
    assert_eq!(
        enabled.json()?,
        &json!({"enabled":true,"confirmation_required":true})
    );
    client.user(access, StatusCode::UNAUTHORIZED).await?;
    let secret = client
        .call(Method::GET, "/two-factor/secret", None, None, Some(&grant))
        .await?;
    secret.status(StatusCode::OK, "读取真实secret");
    let secret = secret.text("secret_key")?;
    assert_eq!(secret.len(), 16);
    let qr = client
        .call(Method::GET, "/two-factor/qr-code", None, None, Some(&grant))
        .await?;
    qr.status(StatusCode::OK, "真实QR");
    assert!(qr.text("url")?.starts_with("otpauth://totp/"));
    assert!(qr.text("svg")?.starts_with("<svg "));
    let before = client
        .call(
            Method::GET,
            "/two-factor/recovery-codes",
            None,
            None,
            Some(&grant),
        )
        .await?;
    before.status(StatusCode::OK, "读取8个恢复码");
    let before: Vec<String> = serde_json::from_value(before.json()?["recovery_codes"].clone())
        .map_err(Error::internal)?;
    assert_eq!(before.len(), 8);
    let rotated = client
        .call(
            Method::POST,
            "/two-factor/recovery-codes",
            Some(json!({})),
            None,
            Some(&grant),
        )
        .await?;
    rotated.status(StatusCode::OK, "同grant轮换恢复码");
    let codes: Vec<String> = serde_json::from_value(rotated.json()?["recovery_codes"].clone())
        .map_err(Error::internal)?;
    assert_eq!(codes.len(), 8);
    assert!(before.iter().all(|old| !codes.contains(old)));
    let pending = client.login(email).await?;
    let pending_access = pending.text("access_token")?;
    assert!(pending.json()?.get("challenge").is_none());
    client.user(&pending_access, StatusCode::OK).await?;
    let confirmed = client
        .call(
            Method::POST,
            "/two-factor/confirm",
            Some(json!({"code":totp(&secret,0)?})),
            None,
            Some(&grant),
        )
        .await?;
    confirmed.status(StatusCode::OK, "真实TOTP确认");
    assert_eq!(
        confirmed.json()?,
        &json!({"confirmed":true,"login_required":true})
    );
    client
        .call(Method::GET, "/two-factor/secret", None, None, Some(&grant))
        .await?
        .status(StatusCode::UNAUTHORIZED, "确认后旧grant已撤销，认证失败");
    client
        .user(&pending_access, StatusCode::UNAUTHORIZED)
        .await?;
    Ok((secret, codes))
}
async fn totp_login<A: ApplicationDefinition>(
    client: &Client<'_, A>,
    email: &str,
    secret: &str,
) -> Result<Reply> {
    let challenged = client.login(email).await?;
    assert_eq!(challenged.json()?["two_factor"], true);
    assert!(challenged.json()?.get("access_token").is_none());
    let challenge = challenged.text("challenge")?;
    let code = totp(secret, 1)?;
    client
        .send(
            Method::POST,
            &format!("{PREFIX}/two-factor-challenge"),
            Some(json!({"challenge":challenge,"code":code})),
            None,
            None,
            72,
        )
        .await?
        .status(StatusCode::UNAUTHORIZED, "错误来源挑战");
    let completed = client.complete(&challenge, Some(&code), None).await?;
    completed.status(StatusCode::OK, "真实TOTP完成");
    client
        .user(&completed.text("access_token")?, StatusCode::OK)
        .await?;
    client
        .complete(&challenge, Some(&code), None)
        .await?
        .status(StatusCode::UNAUTHORIZED, "挑战不可重放");
    Ok(completed)
}
async fn refresh_and_logout<A: ApplicationDefinition>(
    client: &Client<'_, A>,
    email: &str,
    pair: Reply,
    codes: &[String],
) -> Result<()> {
    let access = pair.text("access_token")?;
    let refresh = pair.text("refresh_token")?;
    for path in [
        "/refresh?refresh_token=forbidden",
        "/logout?refresh_token=forbidden",
    ] {
        client
            .call(Method::POST, path, Some(json!({})), None, None)
            .await?
            .status(StatusCode::BAD_REQUEST, "不从URL接受凭据");
    }
    let rotated = client
        .call(
            Method::POST,
            "/refresh",
            Some(json!({"refresh_token":refresh})),
            None,
            None,
        )
        .await?;
    rotated.status(StatusCode::OK, "真实refresh轮换");
    let next = rotated.text("access_token")?;
    client.user(&access, StatusCode::OK).await?;
    client.user(&next, StatusCode::OK).await?;
    client
        .call(
            Method::POST,
            "/refresh",
            Some(json!({"refresh_token":refresh})),
            None,
            None,
        )
        .await?
        .status(StatusCode::UNAUTHORIZED, "旧refresh重放撤销家族");
    client.user(&access, StatusCode::UNAUTHORIZED).await?;
    client.user(&next, StatusCode::UNAUTHORIZED).await?;
    let challenged = client.login(email).await?;
    let challenge = challenged.text("challenge")?;
    let first = codes.first().ok_or_else(missing)?;
    let pair = client.complete(&challenge, None, Some(first)).await?;
    pair.status(StatusCode::OK, "真实恢复码完成");
    client
        .complete(&challenge, None, Some(first))
        .await?
        .status(StatusCode::UNAUTHORIZED, "恢复挑战重放拒绝");
    let access = pair.text("access_token")?;
    client.user(&access, StatusCode::OK).await?;
    let logout = client
        .call(
            Method::POST,
            "/logout",
            Some(json!({"refresh_token":pair.text("refresh_token")?})),
            None,
            None,
        )
        .await?;
    logout.status(StatusCode::OK, "无Cookie退出");
    assert_eq!(logout.json()?, &json!({"logged_out":true}));
    client.user(&access, StatusCode::UNAUTHORIZED).await?;
    let challenged = client.login(email).await?;
    let challenge = challenged.text("challenge")?;
    let pair = client
        .complete(&challenge, None, Some(codes.get(1).ok_or_else(missing)?))
        .await?;
    pair.status(StatusCode::OK, "第二恢复码重新登录");
    let access = pair.text("access_token")?;
    let grant = client.authorize(&access, PASSWORD).await?;
    grant.status(StatusCode::OK, "已完成因子的真实管理授权");
    let disabled = client
        .call(
            Method::DELETE,
            "/two-factor",
            Some(json!({})),
            None,
            Some(&grant.text("management_token")?),
        )
        .await?;
    disabled.status(StatusCode::OK, "禁用因子");
    assert_eq!(
        disabled.json()?,
        &json!({"disabled":true,"login_required":true})
    );
    client.user(&access, StatusCode::UNAUTHORIZED).await
}
fn totp(encoded: &str, offset: u64) -> Result<String> {
    use hmac::{Hmac, Mac, digest::KeyInit};
    let (mut bits, mut buffer, mut secret) = (0_u8, 0_u32, Vec::new());
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err(missing()),
        };
        buffer = ((buffer << 5) | u32::from(value)) & 0xffff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            secret.push(u8::try_from((buffer >> bits) & 255).map_err(Error::internal)?);
        }
    }
    let counter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(Error::internal)?
        .as_secs()
        / 30
        + offset;
    let mut mac =
        <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(&secret).map_err(Error::internal)?;
    mac.update(&counter.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[19] & 15);
    let value = (u32::from(hash[offset] & 127) << 24)
        | (u32::from(hash[offset + 1]) << 16)
        | (u32::from(hash[offset + 2]) << 8)
        | u32::from(hash[offset + 3]);
    Ok(format!("{:06}", value % 1_000_000))
}
/// 使用调用方自有已迁移数据库，验证显式JWT API生成应用；不创建Session或假因子证明。
pub async fn exercise<A: ApplicationDefinition>(input: Figment) -> Result<()> {
    let entries = input
        .extract::<BTreeMap<String, Value>>()
        .map_err(Error::internal)?;
    let sources = openrust::config::ConfigLoader::application_code_sources_with_overrides(
        ".",
        A::config_namespace(),
        Some("local"),
        &A::config(),
        entries,
    )?;
    let mut builder = TestApp::<A>::builder(sources);
    let mail = CaptureMail::default();
    let captured = mail.clone();
    builder.configure_after_register(move |bindings| {
        let captured = captured.clone();
        Box::pin(async move {
            bindings.instance(Arc::new(Mailer::new(captured))).await?;
            Ok(())
        })
    });
    let mut app = builder.build().await?;
    let outcome: Result<()> = async {
        assert!(
            !app.app()?
                .state()
                .container()
                .contains::<openrust::session::SessionStoreHandle>()
        );
        assert!(
            app.app()?
                .section::<openrust::config::SessionConfig>("session")?
                .driver
                .is_none()
        );
        let config = app
            .app()?
            .config_repository()
            .section::<openrust::auth::fortify::FortifyConfig>("fortify")?;
        let client = Client {
            app: &app,
            username: config.username,
        };
        client
            .send(Method::GET, "/login", None, None, None, 71)
            .await?
            .status(StatusCode::NOT_FOUND, "不挂Web登录路由");
        let email = format!(
            "jwt-api-{}-{}@example.test",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(Error::internal)?
                .as_nanos()
        );
        register(&client, &mail, &email).await?;
        let initial = client.login(&email).await?;
        let access = initial.text("access_token")?;
        let user = initial.json()?["user_id"].as_i64().ok_or_else(missing)?;
        client.user(&access, StatusCode::OK).await?;
        let resource = client.business_user(&access, StatusCode::OK).await?;
        assert_eq!(resource.json()?["data"]["user_id"].as_i64(), Some(user));
        unbound_credentials(&client, &email, user).await?;
        let (secret, codes) = enable_confirm(&client, &email, &access).await?;
        let completed = totp_login(&client, &email, &secret).await?;
        refresh_and_logout(&client, &email, completed, &codes).await?;
        let db = app.app()?.state().resolve::<openrust::Db>().await?;
        assert_eq!(
            db.table("sessions")?.count().await?,
            0,
            "API完整流程不得创建Session记录"
        );
        assert_eq!(
            db.table("audit_records")?
                .where_eq("action", "auth.two_factor.login")?
                .count()
                .await?,
            3,
            "三次真实因子完成均在同一数据库保留脱敏审计"
        );
        Ok(())
    }
    .await;
    let stopped = app
        .shutdown(openrust::tokio::time::Instant::now() + BUDGET)
        .await;
    outcome?;
    stopped
}
