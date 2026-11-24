#[path = "../stubs/mod.rs"]
mod installed;
#[path = "../stubs/fortify_service_provider.rs"]
mod installed_provider;
#[cfg(feature = "sanctum")]
#[path = "../stubs/starter/migrations/token-credentials.rs"]
mod token_credential_migration;
#[cfg(feature = "sanctum")]
#[path = "../stubs/starter/migrations/user-credentials.rs"]
mod user_credential_migration;
#[path = "../stubs/starter/user_service_provider.rs"]
mod user_provider;
mod app {
    pub(crate) mod models {
        mod user {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../app/models/user.rs"
            ));
        }
        pub(crate) use user::User;
    }
    pub(crate) mod actions {
        pub(crate) mod fortify {
            pub(crate) use crate::installed::*;
        }
    }
}
impl Clone for app::models::User {
    fn clone(&self) -> Self {
        use openrust::database::eloquent::Model;
        Self::from_state(self.state().clone())
    }
}
#[cfg(feature = "sanctum")]
impl openrust::sanctum::HasApiTokens for app::models::User {}
impl openrust_fortify::TwoFactorAuthenticatable for app::models::User {}
impl openrust_fortify::passkeys::PasskeyAuthenticatable for app::models::User {}

use openrust::{
    auth::AuthManager,
    contracts::http::Kernel as _,
    foundation::{Application, http::Kernel},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
};
struct Root(std::path::PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Server {
    address: SocketAddr,
    mailer: Arc<openrust::mail::Mailer>,
    task: tokio::task::JoinHandle<()>,
    _root: Root,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
fn database_configuration(base: &std::path::Path) -> Value {
    let prefix = format!(
        "{}_",
        base.file_name()
            .unwrap()
            .to_string_lossy()
            .replace('-', "_")
    );
    let mut configuration = match std::env::var("OPENRUST_USER_KIT_DATABASE")
        .as_deref()
        .unwrap_or("sqlite")
    {
        "sqlite" => {
            json!({"default":"acceptance","connections":{"acceptance":{"driver":"sqlite","database":":memory:","prefix":prefix}}})
        }
        driver @ ("mysql" | "pgsql") => {
            json!({"default":"acceptance","connections":{"acceptance":{"driver":driver,"prefix":prefix,"prefix_indexes":true,"url":std::env::var("OPENRUST_USER_KIT_DATABASE_URL").expect("必须提供独立验收数据库URL")}}})
        }
        driver => panic!("不支持的用户套件验收数据库：{driver}"),
    };
    if let Ok(url) = std::env::var("OPENRUST_USER_KIT_REDIS_URL") {
        configuration["redis"] =
            json!({"client":"redis","default":{"url":url},"cache":{"url":url}});
    }
    configuration
}
async fn start() -> Server {
    start_profile("full").await
}
async fn start_profile(profile: &'static str) -> Server {
    start_options(profile, true).await
}
async fn start_options(profile: &'static str, verification: bool) -> Server {
    use axum::handler::Handler as _;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = Root(std::env::temp_dir().join(format!(
        "user-starter-http-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&root.0).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(root.0.join("app-url"), format!("http://{address}")).unwrap();
    std::fs::write(
        root.0.join("user-config.json"),
        json!({"profile":profile,"registration":true,"email_verification":verification})
            .to_string(),
    )
    .unwrap();
    let mut app = Application::configure(root.0.clone())
        .with_config(if profile == "api" {
            api_configuration
        } else {
            full_configuration
        })
        .with_providers(vec![
            Box::new(user_provider::UserServiceProvider),
            Box::new(openrust_fortify::FortifyServiceProvider),
            Box::new(installed_provider::FortifyServiceProvider),
        ])
        .with_routing(openrust::foundation::configuration::Routing::default().web(fixture_routes))
        .create()
        .unwrap();
    let package = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "layout.blade.html",
        "home.blade.html",
        "auth/login.blade.html",
        "auth/mail-login.blade.html",
        "auth/register.blade.html",
        "auth/forgot-password.blade.html",
        "auth/reset-password.blade.html",
        "auth/confirm-password.blade.html",
        "auth/verify-email.blade.html",
        "auth/two-factor-challenge.blade.html",
    ] {
        let target = root.0.join("resources/views/user").join(relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(package.join("stubs/starter/views").join(relative), target).unwrap();
    }
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let target = root.0.join(format!("lang/{locale}/user.json"));
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(
            package.join(format!("stubs/starter/lang/{locale}.json")),
            target,
        )
        .unwrap();
    }
    app.boot().await.unwrap();
    let auth = app.make::<AuthManager>().await.unwrap();
    #[cfg(feature = "sanctum")]
    auth.register_sanctum_model::<app::models::User>("StarterUser");
    #[cfg(not(feature = "sanctum"))]
    auth.register_model::<app::models::User>("StarterUser");
    let db = app
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let version_sql = match db.get_driver_name() {
        "mysql" => "SELECT VERSION() AS version",
        "pgsql" => "SELECT version() AS version",
        _ => "SELECT sqlite_version() AS version",
    };
    let version = db.select_one(version_sql, &[]).await.unwrap().unwrap();
    eprintln!(
        "用户套件实际数据库 {} {:?}",
        db.get_driver_name(),
        version.get("version")
    );
    if db.get_driver_name() == "mysql" {
        assert!(
            matches!(version.get("version"),Some(openrust::database::Value::Text(value)) if value.starts_with("8.4."))
        );
    }
    if db.get_driver_name() == "pgsql" {
        assert!(
            matches!(version.get("version"),Some(openrust::database::Value::Text(value)) if value.starts_with("PostgreSQL 18."))
        );
    }
    let mut schema = db.get_schema_builder();
    schema
        .create("users", |table| -> openrust::database::DatabaseResult<()> {
            table.id();
            table.string("name", None);
            table.string("email", None).unique();
            table.timestamp("email_verified_at", 0).nullable();
            table.string("password", None);
            table.remember_token();
            table.big_integer("credential_version").default(0);
            table.timestamp("created_at", None).nullable();
            table.timestamp("updated_at", None).nullable();
            Ok(())
        })
        .await
        .unwrap();
    #[cfg(feature = "sanctum")]
    schema
        .create("personal_access_tokens", |table| {
            table.id();
            table.string("tokenable_type", 255);
            table.unsigned_big_integer("tokenable_id");
            table.text("name");
            table.string("token", 64).unique();
            table.text("abilities");
            table.string("credential_stamp", 64).nullable();
            table.timestamp("last_used_at", 0).nullable();
            table.timestamp("expires_at", 0).nullable();
            table.timestamps();
        })
        .await
        .unwrap();
    schema
        .create(
            "password_reset_tokens",
            |table| -> openrust::database::DatabaseResult<()> {
                table.string("email", None).primary();
                table.string("token", None);
                table.timestamp("created_at", None).nullable();
                Ok(())
            },
        )
        .await
        .unwrap();
    schema
        .create(
            "sessions",
            |table| -> openrust::database::DatabaseResult<()> {
                table.string("id", None).primary();
                table.foreign_id("user_id").nullable().index();
                table.string("ip_address", 45).nullable();
                table.text("user_agent").nullable();
                table.long_text("payload");
                table.integer("last_activity").index();
                Ok(())
            },
        )
        .await
        .unwrap();
    let mailer = app
        .make::<openrust::mail::MailManager>()
        .await
        .unwrap()
        .mailer(None)
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    Server {
        address,
        mailer,
        task: tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }),
        _root: root,
    }
}
struct Client {
    address: SocketAddr,
    cookies: BTreeMap<String, String>,
    csrf: String,
}
impl Client {
    async fn send(&mut self, method: &str, path: &str, body: &str, json: bool) -> (u16, String) {
        let (status, _, body) = self.exchange(method, path, body, json).await;
        (status, body)
    }
    async fn exchange(
        &mut self,
        method: &str,
        path: &str,
        body: &str,
        json: bool,
    ) -> (u16, String, String) {
        tokio::time::timeout(std::time::Duration::from_secs(10),async{
            let mut socket=TcpStream::connect(self.address).await.unwrap();
            let cookies=self.cookies.iter().map(|(k,v)|format!("{k}={v}")).collect::<Vec<_>>().join("; ");
            let accept=if json{"application/json"}else{"text/html"};
            let wire=format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: {accept}\r\nContent-Type: application/json\r\nX-CSRF-TOKEN: {}\r\nCookie: {cookies}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",self.address,self.csrf,body.len());
            socket.write_all(wire.as_bytes()).await.unwrap();let mut wire=String::new();socket.read_to_string(&mut wire).await.unwrap();
            let (headers,body)=wire.split_once("\r\n\r\n").unwrap();
            let status=headers.split_whitespace().nth(1).unwrap().parse().unwrap();
            for line in headers.lines(){if let Some(cookie)=line.strip_prefix("set-cookie: "){let(name,value)=cookie.split(';').next().unwrap().split_once('=').unwrap();self.cookies.insert(name.into(),value.into());}}
            if let Some((_,tail))=body.split_once("name=\"_token\" value=\""){self.csrf=tail.split('"').next().unwrap().into();}
            (status,headers.into(),body.into())
        }).await.unwrap()
    }
}
#[tokio::test]
async fn installed_blade_forms_and_real_database_registration_login_and_logout() {
    let server = start().await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    for (path, label) in [
        ("/login", "登录"),
        ("/register", "注册账户"),
        ("/forgot-password", "忘记密码"),
    ] {
        let (status, body) = client.send("GET", path, "", false).await;
        assert_eq!(status, 200, "{path}: {body}");
        assert!(body.contains(label), "{body}");
        assert!(body.contains("用户套件测试"));
        assert!(!client.csrf.is_empty());
    }
    let (status,body)=client.send("POST","/register",r#"{"name":"开发者","email":"developer@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["data"]["email"], "developer@example.test");
    assert!(value["data"].get("password").is_none());
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("开发者"));
    assert!(body.contains("验证邮箱"), "{body}");
    let (status, body) = client.send("POST", "/logout", "{}", true).await;
    assert_eq!(status, 204, "{body}");
    assert!(body.is_empty());
    let (status, body) = client.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = client
        .send(
            "POST",
            "/login",
            r#"{"email":"developer@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
}

fn fixture_routes() -> Result<(), openrust::routing::RoutingError> {
    #[cfg(feature = "sanctum")]
    {
        openrust::support::facades::Route::post("/fixture-token", native_token)?;
        openrust::support::facades::Route::get_with("/api/token-me", token_user, |route| {
            route.middleware_name("auth:sanctum");
            Ok(())
        })?;
    }

    openrust::support::facades::Route::get("/old-input", old_input)?;
    openrust::support::facades::Route::post("/forget-fingerprint", forget_fingerprint)?;
    openrust::support::facades::Route::post("/expired-verification", expired_verification)
}
fn old_input(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::contracts::session::Session as _;
    Box::pin(async move {
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            request
                .session()?
                .get("_old_input")
                .cloned()
                .unwrap_or(Value::Null),
        )
    })
}
#[tokio::test]
async fn validation_redirect_keeps_public_input_without_authentication_secrets() {
    let server = start().await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let (status, body) = client.send("GET", "/register", "", false).await;
    assert_eq!(status, 200, "{body}");
    let payload = json!({"name":"允许回填","email":"invalid-email","password":"private-password","password_confirmation":"mismatch","token":"reset-secret","code":"otp-secret","recovery_code":"private-recovery","credential":{"response":{"authenticator_data":"private-data"}}});
    let (status, body) = client
        .send("POST", "/register", &payload.to_string(), false)
        .await;
    assert_eq!(status, 302, "{body}");
    let (status, body) = client.send("GET", "/old-input", "", true).await;
    assert_eq!(status, 200, "{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["name"], "允许回填");
    for field in [
        "password",
        "password_confirmation",
        "token",
        "code",
        "recovery_code",
        "credential",
    ] {
        assert!(
            value.get(field).is_none(),
            "秘密字段 {field} 被闪存: {value}"
        );
    }
}

#[tokio::test]
async fn api_preset_email_pages_use_native_auth_without_a_dashboard() {
    let server = start_profile("api").await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let (status, body) = client.send("GET", "/email/verify", "", false).await;
    assert_eq!(status, 302, "{body}");
    let (status, body) = client.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("/forgot-password"));
    assert!(!body.contains("/register"));
    let(status,body)=client.send("POST","/register",r#"{"name":"邮件用户","email":"landing@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = client.send("GET", "/email/verify", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("重发验证邮件"), "{body}");
    let (status, body) = client.send("POST", "/logout", "{}", true).await;
    assert_eq!(status, 204, "{body}");
    let (status, body) = client
        .send(
            "GET",
            "/reset-password/example?email=landing%40example.test",
            "",
            false,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("landing@example.test"), "{body}");
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 404, "{body}");
}

fn full_configuration(
    base: &std::path::Path,
    _env: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok([
        ("app".into(),json!({"key":"a".repeat(32),"url":std::fs::read_to_string(base.join("app-url")).unwrap(),"env":"local","name":"用户套件测试","version":"1.2.3","locale":"zh-Hans","supported_locales":[],"debug":true})),
        ("user".into(),serde_json::from_slice::<Value>(&std::fs::read(base.join("user-config.json")).unwrap()).unwrap()),
        ("database".into(),database_configuration(base)),
        ("session".into(),session_configuration(base)),
        ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"},"redis":{"driver":"redis","connection":"default","prefix":format!("{}:",base.file_name().unwrap().to_string_lossy())}}})),
        ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
        ("auth".into(),json!({"defaults":{"guard":"web","passwords":"users"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"StarterUser"}},"passwords":{"users":{"provider":"users","table":"password_reset_tokens","expire":60,"throttle":60}}})),
        ("mail".into(),json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"admin@example.test","name":"用户套件测试"}})),
    ].into())
}
fn api_configuration(
    base: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    let mut config = full_configuration(base, env)?;
    config.get_mut("user").unwrap()["profile"] = json!("api");
    Ok(config)
}

#[tokio::test]
async fn native_csrf_and_login_rate_limits_remain_enforced() {
    let server = start().await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let input = r#"{"email":"throttled@example.test","password":"wrong-password"}"#;
    let (status, body) = client.send("POST", "/login", input, true).await;
    assert_eq!(status, 419, "{body}");
    let (status, body) = client.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    for attempt in 0..6 {
        let (status, body) = client.send("POST", "/login", input, true).await;
        assert_eq!(
            status,
            if attempt < 5 { 422 } else { 429 },
            "attempt {attempt}: {body}"
        );
    }
}

impl Server {
    fn mail_link(&self, kind: &str) -> String {
        let messages = self.mailer.sent().expect("验收必须使用可捕获传输");
        for message in messages.iter().rev() {
            for tail in message
                .message
                .get_html_body()
                .unwrap_or_default()
                .split("href=\"")
                .skip(1)
            {
                let url = tail.split('"').next().unwrap().replace("&amp;", "&");
                if url.contains(kind) {
                    return url
                        .strip_prefix(&format!("http://{}", self.address))
                        .expect("邮件链接必须采用应用实际Origin")
                        .into();
                }
            }
        }
        panic!("没有捕获到指定用途的邮件链接：{kind}")
    }
}
#[tokio::test]
async fn verification_mail_uses_signed_links_and_repeated_verification_is_idempotent() {
    let server = start().await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let (status, body) = client.send("GET", "/register", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=client.send("POST","/register",r#"{"name":"验证用户","email":"verify@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(server.mailer.sent().unwrap().len(), 1);
    let link = server.mail_link("/email/verify/");
    let (status, body) = client
        .send("GET", &format!("{link}&tampered=1"), "", true)
        .await;
    assert_eq!(status, 403, "{body}");
    for _ in 0..2 {
        let (status, body) = client.send("GET", &link, "", true).await;
        assert_eq!(status, 204, "{body}");
        assert!(body.is_empty());
    }
    assert_eq!(server.mailer.sent().unwrap().len(), 1);
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = client
        .send(
            "PUT",
            "/user/profile-information",
            r#"{"name":"验证用户","email":"changed-verify@example.test"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let new_link = server.mail_link("/email/verify/");
    assert_ne!(new_link, link);
    let (status, body) = client.send("GET", &link, "", true).await;
    assert_eq!(status, 403, "旧邮箱链接仍可验证: {body}");
    let (status, body) = client.send("GET", &new_link, "", true).await;
    assert_eq!(status, 204, "{body}");
}

#[tokio::test]
async fn password_reset_mail_is_single_use_and_invalidates_sessions_and_remember_cookies() {
    let server = start().await;
    let make_client = || Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let mut first = make_client();
    let (status, body) = first.send("GET", "/register", "", false).await;
    assert_eq!(status, 200, "{body}");
    let credentials = r#"{"name":"找回用户","email":"reset@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#;
    let (status, body) = first.send("POST", "/register", credentials, true).await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = first.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    let mut second = make_client();
    let (status, body) = second.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = second
        .send(
            "POST",
            "/login",
            r#"{"email":"reset@example.test","password":"long-password-123","remember":true}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let mut remembered = make_client();
    remembered.cookies = second
        .cookies
        .iter()
        .filter(|(key, _)| key.starts_with("remember_"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert!(!remembered.cookies.is_empty());
    let mut reset = make_client();
    let (status, body) = reset.send("GET", "/forgot-password", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = reset
        .send(
            "POST",
            "/forgot-password",
            r#"{"email":"reset@example.test"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let link = server.mail_link("/reset-password/");
    let token = link.split('?').next().unwrap().rsplit('/').next().unwrap();
    let (status, body) = reset.send("GET", &link, "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("reset@example.test"));
    let input=json!({"token":token,"email":"reset@example.test","password":"new-password-456","password_confirmation":"new-password-456"}).to_string();
    let mut concurrent = Client {
        address: server.address,
        cookies: reset.cookies.clone(),
        csrf: reset.csrf.clone(),
    };
    let (first_result, second_result) = tokio::join!(
        reset.send("POST", "/reset-password", &input, true),
        concurrent.send("POST", "/reset-password", &input, true)
    );
    let mut statuses = [first_result.0, second_result.0];
    statuses.sort();
    assert_eq!(
        statuses,
        [200, 422],
        "重置令牌未被一次性兑换: {:?} {:?}",
        first_result,
        second_result
    );
    let (status, body) = reset.send("POST", "/reset-password", &input, true).await;
    assert_eq!(status, 422, "{body}");
    for (label, client) in [
        ("first", &mut first),
        ("second", &mut second),
        ("remembered", &mut remembered),
    ] {
        let (status, body) = client.send("GET", "/home", "", false).await;
        assert_eq!(status, 302, "{label} 旧凭据仍可访问账户: {status} {body}");
    }
    let (status, body) = reset.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = reset
        .send(
            "POST",
            "/login",
            r#"{"email":"reset@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(status, 422, "{body}");
    let (status, body) = reset
        .send(
            "POST",
            "/login",
            r#"{"email":"reset@example.test","password":"new-password-456"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn disabling_verification_preserves_registration_and_profile_updates_without_mail() {
    let server = start_options("full", false).await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let (status, body) = client.send("GET", "/register", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=client.send("POST","/register",r#"{"name":"免验证用户","email":"noverify@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    assert!(server.mailer.sent().unwrap().is_empty());
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = client
        .send(
            "PUT",
            "/user/profile-information",
            r#"{"name":"新名称","email":"changed@example.test"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(server.mailer.sent().unwrap().is_empty());
    let (status, body) = client.send("POST", "/logout", "{}", true).await;
    assert_eq!(status, 204, "{body}");
    let (status, body) = client.send("GET", "/forgot-password", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = client
        .send(
            "POST",
            "/forgot-password",
            r#"{"email":"changed@example.test"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(server.mailer.sent().unwrap().len(), 1);
    server.mail_link("/reset-password/");
}

#[tokio::test]
async fn password_update_preserves_this_session_and_rejects_an_unvisited_other_session() {
    let server = start().await;
    let make_client = || Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let mut first = make_client();
    let (status, body) = first.send("GET", "/register", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=first.send("POST","/register",r#"{"name":"密码用户","email":"update@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = first.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    let mut second = make_client();
    let (status, body) = second.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = second
        .send(
            "POST",
            "/login",
            r#"{"email":"update@example.test","password":"long-password-123","remember":true}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let mut remembered = make_client();
    remembered.cookies = second
        .cookies
        .iter()
        .filter(|(key, _)| key.starts_with("remember_"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert!(!remembered.cookies.is_empty());
    let(status,body)=first.send("PUT","/user/password",r#"{"current_password":"long-password-123","password":"new-password-456","password_confirmation":"new-password-456"}"#,true).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = first.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "修改密码导致本人退出: {body}");
    for client in [&mut second, &mut remembered] {
        let (status, body) = client.send("GET", "/home", "", false).await;
        assert_eq!(status, 302, "旧凭据仍可访问: {body}");
    }
}

// 仅供故障与过期验收的私有夹具路由，不发布到应用。
fn forget_fingerprint(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::contracts::session::Session as _;
    Box::pin(async move {
        openrust::auth::Auth::user(request)
            .await?
            .ok_or(openrust::http::HttpError::Unauthenticated)?;
        request.session_mut()?.forget(&["password_hash_web"]);
        Ok(openrust::http::Response::no_content())
    })
}
fn expired_verification(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        openrust::support::facades::Config::set("auth.verification.expire", json!(0))?;
        let user = openrust::auth::Auth::user(request).await?.unwrap();
        user.as_email_verification()
            .unwrap()
            .send_email_verification_notification()
            .await?;
        Ok(openrust::http::Response::no_content())
    })
}
#[tokio::test]
async fn valid_remember_recovery_establishes_a_fingerprint_and_missing_proofs_require_login() {
    let server = start().await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let (status, body) = client.send("GET", "/register", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=client.send("POST","/register",r#"{"name":"指纹用户","email":"fingerprint@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = client.send("POST", "/logout", "{}", true).await;
    assert_eq!(status, 204, "{body}");
    let (status, body) = client.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=client.send("POST","/login",r#"{"email":"fingerprint@example.test","password":"long-password-123","remember":true}"#,true).await;
    assert_eq!(status, 200, "{body}");
    client.cookies.retain(|key, _| key.starts_with("remember_"));
    client.csrf.clear();
    assert!(!client.cookies.is_empty());
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "有效remember恢复失败: {body}");
    let (status, body) = client.send("POST", "/forget-fingerprint", "{}", true).await;
    assert_eq!(status, 204, "{body}");
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 302, "缺少指纹仍被信任: {body}");
}
#[tokio::test]
async fn expired_captured_verification_mail_cannot_verify_the_account() {
    let server = start().await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let (status, body) = client.send("GET", "/register", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=client.send("POST","/register",r#"{"name":"过期用户","email":"expired@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = client.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = client
        .send("POST", "/expired-verification", "{}", true)
        .await;
    assert_eq!(status, 204, "{body}");
    let link = server.mail_link("/email/verify/");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let (status, body) = client.send("GET", &link, "", true).await;
    assert_eq!(status, 403, "过期链接仍可验证: {body}");
}

fn session_configuration(base: &std::path::Path) -> Value {
    let driver =
        std::env::var("OPENRUST_USER_KIT_SESSION_DRIVER").unwrap_or_else(|_| "array".into());
    assert!(
        matches!(driver.as_str(), "array" | "file" | "database" | "redis"),
        "不支持的会话验收驱动：{driver}"
    );
    if driver == "redis" {
        std::env::var("OPENRUST_USER_KIT_REDIS_URL").expect("Redis会话验收必须明确提供独占实例URL");
    }
    eprintln!("用户套件实际会话驱动 {driver}");
    json!({"driver":driver,"cookie":"user_session","lottery":[0,100],"table":"sessions","files":base.join("sessions"),"prefix":format!("{}:",base.file_name().unwrap().to_string_lossy())})
}

#[tokio::test]
async fn api_mail_browser_login_keeps_the_original_signature_and_renders_a_receipt() {
    let server = start_profile("api").await;
    let make_client = || Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let mut owner = make_client();
    let (status, body) = owner.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=owner.send("POST","/register",r#"{"name":"邮件落地用户","email":"browser@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = owner
        .send("GET", "/email/verify?verified=1", "", false)
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("重发验证邮件"));
    assert!(!body.contains("你的邮箱已验证"));
    let link = server.mail_link("/email/verify/");
    let mut browser = make_client();
    let (status, headers, body) = browser.exchange("GET", &link, "", false).await;
    assert_eq!(status, 302, "{body}");
    assert!(response_location(&headers).ends_with("/login"));
    let (status, body) = browser.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let (status, headers, body) = browser
        .exchange(
            "POST",
            "/login",
            r#"{"email":"browser@example.test","password":"long-password-123"}"#,
            false,
        )
        .await;
    assert_eq!(status, 302, "{body}");
    assert_eq!(
        response_location(&headers),
        format!("http://{}{}", server.address, link)
    );
    let (status, headers, body) = browser.exchange("GET", &link, "", false).await;
    assert_eq!(status, 302, "{body}");
    assert!(
        response_location(&headers).ends_with("/email/verify?verified=1"),
        "{headers}"
    );
    let (status, body) = browser.send("GET", "/email/verify", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("你的邮箱已验证"), "{body}");
    assert!(
        !body.contains("verification-notification"),
        "验证后仍显示重发邮件: {body}"
    );
}
fn response_location(headers: &str) -> &str {
    headers
        .lines()
        .find_map(|line| line.strip_prefix("location: "))
        .expect("必须有重定向目标")
}

#[cfg(feature = "sanctum")]
#[test]
fn published_credential_migrations_implement_native_migration_contract() {
    fn migration(_: impl openrust::database::migrations::Migration) {}
    migration(user_credential_migration::Definition);
    migration(token_credential_migration::Definition);
}

#[cfg(feature = "sanctum")]
fn native_token(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{
            database::eloquent::Model as _,
            support::facades::{App, Auth, Config, Facade as _},
        };
        let user = Auth::user(request)
            .await?
            .ok_or(openrust::http::HttpError::Unauthenticated)?;
        let user = (user.as_ref() as &dyn std::any::Any)
            .downcast_ref::<app::models::User>()
            .unwrap();
        let mut user = app::models::User::from_state(user.state().clone());
        let token = openrust_fortify::starter::credentials::issue(
            &mut user,
            App::make::<openrust::sanctum::Sanctum>().await?.as_ref(),
            App::make::<openrust::database::DatabaseManager>()
                .await?
                .as_ref(),
            Config::get_facade_root()?.as_ref(),
            "mobile",
            &["account:read"],
        )
        .await?;
        openrust::http::ApiResponse::success(token.to_array()?)
            .with_status(openrust::http::StatusCode::CREATED)
            .into_response()
    })
}
#[cfg(feature = "sanctum")]
fn token_user(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let user = openrust::support::facades::Auth::user(request)
            .await?
            .ok_or(openrust::http::HttpError::Unauthenticated)?;
        let token = openrust::sanctum::AccessToken::current_access_token(request);
        assert!(matches!(
            token,
            Some(openrust::sanctum::AccessToken::Personal(_))
        ));
        openrust::http::ApiResponse::success(json!({"id":user.get_auth_identifier()}))
            .into_response()
    })
}
#[cfg(feature = "sanctum")]
async fn bearer_request(address: SocketAddr, token: &str) -> u16 {
    tokio::time::timeout(std::time::Duration::from_secs(10),async{
        let mut socket=TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("GET /api/token-me HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut wire=String::new();socket.read_to_string(&mut wire).await.unwrap();
        wire.split_whitespace().nth(1).unwrap().parse().unwrap()
    }).await.unwrap()
}
#[cfg(feature = "sanctum")]
#[tokio::test]
async fn password_update_revokes_bearer_and_allows_fresh_issuance() {
    let server = start().await;
    let mut client = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    assert_eq!(client.send("GET", "/register", "", false).await.0, 200);
    let registration = r#"{"name":"令牌用户","email":"token@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#;
    let (status, body) = client.send("POST", "/register", registration, true).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(client.send("GET", "/home", "", false).await.0, 200);
    let (status, body) = client.send("POST", "/fixture-token", "{}", true).await;
    assert_eq!(status, 201, "{body}");
    let payload: Value = serde_json::from_str(&body).unwrap();
    let token = payload["data"]["plainTextToken"].as_str().unwrap();
    assert!(payload["data"]["accessToken"].get("token").is_none());
    assert!(
        payload["data"]["accessToken"]
            .get("credential_stamp")
            .is_none()
    );
    assert_eq!(bearer_request(server.address, token).await, 200);
    let update = r#"{"current_password":"long-password-123","password":"changed-password-456","password_confirmation":"changed-password-456"}"#;
    let (status, body) = client.send("PUT", "/user/password", update, true).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(bearer_request(server.address, token).await, 401);
    let (status, body) = client.send("POST", "/fixture-token", "{}", true).await;
    assert_eq!(status, 201, "{body}");
    let payload: Value = serde_json::from_str(&body).unwrap();
    let renewed = payload["data"]["plainTextToken"].as_str().unwrap();
    assert_ne!(token, renewed);
    assert_eq!(bearer_request(server.address, renewed).await, 200);
}
