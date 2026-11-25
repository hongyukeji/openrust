#[cfg(feature = "sanctum")]
#[path = "../stubs/starter/migrations/auth-challenges.rs"]
mod auth_challenge_migration;
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
    pub(crate) mod http {
        pub(crate) mod resources {
            pub(crate) mod user_resource {
                include!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/stubs/starter/user_resource.rs"
                ));
            }
        }
    }
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
    _kernel: Arc<Kernel>,
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
    start_security(profile, verification, false, "api").await
}
async fn start_security(
    profile: &'static str,
    verification: bool,
    two_factor: bool,
    prefix: &str,
) -> Server {
    start_security_catalog(profile, verification, two_factor, prefix, false).await
}
async fn start_security_catalog(
    profile: &'static str,
    verification: bool,
    two_factor: bool,
    prefix: &str,
    legacy: bool,
) -> Server {
    start_security_catalog_mail(profile, verification, two_factor, prefix, legacy, None).await
}
async fn start_security_catalog_mail(
    profile: &'static str,
    verification: bool,
    two_factor: bool,
    prefix: &str,
    legacy: bool,
    mail: Option<Value>,
) -> Server {
    start_custom_resource(
        profile,
        verification,
        two_factor,
        prefix,
        legacy,
        mail,
        false,
    )
    .await
}
async fn start_custom_resource(
    profile: &'static str,
    verification: bool,
    two_factor: bool,
    prefix: &str,
    legacy: bool,
    mail: Option<Value>,
    custom: bool,
) -> Server {
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
        json!({"profile":profile,"registration":true,"email_verification":verification,"two_factor":two_factor,"api_prefix":prefix})
            .to_string(),
    )
    .unwrap();
    if let Some(mail) = mail {
        std::fs::write(root.0.join("mail-config.json"), mail.to_string()).unwrap();
    }
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
        .with_middleware(|_middleware| {
            #[cfg(feature = "sanctum")]
            _middleware.stateful_api();
        })
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
        let original =
            std::fs::read_to_string(package.join(format!("stubs/starter/lang/{locale}.json")))
                .unwrap();
        if legacy {
            let mut values: Value = serde_json::from_str(&original).unwrap();
            values.as_object_mut().unwrap().remove("challenge_required");
            values.as_object_mut().unwrap().remove("challenges_pruned");
            values
                .as_object_mut()
                .unwrap()
                .remove("confirmation_required");
            values["login"] = json!("自定义登录");
            std::fs::write(target, serde_json::to_vec(&values).unwrap()).unwrap();
        } else {
            std::fs::write(target, original).unwrap();
        }
    }
    app.boot().await.unwrap();
    if custom {
        openrust_fortify::starter::UserResource::use_resource::<app::models::User, CustomResource>(
            &app,
        )
        .unwrap();
    }

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
            table.text("two_factor_secret").nullable();
            table.text("two_factor_recovery_codes").nullable();
            table.timestamp("two_factor_confirmed_at", 0).nullable();
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
    #[cfg(feature = "sanctum")]
    schema
        .create("user_auth_challenges", |table| {
            table.string("id", 64).primary();
            table.string("purpose", 64);
            table.string("owner_hash", 64).index();
            table.text("payload");
            table.big_integer("created_at");
            table.big_integer("expires_at").index();
            table.big_integer("consumed_at").nullable();
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
    let kernel = Arc::new(kernel);
    let service = transport.with_state(kernel.clone()).into_make_service();
    Server {
        address,
        mailer,
        task: tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }),
        _root: root,
        _kernel: kernel,
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
        self.exchange_origin(method, path, body, json, false).await
    }
    async fn exchange_origin(
        &mut self,
        method: &str,
        path: &str,
        body: &str,
        json: bool,
        origin: bool,
    ) -> (u16, String, String) {
        tokio::time::timeout(std::time::Duration::from_secs(10),async{
            let mut socket=TcpStream::connect(self.address).await.unwrap();
            let cookies=self.cookies.iter().map(|(k,v)|format!("{k}={v}")).collect::<Vec<_>>().join("; ");
            let origin=if origin {format!("Origin: http://{}\r\n",self.address)}else{String::new()};
            let accept=if json{"application/json"}else{"text/html"};
            let wire=format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n{origin}Accept: {accept}\r\nContent-Type: application/json\r\nX-CSRF-TOKEN: {}\r\nCookie: {cookies}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",self.address,self.csrf,body.len());
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
    openrust::support::facades::Route::post("/admin-fixture-login", admin_fixture_login)?;
    openrust::support::facades::Route::get_with(
        "/admin-fixture-user",
        admin_fixture_user,
        |route| {
            route
                .middleware_name("auth:admin")
                .middleware_name("auth.session");
            Ok(())
        },
    )?;
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
    let payload = json!({"name":"允许回填","email":"invalid-email","password":"private-password","password_confirmation":"mismatch","token":"reset-secret","code":"otp-secret","recovery_code":"private-recovery","credential":{"response":{"authenticator_data":"private-data"}},"proof":"private-proof"});
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
        "proof",
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
        ("api".into(),json!({"prefix":serde_json::from_slice::<Value>(&std::fs::read(base.join("user-config.json")).unwrap()).unwrap()["api_prefix"],"response_envelope":true})),
        ("database".into(),database_configuration(base)),
        ("session".into(),session_configuration(base)),
        ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"},"redis":{"driver":"redis","connection":"default","prefix":format!("{}:",base.file_name().unwrap().to_string_lossy())}}})),
        ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
        ("auth".into(),json!({"defaults":{"guard":"web","passwords":"users"},"guards":{"web":{"driver":"session","provider":"users"},"admin":{"driver":"session","provider":"admins"}},"providers":{"users":{"driver":"eloquent","model":"StarterUser"},"admins":{"driver":"database","table":"admins"}},"passwords":{"users":{"provider":"users","table":"password_reset_tokens","expire":60,"throttle":60}}})),
        ("fortify-options".into(),json!({"two-factor-authentication":{"confirm":true,"confirmPassword":true}})),
        ("mail".into(),std::fs::read(base.join("mail-config.json")).ok().map(|bytes|serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap_or_else(||json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"admin@example.test","name":"用户套件测试"}}))),
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
    migration(auth_challenge_migration::Definition);
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

#[cfg(feature = "sanctum")]
async fn token_client_request(
    address: SocketAddr,
    path: &str,
    payload: Value,
) -> (u16, String, Value) {
    let mut client = Client {
        address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    let (status, headers, body) = client
        .exchange("POST", path, &payload.to_string(), true)
        .await;
    assert!(
        client.cookies.is_empty(),
        "无Cookie令牌客户端不应创建浏览器会话"
    );
    (status, headers, serde_json::from_str(&body).unwrap())
}
#[cfg(feature = "sanctum")]
async fn register_token_user(server: &Server) {
    let mut client = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(client.send("GET", "/register", "", false).await.0, 200);
    let(status,body)=client.send("POST","/register",r#"{"name":"独立客户端","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(
        status,
        201,
        "{}",
        token_failure(&serde_json::from_str(&body).unwrap_or(Value::Null))
    );
}
#[cfg(feature = "sanctum")]
fn device_login() -> Value {
    json!({"email":"device@example.test","password":"long-password-123","device_name":"phone"})
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn stateless_password_token_login_keeps_sessions_separate_and_obeys_api_prefix() {
    use openrust::support::facades::{App, Facade as _};
    let server = start_security("full", true, false, "api/admin").await;
    register_token_user(&server).await;
    let mut wrong = device_login();
    wrong["password"] = json!("incorrect");
    let (status, _, body) =
        token_client_request(server.address, "/api/admin/auth/token", wrong).await;
    assert_eq!(status, 422, "{}", token_failure(&body));
    let mut denied = device_login();
    denied["abilities"] = json!(["admin:all"]);
    let (status, _, body) =
        token_client_request(server.address, "/api/admin/auth/token", denied).await;
    assert_eq!(status, 422, "{}", token_failure(&body));
    assert!(!body.to_string().contains(":attribute"));
    let (status, headers, body) =
        token_client_request(server.address, "/api/admin/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    assert!(headers.contains("no-store"));
    assert_eq!(body["message"], "成功");
    assert_eq!(body["data"]["token_type"], "Bearer");
    assert!(body["data"]["expires_at"].is_string());
    assert_eq!(body["data"]["user"].as_object().unwrap().len(), 4);
    let token = body["data"]["token"].as_str().unwrap();
    assert_eq!(bearer_request(server.address, token).await, 200);
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let sanctum = App::make::<openrust::sanctum::Sanctum>().await.unwrap();
        let db = App::make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let stored = sanctum
            .personal_access_tokens(&db, "")
            .unwrap()
            .find_token(token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.abilities,
            vec!["account:read", "account:update", "tokens:manage"]
        );
        assert!(
            stored
                .get_attribute_value("credential_stamp")
                .unwrap()
                .is_some()
        );
        assert!(
            !serde_json::to_string(&stored)
                .unwrap()
                .contains("credential_stamp")
        );
    })
    .await;
    let (status, _, _) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 404);
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn stateless_factor_challenges_reject_replay_and_consume_recovery_codes_atomically() {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        database::{Value as DbValue, eloquent::Model as _},
        support::facades::{App, Facade as _},
    };
    let server = start_security("full", true, true, "api").await;
    register_token_user(&server).await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let fortify = app.make::<openrust_fortify::Fortify>().await.unwrap();
        let encrypter = fortify.current_encrypter().unwrap();
        let secret = encrypter.encrypt(b"JBSWY3DPEHPK3PXP", true).unwrap();
        let recovery = encrypter
            .encrypt(br#"["first-code","second-code","third-code"]"#, true)
            .unwrap();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        db.table("users")
            .r#where("email", "=", "device@example.test")
            .update(
                [
                    ("two_factor_secret".into(), secret.into()),
                    ("two_factor_recovery_codes".into(), recovery.into()),
                    (
                        "two_factor_confirmed_at".into(),
                        "2026-01-01 00:00:00".into(),
                    ),
                ]
                .into(),
            )
            .await
            .unwrap();
        let cache = app
            .make::<openrust::cache::CacheManager>()
            .await
            .unwrap()
            .store(None)
            .unwrap();
        app.container()
            .instance::<dyn openrust_fortify::contracts::TwoFactorAuthenticationProvider>(Arc::new(
                openrust_fortify::TwoFactorAuthenticationProvider::new(Some(cache))
                    .with_clock(|| Ok(1_234_567_890)),
            ))
            .unwrap();
    })
    .await;
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 200, "{}", token_failure(&body));
    assert_eq!(body["code"], "auth_challenge_required");
    assert!(body["data"].get("token").is_none());
    let challenge = body["data"]["challenge"].as_str().unwrap();
    assert!(body["data"]["expires_at"].is_string());
    assert_eq!(bearer_request(server.address, challenge).await, 401);
    let (status, _, body) = token_client_request(
        server.address,
        "/api/auth/token/challenge",
        json!({"challenge":challenge,"code":"bad"}),
    )
    .await;
    assert_eq!(status, 422, "{}", token_failure(&body));
    let (status, headers, body) = token_client_request(
        server.address,
        "/api/auth/token/challenge",
        json!({"challenge":challenge,"code":"742275"}),
    )
    .await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    assert!(headers.contains("no-store"));
    assert_eq!(
        bearer_request(server.address, body["data"]["token"].as_str().unwrap()).await,
        200
    );
    let (status, _, _) = token_client_request(
        server.address,
        "/api/auth/token/challenge",
        json!({"challenge":challenge,"code":"742275"}),
    )
    .await;
    assert_eq!(status, 401);
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let next = body["data"]["challenge"].as_str().unwrap();
    let (status, _, _) = token_client_request(
        server.address,
        "/api/auth/token/challenge",
        json!({"challenge":next,"code":"742275"}),
    )
    .await;
    assert_eq!(status, 422);
    let (status, _, body) = token_client_request(
        server.address,
        "/api/auth/token/challenge",
        json!({"challenge":next,"recovery_code":"first-code"}),
    )
    .await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let (_, _, a) = token_client_request(server.address, "/api/auth/token", device_login()).await;
    let (_, _, b) = token_client_request(server.address, "/api/auth/token", device_login()).await;
    let (a, b) = tokio::join!(
        token_client_request(
            server.address,
            "/api/auth/token/challenge",
            json!({"challenge":a["data"]["challenge"],"recovery_code":"second-code"})
        ),
        token_client_request(
            server.address,
            "/api/auth/token/challenge",
            json!({"challenge":b["data"]["challenge"],"recovery_code":"second-code"})
        )
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [201, 422], "{:?} {:?}", a.2, b.2);
    let (_, _, pending) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let mut query = app::models::User::query_on(&db).use_write_pdo();
        query.r#where("email", "=", "device@example.test");
        let mut user = query.first().await.unwrap().unwrap();
        openrust_fortify::starter::credentials::revoke_all(&mut user)
            .await
            .unwrap();
        assert!(matches!(
            user.get_attribute("credential_version"),
            Some(DbValue::Unsigned(1)) | Some(DbValue::Integer(1))
        ));
    })
    .await;
    let (status, _, body) = token_client_request(
        server.address,
        "/api/auth/token/challenge",
        json!({"challenge":pending["data"]["challenge"],"recovery_code":"third-code"}),
    )
    .await;
    assert_eq!(status, 401, "{}", token_failure(&body));
}

#[cfg(feature = "sanctum")]
fn token_failure(value: &Value) -> Value {
    let mut safe = value.clone();
    if let Some(data) = safe.get_mut("data").and_then(Value::as_object_mut) {
        for key in ["token", "challenge", "proof"] {
            if data.contains_key(key) {
                data.insert(key.into(), Value::String("[REDACTED]".into()));
            }
        }
    }
    safe
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn compiled_user_messages_preserve_overrides_and_artisan_prunes_challenges() {
    use openrust::support::facades::{App, Facade as _};
    let server = start_security_catalog("full", true, true, "api", true).await;
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    let (status, page) = browser.send("GET", "/login", "", false).await;
    assert_eq!(status, 200);
    assert!(page.contains("自定义登录"));
    register_token_user(&server).await;
    assert_eq!(
        browser
            .send(
                "POST",
                "/login",
                r#"{"email":"device@example.test","password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        200
    );
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    let (status, _, body) = browser
        .exchange_origin(
            "POST",
            "/api/user/tokens",
            r#"{"device_name":"legacy-browser"}"#,
            true,
            true,
        )
        .await;
    assert_eq!(status, 423);
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["message"], "请先确认身份，再继续操作。");
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let fortify = app.make::<openrust_fortify::Fortify>().await.unwrap();
        let secret = fortify
            .current_encrypter()
            .unwrap()
            .encrypt(b"JBSWY3DPEHPK3PXP", true)
            .unwrap();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        db.table("users")
            .r#where("email", "=", "device@example.test")
            .update(
                [
                    ("two_factor_secret".into(), secret.into()),
                    (
                        "two_factor_confirmed_at".into(),
                        "2026-01-01 00:00:00".into(),
                    ),
                ]
                .into(),
            )
            .await
            .unwrap();
    })
    .await;
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 200, "{}", token_failure(&body));
    assert_eq!(body["message"], "请完成额外身份验证。");
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let clock = app.make::<openrust::support::DateFactory>().await.unwrap();
        let instant = openrust::support::facades::Date::now(None)
            .unwrap()
            .as_date_time()
            .timestamp()
            .checked_add(jiff::SignedDuration::from_secs(301))
            .unwrap();
        clock.set_test_now(Some(openrust::support::Date::from_utc(
            instant,
            jiff::tz::TimeZone::UTC,
        )));
        let mut output = openrust::console::Output::default();
        assert_eq!(
            openrust::support::facades::Artisan::call(
                "auth:prune-challenges",
                &["--hours=0".into()],
                &mut output
            )
            .await
            .unwrap(),
            0,
            "{}",
            output.stderr
        );
        assert!(output.stdout.contains('1'));
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(
            db.table("user_auth_challenges")
                .use_write_pdo()
                .count("*")
                .await
                .unwrap(),
            0
        );
        let mut output = openrust::console::Output::default();
        assert_eq!(
            openrust::support::facades::Artisan::call(
                "auth:prune-challenges",
                &["--hours=0".into()],
                &mut output
            )
            .await
            .unwrap(),
            0,
            "{}",
            output.stderr
        );
        assert!(output.stdout.contains('0'));
        let mut output = openrust::console::Output::default();
        assert!(
            openrust::support::facades::Artisan::call(
                "auth:prune-challenges",
                &["--hours=-1".into()],
                &mut output
            )
            .await
            .is_err()
        );
    })
    .await;
}

#[cfg(feature = "sanctum")]
async fn account_request(
    address: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    payload: Value,
) -> (u16, String, Value) {
    account_request_with_proof(address, token, method, path, payload, None).await
}
#[cfg(feature = "sanctum")]
async fn account_request_with_proof(
    address: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    payload: Value,
    proof: Option<&str>,
) -> (u16, String, Value) {
    let result = account_wire_request(address, token, method, path, payload, proof).await;
    assert!(!result.1.to_ascii_lowercase().contains("set-cookie:"));
    result
}
#[cfg(feature = "sanctum")]
async fn account_wire_request(
    address: SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    payload: Value,
    proof: Option<&str>,
) -> (u16, String, Value) {
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        let mut socket=TcpStream::connect(address).await.unwrap();let body=payload.to_string();
        let extra=proof.map(|value|format!("X-Auth-Confirmation: {value}\r\n")).unwrap_or_default();
        socket.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nAuthorization: Bearer {token}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut wire=String::new();socket.read_to_string(&mut wire).await.unwrap();
        let(headers,body)=wire.split_once("\r\n\r\n").unwrap();let status=headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status,headers.to_owned(),if body.is_empty(){Value::Null}else{serde_json::from_str(body).unwrap()})
    }).await.unwrap()
}
#[cfg(feature = "sanctum")]
async fn confirmation(server: &Server, token: &str, purpose: &str) -> String {
    let (status, _, body) = account_request(
        server.address,
        token,
        "POST",
        "/api/auth/confirm",
        json!({"purpose":purpose,"password":"long-password-123"}),
    )
    .await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    body["data"]["proof"].as_str().unwrap().into()
}
#[cfg(feature = "sanctum")]
#[tokio::test]
async fn account_tokens_require_bound_single_use_confirmation_and_logout_only_current_token() {
    let server = start().await;
    register_token_user(&server).await;
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let first = body["data"]["token"].as_str().unwrap();
    let (status, _, body) = account_request(
        server.address,
        first,
        "GET",
        "/api/user/tokens",
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{}", token_failure(&body));
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["current"], true);
    for item in body["data"].as_array().unwrap() {
        for key in ["token", "credential_stamp", "tokenable_id"] {
            assert!(item.get(key).is_none());
        }
    }
    assert_eq!(
        account_request(
            server.address,
            first,
            "POST",
            "/api/user/tokens",
            json!({"device_name":"tablet"})
        )
        .await
        .0,
        401
    );
    assert_eq!(
        account_request(
            server.address,
            first,
            "POST",
            "/api/auth/confirm",
            json!({"purpose":"tokens","password":"wrong"})
        )
        .await
        .0,
        422
    );
    let proof = confirmation(&server, first, "tokens").await;
    let (status, _, second) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&second));
    let second = second["data"]["token"].as_str().unwrap();
    assert_eq!(
        account_request(
            server.address,
            second,
            "POST",
            "/api/user/tokens",
            json!({"device_name":"tablet","proof":proof})
        )
        .await
        .0,
        401
    );
    let (status, _, created) = account_request(
        server.address,
        first,
        "POST",
        "/api/user/tokens",
        json!({"device_name":"tablet","abilities":["account:read"],"proof":proof}),
    )
    .await;
    assert_eq!(status, 201, "{}", token_failure(&created));
    let child = created["data"]["token"].as_str().unwrap();
    assert_eq!(bearer_request(server.address, child).await, 200);
    assert_eq!(
        account_request(
            server.address,
            first,
            "POST",
            "/api/user/tokens",
            json!({"device_name":"replay","proof":proof})
        )
        .await
        .0,
        401
    );
    let wrong = confirmation(&server, first, "password").await;
    assert_eq!(
        account_request(
            server.address,
            first,
            "POST",
            "/api/user/tokens",
            json!({"device_name":"wrong-purpose","proof":wrong})
        )
        .await
        .0,
        401
    );
    assert_eq!(
        account_request(
            server.address,
            child,
            "GET",
            "/api/user/tokens",
            Value::Null
        )
        .await
        .0,
        403
    );
    let (status, headers, body) = account_request(
        server.address,
        child,
        "POST",
        "/api/auth/logout",
        json!({"token":first.split('|').next()}),
    )
    .await;
    assert_eq!(status, 204, "{}", token_failure(&body));
    assert!(body.is_null());
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    assert_eq!(bearer_request(server.address, child).await, 401);
    assert_eq!(bearer_request(server.address, first).await, 200);
    assert_eq!(
        account_request(
            server.address,
            child,
            "POST",
            "/api/auth/logout",
            Value::Null
        )
        .await
        .0,
        401
    );
    let proof = confirmation(&server, first, "tokens").await;
    let id = second.split('|').next().unwrap();
    let (status, _, body) = account_request(
        server.address,
        first,
        "DELETE",
        &format!("/api/user/tokens/{id}"),
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(status, 204, "{}", token_failure(&body));
    assert!(body.is_null());
    assert_eq!(bearer_request(server.address, second).await, 401);
    assert_eq!(bearer_request(server.address, first).await, 200);
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn account_confirmation_expires_and_one_concurrent_token_grant_wins() {
    use openrust::support::facades::{App, Facade as _};
    let server = start().await;
    register_token_user(&server).await;
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let token = body["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, token, "tokens").await;
    let payload = json!({"device_name":"concurrent","proof":proof});
    let (a, b) = tokio::join!(
        account_request(
            server.address,
            token,
            "POST",
            "/api/user/tokens",
            payload.clone()
        ),
        account_request(server.address, token, "POST", "/api/user/tokens", payload)
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [201, 401]);
    let proof = confirmation(&server, token, "tokens").await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        use openrust::database::eloquent::{Model as _, concerns::ModelEvent};
        let app = server._kernel.get_application();
        let clock = app.make::<openrust::support::DateFactory>().await.unwrap();
        let at = openrust::support::facades::Date::now(None)
            .unwrap()
            .as_date_time()
            .timestamp()
            .checked_add(jiff::SignedDuration::from_secs(10800))
            .unwrap();
        openrust_fortify::starter::credentials::CredentialToken::register_model_event(
            app,
            ModelEvent::Creating,
            ExpireTokenCreation { clock, at },
        )
        .unwrap();
    })
    .await;
    assert_eq!(
        account_request(
            server.address,
            token,
            "POST",
            "/api/user/tokens",
            json!({"device_name":"expire-while-creating","proof":proof})
        )
        .await
        .0,
        401
    );
    let (status, _, listed) = account_request(
        server.address,
        token,
        "GET",
        "/api/user/tokens",
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{}", token_failure(&listed));
    assert!(
        listed["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["name"] != "expire-while-creating")
    );
    let proof = confirmation(&server, token, "tokens").await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let clock = app.make::<openrust::support::DateFactory>().await.unwrap();
        let instant = openrust::support::facades::Date::now(None)
            .unwrap()
            .as_date_time()
            .timestamp()
            .checked_add(jiff::SignedDuration::from_secs(10800))
            .unwrap();
        clock.set_test_now(Some(openrust::support::Date::from_utc(
            instant,
            jiff::tz::TimeZone::UTC,
        )));
    })
    .await;
    assert_eq!(
        account_request(
            server.address,
            token,
            "POST",
            "/api/user/tokens",
            json!({"device_name":"expired","proof":proof})
        )
        .await
        .0,
        401
    );
    assert_eq!(bearer_request(server.address, token).await, 200);
}

#[cfg(feature = "sanctum")]
struct ExpireTokenCreation {
    clock: Arc<openrust::support::DateFactory>,
    at: jiff::Timestamp,
}
#[cfg(feature = "sanctum")]
impl
    openrust::database::eloquent::concerns::ModelEventListener<
        openrust_fortify::starter::credentials::CredentialToken,
    > for ExpireTokenCreation
{
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut openrust_fortify::starter::credentials::CredentialToken,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            use openrust::database::eloquent::Model as _;
            if model.get_attribute("name")
                == Some(&openrust::database::Value::Text(
                    "expire-while-creating".into(),
                ))
            {
                self.clock
                    .set_test_now(Some(openrust::support::Date::from_utc(
                        self.at,
                        jiff::tz::TimeZone::UTC,
                    )));
            }
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn account_proofs_reject_other_users_and_token_creation_cannot_expand_parent_abilities() {
    let server = start().await;
    register_token_user(&server).await;
    let (status, _, first) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&first));
    let first = first["data"]["token"].as_str().unwrap();
    let mut other = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(other.send("GET", "/register", "", false).await.0, 200);
    assert_eq!(other.send("POST","/register",r#"{"name":"其他用户","email":"other@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await.0,201);
    let mut login = device_login();
    login["email"] = json!("other@example.test");
    let (status, _, second) = token_client_request(server.address, "/api/auth/token", login).await;
    assert_eq!(status, 201, "{}", token_failure(&second));
    let second = second["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, second, "tokens").await;
    assert_eq!(
        account_request(
            server.address,
            first,
            "POST",
            "/api/user/tokens",
            json!({"device_name":"forged","proof":proof})
        )
        .await
        .0,
        401
    );
    let (status, _, created) = account_request_with_proof(
        server.address,
        second,
        "POST",
        "/api/user/tokens",
        json!({"device_name":"other-device"}),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 201, "{}", token_failure(&created));
    let foreign = created["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, first, "tokens").await;
    assert_eq!(
        account_request(
            server.address,
            first,
            "DELETE",
            &format!("/api/user/tokens/{}", foreign.split('|').next().unwrap()),
            json!({"proof":proof})
        )
        .await
        .0,
        404
    );
    assert_eq!(bearer_request(server.address, foreign).await, 200);
    let mut limited = device_login();
    limited["abilities"] = json!(["tokens:manage"]);
    let (status, _, limited) =
        token_client_request(server.address, "/api/auth/token", limited).await;
    assert_eq!(status, 201, "{}", token_failure(&limited));
    let limited = limited["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, limited, "tokens").await;
    let (status, _, body) = account_request(
        server.address,
        limited,
        "POST",
        "/api/user/tokens",
        json!({"device_name":"escalation","abilities":["account:read"],"proof":proof}),
    )
    .await;
    assert_eq!(status, 403, "{}", token_failure(&body));
    let (status, _, listed) = account_request(
        server.address,
        limited,
        "GET",
        "/api/user/tokens",
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{}", token_failure(&listed));
    assert!(
        listed["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["name"] != "escalation")
    );
    let proof = confirmation(&server, limited, "tokens").await;
    let (status, _, child) = account_request(
        server.address,
        limited,
        "POST",
        "/api/user/tokens",
        json!({"device_name":"restricted-default","proof":proof}),
    )
    .await;
    assert_eq!(status, 201, "{}", token_failure(&child));
    let (status, _, listed) = account_request(
        server.address,
        limited,
        "GET",
        "/api/user/tokens",
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{}", token_failure(&listed));
    let item = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "restricted-default")
        .unwrap();
    assert_eq!(item["abilities"], json!(["tokens:manage"]));
    for _ in 0..5 {
        assert_eq!(
            account_request(
                server.address,
                limited,
                "POST",
                "/api/auth/confirm",
                json!({"purpose":"tokens","password":"wrong"})
            )
            .await
            .0,
            422
        );
    }
    let (status, headers, _) = account_request(
        server.address,
        limited,
        "POST",
        "/api/auth/confirm",
        json!({"purpose":"tokens","password":"long-password-123"}),
    )
    .await;
    assert_eq!(status, 429);
    assert!(headers.to_ascii_lowercase().contains("retry-after"));
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn account_cookie_management_uses_native_session_confirmation_and_csrf() {
    use openrust::support::facades::Facade as _;
    let server = start_security("full", true, true, "api").await;
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(browser.send("GET", "/register", "", false).await.0, 200);
    assert_eq!(browser.send("POST","/register",r#"{"name":"浏览器确认","email":"browser@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await.0,201);
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    let (status, _, body) = browser
        .exchange_origin(
            "POST",
            "/api/user/tokens",
            r#"{"device_name":"browser-managed"}"#,
            true,
            true,
        )
        .await;
    assert_eq!(
        status,
        423,
        "{}",
        token_failure(&serde_json::from_str(&body).unwrap_or(Value::Null))
    );
    let csrf = std::mem::take(&mut browser.csrf);
    assert_eq!(
        browser
            .exchange_origin(
                "POST",
                "/api/auth/confirm",
                r#"{"purpose":"tokens","password":"long-password-123"}"#,
                true,
                true
            )
            .await
            .0,
        419
    );
    browser.csrf = csrf;
    openrust::support::facades::App::set_facade_application(
        Some(server._kernel.get_application()),
        async {
            let clock = server
                ._kernel
                .get_application()
                .make::<openrust::support::DateFactory>()
                .await
                .unwrap();
            let at = "2030-01-01T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
            clock.set_test_now(Some(openrust::support::Date::from_utc(
                at,
                jiff::tz::TimeZone::UTC,
            )));
        },
    )
    .await;
    // 沿用原生确认端点，不另外签发Cookie证明；Passkey确认写入同一Session标记。
    assert_eq!(
        browser
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let (status, _, body) = browser
        .exchange_origin(
            "POST",
            "/api/user/tokens",
            r#"{"device_name":"browser-managed"}"#,
            true,
            true,
        )
        .await;
    assert_eq!(
        status,
        201,
        "{}",
        token_failure(&serde_json::from_str(&body).unwrap_or(Value::Null))
    );
    let created: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        bearer_request(server.address, created["data"]["token"].as_str().unwrap()).await,
        200
    );
    let mut other = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(other.send("GET", "/login", "", false).await.0, 200);
    assert_eq!(
        other
            .send(
                "POST",
                "/login",
                r#"{"email":"browser@example.test","password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        200
    );
    assert_eq!(other.send("GET", "/home", "", false).await.0, 200);
    assert_eq!(
        other
            .exchange_origin(
                "POST",
                "/api/user/tokens",
                r#"{"device_name":"unconfirmed-other-browser"}"#,
                true,
                true
            )
            .await
            .0,
        423
    );
    let (status, _, body) = other
        .exchange_origin(
            "POST",
            "/api/auth/confirm",
            r#"{"purpose":"tokens","password":"long-password-123"}"#,
            true,
            true,
        )
        .await;
    assert_eq!(
        status,
        201,
        "{}",
        token_failure(&serde_json::from_str(&body).unwrap_or(Value::Null))
    );
    let response: Value = serde_json::from_str(&body).unwrap();
    assert!(response.get("data").is_none());
    assert_eq!(
        other
            .exchange_origin(
                "POST",
                "/api/user/tokens",
                r#"{"device_name":"confirmed-other-browser"}"#,
                true,
                true
            )
            .await
            .0,
        201
    );
    assert_eq!(
        browser
            .send("POST", "/user/two-factor-authentication", "{}", true)
            .await
            .0,
        200
    );
    openrust::support::facades::App::set_facade_application(
        Some(server._kernel.get_application()),
        async {
            let clock = server
                ._kernel
                .get_application()
                .make::<openrust::support::DateFactory>()
                .await
                .unwrap();
            clock.set_test_now(Some(openrust::support::Date::from_utc(
                "2030-01-01T03:00:00Z".parse().unwrap(),
                jiff::tz::TimeZone::UTC,
            )));
        },
    )
    .await;
    assert_eq!(
        browser
            .exchange_origin(
                "POST",
                "/api/user/tokens",
                r#"{"device_name":"cookie-exact-boundary"}"#,
                true,
                true
            )
            .await
            .0,
        201
    );
    assert_eq!(
        browser
            .send("POST", "/user/two-factor-authentication", "{}", true)
            .await
            .0,
        200
    );
    openrust::support::facades::App::set_facade_application(
        Some(server._kernel.get_application()),
        async {
            let clock = server
                ._kernel
                .get_application()
                .make::<openrust::support::DateFactory>()
                .await
                .unwrap();
            let at = "2030-01-01T03:00:01Z".parse::<jiff::Timestamp>().unwrap();
            clock.set_test_now(Some(openrust::support::Date::from_utc(
                at,
                jiff::tz::TimeZone::UTC,
            )));
        },
    )
    .await;
    assert_eq!(
        browser
            .exchange_origin(
                "POST",
                "/api/user/tokens",
                r#"{"device_name":"expired-cookie-confirmation"}"#,
                true,
                true
            )
            .await
            .0,
        423
    );
    assert_eq!(
        browser
            .send("POST", "/user/two-factor-authentication", "{}", true)
            .await
            .0,
        423
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn mail_api_register_verify_and_resend_work_without_browser_sessions() {
    let server = start_security("full", true, false, "api/admin").await;
    let registration = json!({"name":"客户端用户","email":"native@example.test","password":"long-password-123","password_confirmation":"long-password-123"});
    let (status, headers, body) = token_client_request(
        server.address,
        "/api/admin/auth/register",
        registration.clone(),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    assert_eq!(body["data"]["email"], "native@example.test");
    assert_eq!(body["data"].as_object().unwrap().len(), 4);
    let (status, _, _) =
        token_client_request(server.address, "/api/admin/auth/register", registration).await;
    assert_eq!(status, 422);
    let mut login = device_login();
    login["email"] = json!("native@example.test");
    let (status, _, body) =
        token_client_request(server.address, "/api/admin/auth/token", login).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let token = body["data"]["token"].as_str().unwrap();
    let link = server.mail_link("/email/verify/");
    let (status, _, _) = account_wire_request(
        server.address,
        token,
        "GET",
        &format!("{link}&tampered=1"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, 403);
    let (status, _, body) = account_request(
        server.address,
        token,
        "POST",
        "/api/admin/auth/email/verification-notification",
        json!({}),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    let link = server.mail_link("/email/verify/");
    let mut anonymous = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    for _ in 0..2 {
        let (status, headers, body) =
            account_wire_request(server.address, token, "GET", &link, Value::Null, None).await;
        assert_eq!(status, 204, "{body}");
        for line in headers.lines() {
            if let Some(cookie) = line.strip_prefix("set-cookie: ") {
                let (name, value) = cookie.split(';').next().unwrap().split_once('=').unwrap();
                anonymous.cookies.insert(name.into(), value.into());
            }
        }
    }
    assert_eq!(
        anonymous
            .exchange_origin("GET", "/api/token-me", "", true, true)
            .await
            .0,
        401
    );
    let (status, _, body) = account_request(
        server.address,
        token,
        "POST",
        "/api/admin/auth/email/verification-notification",
        json!({}),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    assert_eq!(server.mailer.sent().unwrap().len(), 2);
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn mail_api_password_reset_is_single_use_and_revokes_existing_tokens() {
    let server = start_security("api", false, false, "api").await;
    let registration = json!({"name":"找回客户端","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"});
    assert_eq!(
        token_client_request(server.address, "/api/auth/register", registration)
            .await
            .0,
        201
    );
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let token = body["data"]["token"].as_str().unwrap();
    let (status, headers, body) = token_client_request(
        server.address,
        "/api/auth/forgot-password",
        json!({"email":"device@example.test"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    let link = server.mail_link("/reset-password/");
    let reset = link.split('?').next().unwrap().rsplit('/').next().unwrap();
    let input = json!({"token":reset,"email":"device@example.test","password":"new-password-456","password_confirmation":"new-password-456"});
    let (a, b) = tokio::join!(
        token_client_request(server.address, "/api/auth/reset-password", input.clone()),
        token_client_request(server.address, "/api/auth/reset-password", input.clone())
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [200, 422]);
    assert_eq!(bearer_request(server.address, token).await, 401);
    assert_eq!(
        token_client_request(server.address, "/api/auth/reset-password", input)
            .await
            .0,
        422
    );
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        422
    );
    let mut login = device_login();
    login["password"] = json!("new-password-456");
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", login)
            .await
            .0,
        201
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn mail_api_rejects_query_credentials_and_disables_response_caching_on_errors() {
    let server = start_security("api", false, false, "api").await;
    for key in [
        "email",
        "password",
        "token",
        "challenge",
        "proof",
        "code",
        "credential",
    ] {
        let (status, headers, _) = token_client_request(
            server.address,
            &format!("/api/auth/token?{key}=must-not-leak"),
            device_login(),
        )
        .await;
        assert_eq!(status, 400, "{key}");
        assert!(headers.to_ascii_lowercase().contains("no-store"));
    }
    let (status, headers, _) =
        token_client_request(server.address, "/api/auth/token", json!({})).await;
    assert_eq!(status, 422);
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    let (status, headers, _) = account_request(
        server.address,
        "invalid",
        "POST",
        "/api/auth/email/verification-notification",
        json!({}),
    )
    .await;
    assert_eq!(status, 404);
    assert!(!headers.is_empty());
    let (status, headers, _) = account_request(
        server.address,
        "invalid",
        "POST",
        "/api/auth/logout",
        json!({}),
    )
    .await;
    assert_eq!(status, 401);
    assert!(headers.to_ascii_lowercase().contains("no-store"));
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn mail_api_uses_application_clock_for_signed_links_and_password_expiration() {
    let server = start_security("api", true, false, "api").await;
    let clock = server
        ._kernel
        .get_application()
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    let start = "2030-01-01T00:00:00Z".parse::<jiff::Timestamp>().unwrap();
    clock.set_test_now(Some(openrust::support::Date::from_utc(
        start,
        jiff::tz::TimeZone::UTC,
    )));
    let registration = json!({"name":"时钟用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"});
    assert_eq!(
        token_client_request(server.address, "/api/auth/register", registration)
            .await
            .0,
        201
    );
    let link = server.mail_link("/email/verify/");
    assert!(link.contains(&format!("expires={}", start.as_second() + 3600)));
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let token = body["data"]["token"].as_str().unwrap();
    assert_eq!(
        token_client_request(
            server.address,
            "/api/auth/forgot-password",
            json!({"email":"device@example.test"})
        )
        .await
        .0,
        200
    );
    let reset_link = server.mail_link("/reset-password/");
    let reset = reset_link
        .split('?')
        .next()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap();
    clock.set_test_now(Some(openrust::support::Date::from_utc(
        start + jiff::SignedDuration::from_secs(3601),
        jiff::tz::TimeZone::UTC,
    )));
    let (status, _, body) =
        account_wire_request(server.address, token, "GET", &link, Value::Null, None).await;
    assert_eq!(status, 403, "{body}");
    let input = json!({"token":reset,"email":"device@example.test","password":"new-password-456","password_confirmation":"new-password-456"});
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/reset-password", input).await;
    assert_eq!(status, 422, "{body}");
    // 前一令牌超过原生代理的节流期限，可再次发信；过期链接不改变账户验证状态。
    let (status, _, body) = account_request(
        server.address,
        token,
        "POST",
        "/api/auth/email/verification-notification",
        json!({}),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    let new_link = server.mail_link("/email/verify/");
    assert_ne!(new_link, link);
    assert_eq!(
        account_wire_request(server.address, token, "GET", &new_link, Value::Null, None)
            .await
            .0,
        204
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn mail_api_smtp_failure_is_reported_and_explicit_resend_delivers_to_local_inbox() {
    use tokio::io::{AsyncBufReadExt as _, BufReader};
    let smtp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = smtp.local_addr().unwrap().port();
    let inbox = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let reject = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let messages = inbox.clone();
    let reject_next = reject.clone();
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = smtp.accept().await.unwrap();
            let messages = messages.clone();
            let reject_next = reject_next.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut read = BufReader::new(read);
                write.write_all(b"220 localhost ESMTP\r\n").await.unwrap();
                let mut data = false;
                let mut body = String::new();
                loop {
                    let mut line = String::new();
                    if read.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    let reply = if data {
                        if line == ".\r\n" {
                            data = false;
                            messages.lock().unwrap().push(std::mem::take(&mut body));
                            "250 accepted\r\n"
                        } else {
                            body.push_str(&line);
                            continue;
                        }
                    } else if line.starts_with("EHLO") {
                        "250-localhost\r\n250 OK\r\n"
                    } else if line.starts_with("MAIL FROM:") || line.starts_with("RSET") {
                        "250 OK\r\n"
                    } else if line.starts_with("RCPT TO:") {
                        if reject_next.swap(false, std::sync::atomic::Ordering::SeqCst) {
                            "451 temporary rejection\r\n"
                        } else {
                            "250 OK\r\n"
                        }
                    } else if line.starts_with("DATA") {
                        data = true;
                        "354 send body\r\n"
                    } else if line.starts_with("QUIT") {
                        let _ = write.write_all(b"221 bye\r\n").await;
                        break;
                    } else {
                        "250 OK\r\n"
                    };
                    if write.write_all(reply.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    struct Stop(tokio::task::JoinHandle<()>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _smtp = Stop(task);
    let mail = json!({"default":"smtp","mailers":{"smtp":{"transport":"smtp","host":"127.0.0.1","port":port,"scheme":"smtp","encryption":null,"timeout":5}},"from":{"address":"admin@example.test","name":"本地验收"}});
    let server = start_security_catalog_mail("api", true, false, "api", false, Some(mail)).await;
    let registration = json!({"name":"SMTP用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"});
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/register", registration).await;
    assert_eq!(status, 201, "{body}");
    {
        let inbox = inbox.lock().unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(inbox[0].contains("device@example.test"));
        assert!(inbox[0].contains("/email/verify/"));
    }
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let token = body["data"]["token"].as_str().unwrap();
    reject.store(true, std::sync::atomic::Ordering::SeqCst);
    let (status, headers, _) = account_request(
        server.address,
        token,
        "POST",
        "/api/auth/email/verification-notification",
        json!({}),
    )
    .await;
    assert_eq!(status, 500);
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    assert_eq!(inbox.lock().unwrap().len(), 1);
    assert_eq!(
        account_request(
            server.address,
            token,
            "POST",
            "/api/auth/email/verification-notification",
            json!({})
        )
        .await
        .0,
        202
    );
    assert_eq!(inbox.lock().unwrap().len(), 2);
    assert_eq!(
        token_client_request(
            server.address,
            "/api/auth/forgot-password",
            json!({"email":"device@example.test"})
        )
        .await
        .0,
        200
    );
    {
        let inbox = inbox.lock().unwrap();
        assert_eq!(inbox.len(), 3);
        assert!(inbox[2].contains("/reset-password/"));
    }
    let proof = confirmation(&server, token, "email").await;
    reject.store(true, std::sync::atomic::Ordering::SeqCst);
    let (status, headers, _) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/profile-information",
        json!({"name":"SMTP资料变更","email":"changed-smtp@example.test"}),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 500);
    assert!(headers.contains("no-store"));
    assert_eq!(inbox.lock().unwrap().len(), 3);
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        422
    );
    let mut changed = device_login();
    changed["email"] = json!("changed-smtp@example.test");
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", changed)
            .await
            .0,
        201,
        "提交后的SMTP失败不会撤回邮箱变更"
    );
    assert_eq!(
        account_request(
            server.address,
            token,
            "POST",
            "/api/auth/email/verification-notification",
            json!({})
        )
        .await
        .0,
        202
    );
    let inbox = inbox.lock().unwrap();
    assert_eq!(inbox.len(), 4);
    assert!(inbox[3].contains("changed-smtp@example.test"));
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn devices_logout_all_requires_bound_proof_and_revokes_every_old_token() {
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, first) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let first = first["data"]["token"].as_str().unwrap();
    let (_, _, second) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let second = second["data"]["token"].as_str().unwrap();
    assert_eq!(
        account_request(
            server.address,
            first,
            "POST",
            "/api/auth/logout-all",
            json!({})
        )
        .await
        .0,
        401
    );
    let proof = confirmation(&server, first, "devices").await;
    assert_eq!(
        account_request(
            server.address,
            second,
            "POST",
            "/api/auth/logout-all",
            json!({"proof":proof})
        )
        .await
        .0,
        401
    );
    assert_eq!(bearer_request(server.address, first).await, 200);
    assert_eq!(bearer_request(server.address, second).await, 200);
    let pending = confirmation(&server, second, "tokens").await;
    let (status, headers, body) = account_request(
        server.address,
        first,
        "POST",
        "/api/auth/logout-all",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    assert!(body.is_null());
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    for token in [first, second] {
        assert_eq!(bearer_request(server.address, token).await, 401);
    }
    assert_eq!(
        account_request(
            server.address,
            second,
            "POST",
            "/api/user/tokens",
            json!({"proof":pending,"device_name":"stale"})
        )
        .await
        .0,
        401
    );
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let new = body["data"]["token"].as_str().unwrap();
    assert_eq!(bearer_request(server.address, new).await, 200);
    let mut login = device_login();
    login["abilities"] = json!(["account:read"]);
    let (_, _, body) = token_client_request(server.address, "/api/auth/token", login).await;
    let limited = body["data"]["token"].as_str().unwrap();
    assert_eq!(
        account_request(
            server.address,
            limited,
            "POST",
            "/api/auth/logout-all",
            json!({})
        )
        .await
        .0,
        403
    );
    assert_eq!(bearer_request(server.address, new).await, 200);
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn devices_cookie_logout_all_requires_confirmation_and_invalidates_remember_cookies() {
    let server = start().await;
    let mut owner = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(owner.send("GET", "/register", "", false).await.0, 200);
    let registration = json!({"name":"所有设备","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"});
    assert_eq!(
        owner
            .send("POST", "/register", &registration.to_string(), true)
            .await
            .0,
        201
    );
    assert_eq!(owner.send("GET", "/home", "", false).await.0, 200);
    let mut other = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(other.send("GET", "/login", "", false).await.0, 200);
    assert_eq!(
        other
            .send(
                "POST",
                "/login",
                r#"{"email":"device@example.test","password":"long-password-123","remember":true}"#,
                true
            )
            .await
            .0,
        200
    );
    let mut remembered = Client {
        address: server.address,
        cookies: other
            .cookies
            .iter()
            .filter(|(name, _)| name.starts_with("remember_"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        csrf: String::new(),
    };
    assert!(!remembered.cookies.is_empty());
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    assert_eq!(
        owner
            .exchange_origin("POST", "/api/auth/logout-all", "{}", true, true)
            .await
            .0,
        423
    );
    assert_eq!(
        owner
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let (status, headers, body) = owner
        .exchange_origin("POST", "/api/auth/logout-all", "{}", true, true)
        .await;
    assert_eq!(status, 204, "{body}");
    assert!(body.is_empty());
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    for client in [&mut owner, &mut other, &mut remembered] {
        assert_eq!(client.send("GET", "/home", "", true).await.0, 401);
    }
    assert_eq!(bearer_request(server.address, token).await, 401);
}

#[cfg(feature = "sanctum")]
struct RejectCredentialRotation(std::sync::atomic::AtomicBool);
#[cfg(feature = "sanctum")]
impl openrust::database::eloquent::concerns::ModelEventListener<app::models::User>
    for RejectCredentialRotation
{
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut app::models::User,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            use openrust::database::eloquent::Model as _;
            let changed = model.get_attribute("credential_version")
                != model.get_raw_original("credential_version");
            Ok(
                if changed && self.0.load(std::sync::atomic::Ordering::SeqCst) {
                    openrust::database::eloquent::concerns::ModelEventResponse::Cancel
                } else {
                    openrust::database::eloquent::concerns::ModelEventResponse::Continue
                },
            )
        })
    }
}
#[cfg(feature = "sanctum")]
#[tokio::test]
async fn devices_cancelled_rotation_rolls_back_and_old_credentials_remain_usable() {
    use openrust::database::eloquent::{Model as _, concerns::ModelEvent};
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, token, "devices").await;
    app::models::User::register_model_event(
        server._kernel.get_application(),
        ModelEvent::Saving,
        RejectCredentialRotation(std::sync::atomic::AtomicBool::new(true)),
    )
    .unwrap();
    let (status, headers, _) = account_request(
        server.address,
        token,
        "POST",
        "/api/auth/logout-all",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(status, 500);
    assert!(headers.to_ascii_lowercase().contains("no-store"));
    assert_eq!(bearer_request(server.address, token).await, 200);
    // 证明可以在失败时被消费；账号与凭据事务仍须完整回滚，可重新认证。
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        201
    );
}

#[cfg(feature = "sanctum")]
struct ExpireCredentialRotation {
    clock: Arc<openrust::support::DateFactory>,
    at: jiff::Timestamp,
}
#[cfg(feature = "sanctum")]
impl openrust::database::eloquent::concerns::ModelEventListener<app::models::User>
    for ExpireCredentialRotation
{
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut app::models::User,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            use openrust::database::eloquent::Model as _;
            if model.get_attribute("credential_version")
                != model.get_raw_original("credential_version")
            {
                self.clock
                    .set_test_now(Some(openrust::support::Date::from_utc(
                        self.at,
                        jiff::tz::TimeZone::UTC,
                    )));
            }
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
}
#[cfg(feature = "sanctum")]
#[tokio::test]
async fn devices_expired_during_model_save_rolls_back_and_concurrent_proof_has_one_winner() {
    use openrust::database::eloquent::{Model as _, concerns::ModelEvent};
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, token, "devices").await;
    let clock = server
        ._kernel
        .get_application()
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    app::models::User::register_model_event(
        server._kernel.get_application(),
        ModelEvent::Saving,
        ExpireCredentialRotation {
            clock,
            at: jiff::Timestamp::now() + jiff::SignedDuration::from_secs(10801),
        },
    )
    .unwrap();
    assert_eq!(
        account_request(
            server.address,
            token,
            "POST",
            "/api/auth/logout-all",
            json!({"proof":proof})
        )
        .await
        .0,
        401
    );
    assert_eq!(bearer_request(server.address, token).await, 200);
    // 本次证明在存储动作中才到期，版本变更仍必须回滚；新证明使用新的应用时钟。
    let proof = confirmation(&server, token, "devices").await;
    let payload = json!({"proof":proof});
    let (a, b) = tokio::join!(
        account_request(
            server.address,
            token,
            "POST",
            "/api/auth/logout-all",
            payload.clone()
        ),
        account_request(
            server.address,
            token,
            "POST",
            "/api/auth/logout-all",
            payload
        )
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [204, 401]);
    assert_eq!(bearer_request(server.address, token).await, 401);
}

fn admin_fixture_login(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::contracts::auth::StatefulGuard as _;
    Box::pin(async move {
        let password = request
            .input("password")
            .await?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let credentials = openrust::auth::Credentials::new(
            [(
                "email".to_owned(),
                openrust::database::Value::from("admin@example.test"),
            )],
            password,
        );
        let result = openrust::auth::Auth::session_guard(request, Some("admin"))?
            .attempt(&credentials, true)
            .await?;
        if !result {
            return Err(openrust::http::HttpError::Unauthenticated);
        }
        Ok(openrust::http::Response::no_content())
    })
}
fn admin_fixture_user(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let user = openrust::auth::Auth::user(request)
            .await?
            .ok_or(openrust::http::HttpError::Unauthenticated)?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"id":user.get_auth_identifier()}),
        )
    })
}

#[cfg(feature = "sanctum")]
#[derive(Clone)]
struct OtherAccount(openrust::database::eloquent::ModelState);
#[cfg(feature = "sanctum")]
impl openrust::database::eloquent::Model for OtherAccount {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("admins").timestamps(None, None);
        &META
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
}
#[cfg(feature = "sanctum")]
impl openrust::contracts::auth::Authenticatable for OtherAccount {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        use openrust::database::eloquent::Model as _;
        self.get_key().unwrap().try_into().unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        use openrust::database::eloquent::Model as _;
        match self.get_attribute("password") {
            Some(openrust::database::Value::Text(value)) => Some(value),
            _ => None,
        }
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_remember_token(&self) -> Option<&str> {
        use openrust::database::eloquent::Model as _;
        match self.get_attribute("remember_token") {
            Some(openrust::database::Value::Text(value)) => Some(value),
            _ => None,
        }
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        use openrust::database::eloquent::Model as _;
        self.set_attribute(
            "remember_token",
            token.map_or(openrust::database::Value::Null, Into::into),
        );
    }
}
#[cfg(feature = "sanctum")]
impl openrust::sanctum::HasApiTokens for OtherAccount {}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn scope_other_guard_without_version_column_keeps_login_rehash_remember_and_tokens() {
    use openrust::{
        database::{Value as DbValue, eloquent::Model as _},
        sanctum::HasApiTokens as _,
        support::facades::{App, Facade as _},
    };
    let server = start().await;
    register_token_user(&server).await;
    let other = App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let connection = db.connection(None).unwrap();
        connection
            .get_schema_builder()
            .create("admins", |table| {
                table.big_integer("id").primary();
                table.string("email", 255);
                table.string("password", 255).nullable();
                table.string("remember_token", 100).nullable();
            })
            .await
            .unwrap();
        let password = openrust::support::facades::Hash::make(
            "admin-password-123",
            &openrust::hashing::HashOptions::from_iter([("rounds".into(), json!(5))]),
        )
        .await
        .unwrap();
        connection
            .table("admins")
            .insert(&[[
                ("id".into(), DbValue::Integer(1)),
                ("email".into(), "admin@example.test".into()),
                ("password".into(), password.into()),
            ]
            .into()])
            .await
            .unwrap();
        app.make::<openrust::auth::AuthManager>()
            .await
            .unwrap()
            .register_sanctum_model::<OtherAccount>("OtherAccount");
        let user = OtherAccount::query()
            .unwrap()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        let sanctum = app.make::<openrust::sanctum::Sanctum>().await.unwrap();
        user.create_token(&sanctum, &db, "admin-device", &["*"], None)
            .await
            .unwrap()
            .plain_text_token
            .to_string()
    })
    .await;
    let mut admin = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(admin.send("GET", "/login", "", false).await.0, 200);
    assert_eq!(
        admin
            .send(
                "POST",
                "/admin-fixture-login",
                r#"{"password":"admin-password-123"}"#,
                true
            )
            .await
            .0,
        204
    );
    assert_eq!(
        admin.send("GET", "/admin-fixture-user", "", true).await.0,
        200
    );
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let row = db
            .connection(None)
            .unwrap()
            .table("admins")
            .r#where("id", "=", 1)
            .first()
            .await
            .unwrap()
            .unwrap();
        let Some(DbValue::Text(hash)) = row.get("password") else {
            panic!("密码字段类型错误")
        };
        assert!(
            hash.starts_with("$2y$04$") || hash.starts_with("$2b$04$"),
            "管理员密码必须通过原生重哈希写入，不能被套件列条件阻断"
        );
    })
    .await;
    let mut remember = Client {
        address: server.address,
        cookies: admin
            .cookies
            .iter()
            .filter(|(name, _)| name.starts_with("remember_admin"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        csrf: String::new(),
    };
    assert!(!remember.cookies.is_empty());
    assert_eq!(
        remember
            .send("GET", "/admin-fixture-user", "", true)
            .await
            .0,
        200
    );
    assert_eq!(bearer_request(server.address, &other).await, 200);
    assert_eq!(
        account_request(
            server.address,
            &other,
            "POST",
            "/api/auth/confirm",
            json!({"purpose":"devices","password":"admin-password-123"})
        )
        .await
        .0,
        401
    );
    assert_eq!(
        account_request(
            server.address,
            &other,
            "POST",
            "/api/auth/email/verification-notification",
            json!({})
        )
        .await
        .0,
        401
    );
    assert_eq!(
        account_request(
            server.address,
            &other,
            "GET",
            "/api/user/tokens",
            Value::Null
        )
        .await
        .0,
        401
    );
    let previous = App::set_facade_application(Some(server._kernel.get_application()), async {
        let config = server
            ._kernel
            .get_application()
            .make::<dyn openrust::contracts::config::Repository>()
            .await
            .unwrap();
        let previous = config.get("auth.session_credential_version_scope").unwrap();
        config
            .set("auth.session_credential_version_scope", Value::Null)
            .unwrap();
        previous
    })
    .await;
    // 显式恢复旧全局语义会要求管理员也具备套件列；独立后台并没有这个字段。
    let mut probe = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(probe.send("GET", "/login", "", false).await.0, 200);
    assert_eq!(
        probe
            .send(
                "POST",
                "/admin-fixture-login",
                r#"{"password":"admin-password-123"}"#,
                true
            )
            .await
            .0,
        500
    );
    App::set_facade_application(Some(server._kernel.get_application()), async {
        server
            ._kernel
            .get_application()
            .make::<dyn openrust::contracts::config::Repository>()
            .await
            .unwrap()
            .set("auth.session_credential_version_scope", previous)
            .unwrap();
    })
    .await;
    assert_eq!(
        probe
            .send(
                "POST",
                "/admin-fixture-login",
                r#"{"password":"admin-password-123"}"#,
                true
            )
            .await
            .0,
        204
    );

    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let user = body["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, user, "devices").await;
    assert_eq!(
        account_request(
            server.address,
            user,
            "POST",
            "/api/auth/logout-all",
            json!({"proof":proof})
        )
        .await
        .0,
        204
    );
    assert_eq!(bearer_request(server.address, user).await, 401);
    assert_eq!(bearer_request(server.address, &other).await, 200);
    assert_eq!(
        admin.send("GET", "/admin-fixture-user", "", true).await.0,
        200
    );
    assert_eq!(
        remember
            .send("GET", "/admin-fixture-user", "", true)
            .await
            .0,
        200
    );
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let sanctum = server
            ._kernel
            .get_application()
            .make::<openrust::sanctum::Sanctum>()
            .await
            .unwrap();
        sanctum.validate_token_owner_using(|_, _| Ok(false));
        openrust_fortify::starter::credentials::configure_for::<app::models::User>(&sanctum);
    })
    .await;
    assert_eq!(
        bearer_request(server.address, &other).await,
        401,
        "追加策略不能覆盖应用原有拒绝回调"
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn scope_invalid_configuration_never_silently_disables_credential_checks() {
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    let config = server
        ._kernel
        .get_application()
        .make::<dyn openrust::contracts::config::Repository>()
        .await
        .unwrap();
    let original = config.get("auth.session_credential_version_scope").unwrap();
    for invalid in [
        json!({"guards":["web"]}),
        json!({"guards":["web"],"tables":[{"connection":"sqlite","table":""}]}),
        json!({"guards":[""],"tables":[]}),
        json!({"guards":["web"],"tables":[],"unknown":true}),
    ] {
        config
            .set("auth.session_credential_version_scope", invalid)
            .unwrap();
        assert_eq!(bearer_request(server.address, token).await, 500);
        let (status, headers, _) =
            token_client_request(server.address, "/api/auth/token", device_login()).await;
        assert_eq!(status, 500);
        assert!(headers.to_ascii_lowercase().contains("no-store"));
        config
            .set("auth.session_credential_version_scope", original.clone())
            .unwrap();
        assert_eq!(bearer_request(server.address, token).await, 200);
    }
}

struct CustomResource(app::models::User);
impl From<app::models::User> for CustomResource {
    fn from(user: app::models::User) -> Self {
        Self(user)
    }
}
impl openrust::http::resources::json::JsonResource for CustomResource {
    fn to_array(
        &self,
        request: &openrust::http::Request,
    ) -> Result<Value, openrust::http::HttpError> {
        use openrust::contracts::auth::Authenticatable as _;
        if request.parts().uri.query() == Some("resource_fail=1") {
            return Err(openrust::http::HttpError::InvalidSignature);
        }
        Ok(json!({"identity":self.0.get_auth_identifier(),"endpoint":request.path()}))
    }
}
#[cfg(feature = "sanctum")]
#[tokio::test]
async fn custom_native_resource_is_used_by_registration_login_and_pat_responses() {
    let server = start_custom_resource("full", false, false, "api", false, None, true).await;
    let mut browser = Client {
        address: server.address,
        cookies: BTreeMap::new(),
        csrf: String::new(),
    };
    let (status, body) = browser.send("GET", "/login", "", false).await;
    assert_eq!(status, 200, "{body}");
    let(status,body)=browser.send("POST","/api/auth/register",r#"{"name":"资源用户","email":"resource@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["data"]["endpoint"], "api/auth/register");
    assert!(value["data"].get("email").is_none());
    let (status, body) = browser
        .send(
            "POST",
            "/login",
            r#"{"email":"resource@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["data"]["endpoint"],
        "login"
    );
    let (status, body) = browser.send("GET", "/user/profile", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body.contains("资源用户"),
        "自定义API资源不应破坏网页字段: {body}"
    );
    let(status,_,value)=token_client_request(server.address,"/api/auth/token",json!({"email":"resource@example.test","password":"long-password-123","device_name":"Resource phone","abilities":["account:read","tokens:manage"]})).await;
    assert_eq!(status, 201, "{value}");
    assert_eq!(value["data"]["user"]["endpoint"], "api/auth/token");
    let token = value["data"]["token"].as_str().unwrap();
    let (status,_,body) = token_client_request(server.address,"/api/auth/token?resource_fail=1",json!({"email":"resource@example.test","password":"long-password-123","device_name":"failed resource"})).await;
    assert_eq!(status, 403, "{body}");
    let (status, _, body) = account_request(
        server.address,
        token,
        "GET",
        "/api/user/tokens",
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["data"].as_array().unwrap().len(),
        1,
        "资源失败不应签发未知令牌"
    );
    let proof = confirmation(&server, token, "tokens").await;
    let (status, _, body) = account_request_with_proof(
        server.address,
        token,
        "POST",
        "/api/user/tokens?resource_fail=1",
        json!({"device_name":"failed resource"}),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let (status, _, body) = account_request(
        server.address,
        token,
        "GET",
        "/api/user/tokens",
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    let proof = confirmation(&server, token, "tokens").await;
    let (status, _, value) = account_request_with_proof(
        server.address,
        token,
        "POST",
        "/api/user/tokens",
        json!({"device_name":"Resource tablet","abilities":["account:read"]}),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 201, "{value}");
    assert_eq!(value["data"]["user"]["endpoint"], "api/user/tokens");
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_api_requires_email_proof_and_keeps_validation_and_verification_consistent() {
    use openrust::database::eloquent::Model as _;
    use openrust::support::facades::{App, Facade as _};
    let server = start().await;
    register_token_user(&server).await;
    let (status, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&body));
    let token = body["data"]["token"].as_str().unwrap();
    let old_link = server.mail_link("/email/verify/");
    assert_eq!(
        account_wire_request(server.address, token, "GET", &old_link, Value::Null, None)
            .await
            .0,
        204
    );
    let update = json!({"name":"更新后的姓名","email":"updated@example.test"});
    assert_eq!(
        account_request(
            server.address,
            token,
            "PUT",
            "/api/user/profile-information",
            update.clone()
        )
        .await
        .0,
        401
    );
    let wrong = confirmation(&server, token, "tokens").await;
    assert_eq!(
        account_request_with_proof(
            server.address,
            token,
            "PUT",
            "/api/user/profile-information",
            update.clone(),
            Some(&wrong)
        )
        .await
        .0,
        401
    );
    let proof = confirmation(&server, token, "email").await;
    let (status, _, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/profile-information",
        json!({"name":"", "email":"bad-address"}),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert!(body["errors"]["email"].is_array());
    let (status, headers, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/profile-information",
        update.clone(),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.get("data").is_none());
    assert!(headers.contains("no-store"));
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let mut query = app::models::User::query().unwrap();
        query.r#where("email", "=", "updated@example.test");
        let user = query.first().await.unwrap().unwrap();
        assert_eq!(
            user.get_attribute("name"),
            Some(&openrust::database::Value::Text("更新后的姓名".into()))
        );
        assert!(matches!(
            user.get_attribute("email_verified_at"),
            None | Some(openrust::database::Value::Null)
        ));
    })
    .await;
    assert_eq!(
        account_wire_request(server.address, token, "GET", &old_link, Value::Null, None)
            .await
            .0,
        403
    );
    assert_eq!(
        account_request_with_proof(
            server.address,
            token,
            "PUT",
            "/api/user/profile-information",
            update,
            Some(&proof)
        )
        .await
        .0,
        401
    );
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        422
    );
    let mut changed = device_login();
    changed["email"] = json!("updated@example.test");
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", changed)
            .await
            .0,
        201
    );
    let new_link = server.mail_link("/email/verify/");
    assert_ne!(old_link, new_link);
    assert_eq!(
        account_wire_request(server.address, token, "GET", &new_link, Value::Null, None)
            .await
            .0,
        204
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_password_api_scopes_and_mutation_failures_do_not_change_credentials() {
    use openrust::database::eloquent::{Model as _, concerns::ModelEvent};
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    let mut limited = device_login();
    limited["abilities"] = json!(["account:read"]);
    let (_, _, limited) = token_client_request(server.address, "/api/auth/token", limited).await;
    let limited = limited["data"]["token"].as_str().unwrap();
    for path in ["/api/user/profile-information", "/api/user/password"] {
        assert_eq!(
            account_request(server.address, limited, "PUT", path, json!({}))
                .await
                .0,
            403
        );
    }
    let password =
        json!({"password":"new-long-password-456","password_confirmation":"new-long-password-456"});
    let wrong = confirmation(&server, token, "email").await;
    assert_eq!(
        account_request_with_proof(
            server.address,
            token,
            "PUT",
            "/api/user/password",
            password.clone(),
            Some(&wrong)
        )
        .await
        .0,
        401
    );
    let proof = confirmation(&server, token, "password").await;
    let (status, _, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/password",
        json!({"password":"short","password_confirmation":"different"}),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 422, "{body}");
    app::models::User::register_model_event(
        server._kernel.get_application(),
        ModelEvent::Saving,
        RejectCredentialRotation(std::sync::atomic::AtomicBool::new(true)),
    )
    .unwrap();
    let (status, _, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/password",
        password,
        Some(&proof),
    )
    .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(bearer_request(server.address, token).await, 200);
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        201
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_password_api_revokes_old_pat_and_expired_during_save_rolls_back() {
    use openrust::database::eloquent::{Model as _, concerns::ModelEvent};
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, token, "password").await;
    let clock = server
        ._kernel
        .get_application()
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    app::models::User::register_model_event(
        server._kernel.get_application(),
        ModelEvent::Saving,
        ExpireCredentialRotation {
            clock: clock.clone(),
            at: jiff::Timestamp::now() + jiff::SignedDuration::from_secs(10801),
        },
    )
    .unwrap();
    let password =
        json!({"password":"new-long-password-456","password_confirmation":"new-long-password-456"});
    let (status, _, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/password",
        password.clone(),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(bearer_request(server.address, token).await, 200);
    let proof = confirmation(&server, token, "password").await;
    let (status, headers, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/password",
        password,
        Some(&proof),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(headers.contains("no-store"));
    assert!(body.get("data").is_none());
    assert_eq!(bearer_request(server.address, token).await, 401);
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        422
    );
    let mut login = device_login();
    login["password"] = json!("new-long-password-456");
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", login)
            .await
            .0,
        201
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_password_cookie_api_preserves_this_device_and_rejects_other_devices() {
    let server = start().await;
    let mut owner = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(owner.send("GET", "/register", "", false).await.0, 200);
    let registration = r#"{"name":"Cookie账户","email":"cookie-updates@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#;
    assert_eq!(
        owner.send("POST", "/register", registration, true).await.0,
        201
    );
    assert_eq!(owner.send("GET", "/home", "", false).await.0, 200);
    let mut other = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(other.send("GET", "/login", "", false).await.0, 200);
    assert_eq!(other.send("POST","/login",r#"{"email":"cookie-updates@example.test","password":"long-password-123","remember":true}"#,true).await.0,200);
    let password = r#"{"password":"new-cookie-password-456","password_confirmation":"new-cookie-password-456"}"#;
    assert_eq!(
        owner
            .exchange_origin("PUT", "/api/user/password", password, true, true)
            .await
            .0,
        423
    );
    assert_eq!(
        owner
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let (status, _, body) = owner
        .exchange_origin("PUT", "/api/user/password", password, true, true)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(owner.send("GET", "/home", "", false).await.0, 200);
    assert_eq!(other.send("GET", "/home", "", true).await.0, 401);
    assert_eq!(
        owner
            .exchange_origin(
                "PUT",
                "/api/user/profile-information",
                r#"{"name":"修改姓名","email":"cookie-updates@example.test"}"#,
                true,
                true
            )
            .await
            .0,
        423,
        "密码变化必须清除旧近期确认"
    );
    assert_eq!(
        owner
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"new-cookie-password-456"}"#,
                true
            )
            .await
            .0,
        201
    );
    let (status, _, body) = owner
        .exchange_origin(
            "PUT",
            "/api/user/profile-information",
            r#"{"name":"修改姓名","email":"cookie-updates@example.test"}"#,
            true,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = owner.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("修改姓名"));
}

#[cfg(feature = "sanctum")]
struct TimedProfileAction {
    clock: Arc<openrust::support::DateFactory>,
    expire: std::sync::atomic::AtomicBool,
    saved: std::sync::atomic::AtomicUsize,
}
#[cfg(feature = "sanctum")]
impl openrust_fortify::contracts::UpdatesUserProfileInformation for TimedProfileAction {
    fn update<'a>(
        &'a self,
        user: &'a dyn openrust::contracts::auth::Authenticatable,
        input: &'a openrust::http::RequestValue,
    ) -> openrust::http::HttpFuture<
        'a,
        Result<Box<dyn openrust_fortify::contracts::User>, openrust::http::HttpError>,
    > {
        installed::UpdateUserProfileInformation.update(user, input)
    }
    fn validate_for_transaction<'a>(
        &'a self,
        user: &'a dyn openrust::contracts::auth::Authenticatable,
        input: &'a openrust::http::RequestValue,
    ) -> openrust::http::HttpFuture<'a, Result<(), openrust::http::HttpError>> {
        installed::UpdateUserProfileInformation.validate_for_transaction(user, input)
    }
    fn update_in_transaction<'a>(
        &'a self,
        user: &'a dyn openrust::contracts::auth::Authenticatable,
        input: &'a openrust::http::RequestValue,
        tx: &'a mut openrust::database::Transaction,
    ) -> openrust::http::HttpFuture<
        'a,
        Result<Box<dyn openrust_fortify::contracts::User>, openrust::http::HttpError>,
    > {
        Box::pin(async move {
            let result = installed::UpdateUserProfileInformation
                .update_in_transaction(user, input, tx)
                .await?;
            self.saved.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.expire.load(std::sync::atomic::Ordering::SeqCst) {
                self.clock
                    .set_test_now(Some(openrust::support::Date::from_utc(
                        jiff::Timestamp::now() + jiff::SignedDuration::from_secs(10801),
                        jiff::tz::TimeZone::UTC,
                    )));
            }
            Ok(result)
        })
    }
}
#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_custom_transaction_action_expiration_rolls_back_before_mail() {
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    let app = server._kernel.get_application();
    let action = Arc::new(TimedProfileAction {
        clock: app.make().await.unwrap(),
        expire: std::sync::atomic::AtomicBool::new(true),
        saved: std::sync::atomic::AtomicUsize::new(0),
    });
    app.make::<openrust_fortify::Fortify>()
        .await
        .unwrap()
        .update_user_profile_information_using(action.clone());
    let count = server.mailer.sent().unwrap().len();
    let proof = confirmation(&server, token, "email").await;
    let update = json!({"name":"自定义事务","email":"custom-profile@example.test"});
    assert_eq!(
        account_request_with_proof(
            server.address,
            token,
            "PUT",
            "/api/user/profile-information",
            update.clone(),
            Some(&proof)
        )
        .await
        .0,
        401
    );
    assert_eq!(server.mailer.sent().unwrap().len(), count);
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        201
    );
    assert_eq!(action.saved.load(std::sync::atomic::Ordering::SeqCst), 1);
    action
        .expire
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let proof = confirmation(&server, token, "email").await;
    let (status, _, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/profile-information",
        update,
        Some(&proof),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(action.saved.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(server.mailer.sent().unwrap().len(), count + 1);
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_password_proofs_reject_cross_user_and_concurrent_updates_have_one_winner() {
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    let proof = confirmation(&server, token, "password").await;
    let register = json!({"name":"其他用户","email":"other-profile@example.test","password":"long-password-123","password_confirmation":"long-password-123"});
    assert_eq!(
        token_client_request(server.address, "/api/auth/register", register)
            .await
            .0,
        201
    );
    let mut login = device_login();
    login["email"] = json!("other-profile@example.test");
    let (_, _, other) =
        token_client_request(server.address, "/api/auth/token", login.clone()).await;
    let other = other["data"]["token"].as_str().unwrap();
    let password = json!({"password":"concurrent-password-456","password_confirmation":"concurrent-password-456"});
    assert_eq!(
        account_request_with_proof(
            server.address,
            other,
            "PUT",
            "/api/user/password",
            password.clone(),
            Some(&proof)
        )
        .await
        .0,
        401
    );
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", login)
            .await
            .0,
        201
    );
    let (a, b) = tokio::join!(
        account_request_with_proof(
            server.address,
            token,
            "PUT",
            "/api/user/password",
            password.clone(),
            Some(&proof)
        ),
        account_request_with_proof(
            server.address,
            token,
            "PUT",
            "/api/user/password",
            password,
            Some(&proof)
        ),
    );
    let mut codes = [a.0, b.0];
    codes.sort();
    assert_eq!(codes, [200, 401]);
    assert_eq!(bearer_request(server.address, token).await, 401);
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_password_transaction_rejects_other_application_connections_and_honors_rollback() {
    use openrust::database::{DatabaseManager, eloquent::Model as _};
    use openrust::support::facades::{App, Facade as _};
    let a = start().await;
    let b = start().await;
    register_token_user(&a).await;
    register_token_user(&b).await;
    let app = a._kernel.get_application();
    let first = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let second = b
        ._kernel
        .get_application()
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    assert_eq!(first.get_name(), second.get_name());
    App::set_facade_application(Some(app), async {
        let mut user = app::models::User::query_on(&first)
            .first()
            .await
            .unwrap()
            .unwrap();
        let hash = user.get_attribute("password").cloned();
        let mut foreign = second.begin_transaction().await.unwrap();
        assert!(!foreign.belongs_to(&first));
        assert!(
            openrust_fortify::starter::credentials::change_password_in_transaction(
                &mut user,
                &mut foreign,
                "not-persisted"
            )
            .await
            .is_err()
        );
        foreign.commit().await.unwrap();
        let mut tx = first.begin_transaction().await.unwrap();
        assert!(tx.belongs_to(&first));
        openrust_fortify::starter::credentials::change_password_in_transaction(
            &mut user,
            &mut tx,
            "not-persisted",
        )
        .await
        .unwrap();
        tx.roll_back(None).await.unwrap();
        let mut original = app::models::User::query_on(&first)
            .first()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(original.get_attribute("password"), hash.as_ref());
        assert!(
            openrust_fortify::starter::credentials::change_password_in_transaction(
                &mut original,
                &mut tx,
                "closed"
            )
            .await
            .is_err()
        );
    })
    .await;
}

#[cfg(feature = "sanctum")]
struct LegacyProfileAction;
#[cfg(feature = "sanctum")]
impl openrust_fortify::contracts::UpdatesUserProfileInformation for LegacyProfileAction {
    fn update<'a>(
        &'a self,
        _: &'a dyn openrust::contracts::auth::Authenticatable,
        _: &'a openrust::http::RequestValue,
    ) -> openrust::http::HttpFuture<
        'a,
        Result<Box<dyn openrust_fortify::contracts::User>, openrust::http::HttpError>,
    > {
        panic!("套件不能绕过事务入口调用旧Action")
    }
}
#[cfg(feature = "sanctum")]
#[tokio::test]
async fn profile_legacy_action_is_rejected_without_falling_back_to_nontransactional_save() {
    let server = start().await;
    register_token_user(&server).await;
    let (_, _, body) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    let token = body["data"]["token"].as_str().unwrap();
    server
        ._kernel
        .get_application()
        .make::<openrust_fortify::Fortify>()
        .await
        .unwrap()
        .update_user_profile_information_using(Arc::new(LegacyProfileAction));
    let proof = confirmation(&server, token, "email").await;
    let (status, _, body) = account_request_with_proof(
        server.address,
        token,
        "PUT",
        "/api/user/profile-information",
        json!({"name":"旧Action","email":"legacy@example.test"}),
        Some(&proof),
    )
    .await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        201
    );
}
