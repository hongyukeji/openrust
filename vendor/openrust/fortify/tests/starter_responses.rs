use openrust::{
    auth::{AuthIdentifier, AuthManager, AuthResult},
    contracts::{
        auth::Authenticatable, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    database::Value,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Route},
    },
};
use openrust_fortify::{
    Fortify, FortifyServiceProvider, contracts,
    starter::{StarterKitServiceProvider, UserResource},
};
use serde_json::{Value as Json, json};
use std::sync::Arc;

#[derive(Clone)]
struct Identity;
impl Authenticatable for Identity {
    fn get_auth_identifier_name(&self) -> &str {
        "uuid"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        "user-001".into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        panic!("资源不能读取密码")
    }
    fn get_remember_token(&self) -> Option<&str> {
        panic!("资源不能读取remember token")
    }
    fn set_remember_token(&mut self, _: Option<String>) {
        unreachable!()
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_auth_attribute(&self, name: &str) -> AuthResult<Option<Value>> {
        Ok(match name {
            "name" => Some("张三".into()),
            "email" => Some("user@example.test".into()),
            "email_verified_at" | "profile_photo_path" | "profile_photo_disk" => None,
            _ => panic!("资源不能枚举其他属性: {name}"),
        })
    }
}
struct IgnoreRoutes;
impl ServiceProvider for IgnoreRoutes {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<Fortify>().await?.ignore_routes();
            Ok(())
        })
    }
}
macro_rules! endpoint {
    ($name:ident,$contract:ident) => {
        fn $name(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
            Box::pin(async move {
                let service = App::make::<dyn contracts::$contract>()
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                service.to_response(request).await
            })
        }
    };
}
endpoint!(login, LoginResponse);
endpoint!(register, RegisterResponse);
endpoint!(updated, ProfileInformationUpdatedResponse);
endpoint!(password, PasswordUpdateResponse);
endpoint!(confirmed, PasswordConfirmedResponse);
endpoint!(mail, EmailVerificationNotificationSentResponse);
endpoint!(two_factor, TwoFactorLoginResponse);
endpoint!(logout, LogoutResponse);
endpoint!(verified, VerifyEmailResponse);
fn password_result(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::auth::passwords::PasswordResetStatus;
        if request.path().ends_with("invalid") {
            let response = App::make::<dyn contracts::FailedPasswordResetResponse>()
                .await
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
            return response
                .to_response(request, PasswordResetStatus::InvalidToken)
                .await;
        }
        let response = App::make::<dyn contracts::PasswordResetResponse>()
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        response
            .to_response(request, PasswordResetStatus::PasswordReset)
            .await
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::group(
        openrust::routing::RouteGroup::new().middleware_name("web"),
        |router| {
            router.get("/login", login)?;
            router.get("/register", register)?;
            router.get("/updated", updated)?;
            router.get("/password", password)?;
            router.get("/confirmed", confirmed)?;
            router.get("/mail", mail)?;
            router.get("/two-factor", two_factor)?;
            router.get("/logout", logout)?;
            router.get("/verified", verified)?;
            router.get("/reset-result", password_result)?;
            router.get("/reset-invalid", password_result)?;
            Ok(())
        },
    )
}
async fn application(kit: bool) -> Arc<Kernel> {
    application_with_locale(kit, "en").await
}
async fn application_with_locale(kit: bool, locale: &str) -> Arc<Kernel> {
    application_with_resource(kit, locale, false).await
}
async fn application_with_resource(kit: bool, locale: &str, custom: bool) -> Arc<Kernel> {
    let mut providers: Vec<Box<dyn ServiceProvider>> =
        vec![Box::new(FortifyServiceProvider), Box::new(IgnoreRoutes)];
    if kit {
        providers.push(Box::new(StarterKitServiceProvider));
    }
    let mut app=Application::configure(std::env::temp_dir()).with_providers(providers)
        .with_config(|_,_| Ok([
            ("app".into(),json!({"key":"a".repeat(32),"url":"http://localhost","locale":"en","supported_locales":[]})),
            ("session".into(),json!({"driver":"array","cookie":"starter_session","lottery":[0,100]})),
            ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"starter-test"}}})),
            ("fortify".into(),json!({"home":"/dashboard","features":[]})),
        ].into()))
        .with_routing(Routing::default().api(routes)).create().unwrap();
    app.boot().await.unwrap();
    app.set_locale(locale).await.unwrap();
    if custom {
        UserResource::use_resource::<Identity, PublicIdentity>(&app).unwrap();
    }
    app.make::<AuthManager>()
        .await
        .unwrap()
        .via_request("starter-test", |request| {
            let missing = request.headers().contains_key("x-guest");
            Box::pin(async move {
                Ok((!missing).then(|| Box::new(Identity) as Box<dyn Authenticatable>))
            })
        });
    // Provider不更改应用的功能组合。
    assert_eq!(
        app.container()
            .get::<dyn openrust::contracts::config::Repository>("config")
            .unwrap()
            .get("fortify.features"),
        Some(json!([]))
    );
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    Arc::new(kernel)
}
async fn call(
    kernel: &Arc<Kernel>,
    path: &str,
    accept: &str,
    guest: bool,
) -> (u16, Json, axum::http::HeaderMap) {
    let mut builder = axum::http::Request::builder()
        .uri(format!("/api{path}"))
        .header("accept", accept);
    if guest {
        builder = builder.header("x-guest", "1");
    }
    let response = kernel
        .clone()
        .handle_transport(builder.body(axum::body::Body::empty()).unwrap())
        .await;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 16384)
        .await
        .unwrap();
    let json = if bytes.is_empty() {
        Json::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    (status, json, headers)
}
#[test]
fn resource_only_reads_safe_fields_and_preserves_string_identity() {
    let mut expected =
        json!({"id":"user-001","name":"张三","email":"user@example.test","email_verified_at":null});
    if cfg!(feature = "sanctum") {
        expected["avatar_url"] = serde_json::Value::Null;
    }
    assert_eq!(UserResource::make(&Identity).unwrap(), expected);
}
#[tokio::test]
async fn opt_in_json_projection_statuses_empty_payloads_and_cache_protection() {
    let kernel = application(true).await;
    for (path, status, user) in [
        ("/login", 200, true),
        ("/register", 201, true),
        ("/two-factor", 200, true),
        ("/updated", 200, false),
        ("/password", 200, false),
        ("/confirmed", 201, false),
        ("/mail", 202, false),
    ] {
        let (actual, body, headers) = call(&kernel, path, "application/json", false).await;
        assert_eq!(actual, status, "{path}: {body}");
        assert_eq!(body["success"], true);
        assert_eq!(body["code"], "ok");
        assert_eq!(body.get("data").is_some(), user, "{path}: {body}");
        assert!(
            headers["cache-control"]
                .to_str()
                .unwrap()
                .split(',')
                .any(|value| value.trim() == "no-store")
        );
        if user {
            assert_eq!(body["data"]["id"], "user-001");
            assert!(body["data"].get("password").is_none());
        }
    }
    for path in ["/logout", "/verified"] {
        let (status, body, _) = call(&kernel, path, "application/json", false).await;
        assert_eq!(status, 204);
        assert_eq!(body, Json::Null);
    }
    assert_eq!(
        call(&kernel, "/login", "application/json", true).await.0,
        401
    );
}
#[tokio::test]
async fn standalone_fortify_keeps_native_json() {
    let kernel = application(false).await;
    let (status, body, _) = call(&kernel, "/login", "application/json", false).await;
    assert_eq!(status, 200);
    assert_eq!(body, json!({"two_factor":false}));
}
#[tokio::test]
async fn html_registration_keeps_native_redirect() {
    let kernel = application(true).await;
    let (status, _, headers) = call(&kernel, "/register", "text/html", false).await;
    assert_eq!(status, 302);
    assert_eq!(headers["location"], "http://localhost/dashboard");
}

async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn real_http_wire_preserves_projection_and_no_content() {
    use axum::handler::Handler as _;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport
        .with_state(application(true).await)
        .into_make_service();
    let server = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    for (path, status) in [("login", 200), ("register", 201), ("logout", 204)] {
        let wire=tokio::time::timeout(std::time::Duration::from_secs(5),async {
            let mut socket=tokio::net::TcpStream::connect(address).await.unwrap();
            socket.write_all(format!("GET /api/{path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut wire=String::new();socket.read_to_string(&mut wire).await.unwrap();wire
        }).await.unwrap();
        assert!(wire.starts_with(&format!("HTTP/1.1 {status} ")), "{wire}");
        let (headers, body) = wire.split_once("\r\n\r\n").unwrap();
        if status == 204 {
            assert!(body.is_empty());
        } else {
            assert!(headers.contains("cache-control: no-store"));
            let value: Json = serde_json::from_str(body).unwrap();
            assert_eq!(value["data"]["id"], "user-001");
            assert!(value["data"].get("password").is_none());
        }
    }
    server.abort();
}

#[tokio::test]
async fn success_messages_use_each_application_locale() {
    for (locale, expected) in [("en", "Success"), ("zh-Hans", "成功"), ("zh-Hant", "成功")] {
        let kernel = application_with_locale(true, locale).await;
        let (status, body, _) = call(&kernel, "/login", "application/json", false).await;
        assert_eq!(status, 200);
        assert_eq!(body["message"], expected, "{locale}: {body}");
        assert_eq!(body["data"]["name"], "张三");
    }
}

#[tokio::test]
async fn reset_success_is_localized_and_failure_keeps_field_errors() {
    let kernel = application_with_locale(true, "zh-Hans").await;
    let (status, body, headers) = call(&kernel, "/reset-result", "application/json", false).await;
    assert_eq!(status, 200);
    assert_eq!(body["success"], true);
    assert!(body.get("data").is_none());
    assert!(body["message"].as_str().unwrap().contains("密码"), "{body}");
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    let (status, body, _) = call(&kernel, "/reset-invalid", "application/json", false).await;
    assert_eq!(status, 422);
    assert!(body["errors"]["email"].is_array(), "{body}");
}

#[path = "../../../../app/models/user.rs"]
mod application_user;
#[test]
fn default_application_model_projects_casts_without_hidden_fields() {
    use openrust::database::eloquent::{Model, ModelState};
    let mut user = application_user::User::from_state(ModelState::default());
    user.set_attribute("id", Value::Integer(42));
    user.set_attribute("name", "示例用户");
    user.set_attribute("email", "user@example.test");
    user.set_attribute("email_verified_at", "2026-11-25 08:30:00");
    user.set_attribute("password", "secret-hash");
    user.set_attribute("remember_token", "private-token");
    let value = UserResource::make(&user).unwrap();
    assert_eq!(value["id"], 42);
    assert_eq!(value["name"], "示例用户");
    assert!(value["email_verified_at"].is_string());
    assert_eq!(
        value.as_object().unwrap().len(),
        4 + usize::from(cfg!(feature = "sanctum"))
    );
}

#[cfg(feature = "sanctum")]
#[tokio::test]
async fn response_only_adapter_keeps_native_sanctum_model_and_authentication_policy() {
    let mut app = Application::configure(std::env::temp_dir())
        .with_providers(vec![Box::new(
            openrust_fortify::starter::StarterKitServiceProvider,
        )])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert_eq!(
        app.make::<openrust::sanctum::Sanctum>()
            .await
            .unwrap()
            .personal_access_token_model(),
        std::any::type_name::<openrust::sanctum::DefaultPersonalAccessToken>()
    );
    assert!(
        app.make::<dyn openrust::contracts::config::Repository>()
            .await
            .unwrap()
            .get("auth.session_credential_version_attribute")
            .is_none()
    );
}

struct PublicIdentity {
    user: Identity,
    prepared: bool,
}
impl From<Identity> for PublicIdentity {
    fn from(user: Identity) -> Self {
        Self {
            user,
            prepared: false,
        }
    }
}
impl openrust::http::resources::json::JsonResource for PublicIdentity {
    fn prepare_response(
        &mut self,
        request: &Request,
    ) -> Option<HttpFuture<'_, Result<(), HttpError>>> {
        let fail = request.headers().contains_key("x-resource-fail");
        Some(Box::pin(async move {
            tokio::task::yield_now().await;
            if fail {
                return Err(HttpError::InvalidSignature);
            }
            self.prepared = true;
            Ok(())
        }))
    }
    fn to_array(&self, request: &Request) -> Result<Json, HttpError> {
        assert!(self.prepared);
        if request.headers().contains_key("x-resource-null") {
            return Ok(Json::Null);
        }
        Ok(
            json!({"identity":self.user.get_auth_identifier(),"label":"张三", "path":request.path(),
            "locale":openrust::support::facades::Lang::get_locale()?}),
        )
    }
}

#[tokio::test]
async fn application_resource_is_request_aware_and_isolated_over_real_http() {
    use axum::handler::Handler as _;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut servers = Vec::new();
    let mut addresses = Vec::new();
    for custom in [true, false] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        addresses.push(listener.local_addr().unwrap());
        let service = transport
            .with_state(application_with_resource(true, "zh-Hans", custom).await)
            .into_make_service();
        servers.push(tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }));
    }
    for (index, path, header, status) in [
        (0, "login", "", 200),
        (1, "login", "", 200),
        (0, "register", "", 201),
        (0, "two-factor", "", 200),
        (0, "login", "X-Resource-Fail: 1\r\n", 403),
        (0, "login", "X-Resource-Null: 1\r\n", 500),
        (0, "login", "X-Guest: 1\r\n", 401),
        (0, "login", "", 200),
        (1, "login", "", 200),
    ] {
        let address = addresses[index];
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("GET /api/{path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\n{header}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut wire = String::new();
        socket.read_to_string(&mut wire).await.unwrap();
        assert!(wire.starts_with(&format!("HTTP/1.1 {status} ")), "{wire}");
        if status < 300 {
            assert!(wire.contains("cache-control: no-store"));
            let value: Json = serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
            let data = &value["data"];
            if index == 0 {
                assert_eq!(data["identity"], "user-001");
                assert_eq!(data["locale"], "zh-Hans");
                assert_eq!(data["path"], format!("api/{path}"));
                assert!(data.get("id").is_none());
            } else {
                assert_eq!(data["id"], "user-001");
                assert!(data.get("identity").is_none());
            }
            assert!(data.get("password").is_none());
        }
    }
    for server in servers {
        server.abort();
    }
}
