#[path = "../stubs/mod.rs"]
mod installed;
#[path = "../stubs/fortify_service_provider.rs"]
mod installed_provider;
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
    match std::env::var("OPENRUST_USER_KIT_DATABASE")
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
    }
}
async fn start() -> Server {
    start_profile("full").await
}
async fn start_profile(profile: &'static str) -> Server {
    use axum::handler::Handler as _;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = Root(std::env::temp_dir().join(format!(
        "user-starter-http-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&root.0).unwrap();
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
    app.make::<AuthManager>()
        .await
        .unwrap()
        .register_model::<app::models::User>("StarterUser");
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
            table.timestamp("created_at", None).nullable();
            table.timestamp("updated_at", None).nullable();
            Ok(())
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
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    Server {
        address,
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
            (status,body.into())
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
    openrust::support::facades::Route::get("/old-input", old_input)
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
        ("app".into(),json!({"key":"a".repeat(32),"url":"http://localhost","env":"local","name":"用户套件测试","version":"1.2.3","locale":"zh-Hans","supported_locales":[],"debug":true})),
        ("user".into(),json!({"profile":"full","registration":true})),
        ("database".into(),database_configuration(base)),
        ("session".into(),json!({"driver":"array","cookie":"user_session","lottery":[0,100]})),
        ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
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
    config.insert("user".into(), json!({"profile":"api","registration":true}));
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
