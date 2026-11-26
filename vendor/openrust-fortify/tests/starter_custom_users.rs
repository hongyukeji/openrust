#![cfg(feature = "sanctum")]
//! 自定义字符串主键、可变认证标识与具名连接，从真实用户套件HTTP入口验收。
use openrust::{
    auth::{AuthIdentifier, AuthResult},
    contracts::{auth::Authenticatable, http::Kernel as _},
    database::{
        DatabaseManager, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    foundation::{Application, http::Kernel},
    hashing::HashOptions,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Facade as _, Route},
    },
};
use openrust_fortify::{Fortify, FortifyServiceProvider, TwoFactorAuthenticatable};
use serde_json::{Value as Json, json};
use std::{path::PathBuf, sync::Arc};
#[derive(Clone)]
struct Member<const K: u8>(ModelState);
impl<const K: u8> Model for Member<K> {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("members")
            .primary_key("identity")
            .incrementing(false)
            .timestamps(None, None);
        static SOFT: ModelMetadata = ModelMetadata::new("members")
            .primary_key("identity")
            .incrementing(false)
            .timestamps(None, None)
            .soft_deletes("deleted_at");
        if K == 0 { &META } else { &SOFT }
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
    fn get_connection_name(&self) -> Option<&str> {
        Some("accounts")
    }
}
impl<const K: u8> Authenticatable for Member<K> {
    fn get_auth_identifier_name(&self) -> &str {
        "email"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        field(self, "email").unwrap().into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        field(self, "password")
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_remember_token(&self) -> Option<&str> {
        field(self, "remember_token")
    }
    fn set_remember_token(&mut self, value: Option<String>) {
        self.set_attribute("remember_token", value.map_or(Value::Null, Value::Text));
    }
    fn as_email_verification(&self) -> Option<&dyn openrust::contracts::auth::MustVerifyEmail> {
        Some(self)
    }
    fn get_auth_attribute(&self, name: &str) -> AuthResult<Option<Value>> {
        Ok(self.get_attribute_value(name)?)
    }
}
impl<const K: u8> openrust::contracts::auth::MustVerifyEmail for Member<K> {
    fn has_verified_email(&self) -> bool {
        self.get_attribute("email_verified_at")
            .is_some_and(|value| *value != Value::Null)
    }
    fn get_email_for_verification(&self) -> &str {
        field(self, "email").unwrap()
    }
    fn get_email_verification_identifier(&self) -> AuthResult<AuthIdentifier> {
        AuthIdentifier::try_from(self.get_key().unwrap())
    }
    fn mark_email_as_verified(&mut self) -> openrust::auth::AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async move {
            self.set_attribute("email_verified_at", "2026-11-26 01:00:00");
            Ok(self.save().await?)
        })
    }
    fn mark_email_as_unverified(&mut self) -> openrust::auth::AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async move {
            self.set_attribute("email_verified_at", Value::Null);
            Ok(self.save().await?)
        })
    }
    fn send_email_verification_notification(
        &self,
    ) -> openrust::auth::AuthFuture<'_, AuthResult<()>> {
        Box::pin(openrust::auth::notifications::VerifyEmail::send(self))
    }
}
fn field<'a, const K: u8>(user: &'a Member<K>, key: &str) -> Option<&'a str> {
    match user.get_attribute(key) {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}
impl<const K: u8> TwoFactorAuthenticatable for Member<K> {}
impl<const K: u8> openrust::sanctum::HasApiTokens for Member<K> {}
impl<const K: u8> openrust_fortify::passkeys::PasskeyAuthenticatable for Member<K> {}
struct AccountToken(ModelState);
impl Model for AccountToken {
    fn metadata() -> &'static ModelMetadata {
        openrust::sanctum::DefaultPersonalAccessToken::metadata()
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        openrust::sanctum::DefaultPersonalAccessToken::casts()
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
    fn get_connection_name(&self) -> Option<&str> {
        Some("accounts")
    }
}
impl openrust::sanctum::PersonalAccessTokenModel for AccountToken {}
struct Kit<const K: u8>;
impl<const K: u8> ServiceProvider for Kit<K> {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move { openrust_fortify::starter::configure(app).await })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let fortify = app.make::<Fortify>().await?;
            app.make::<openrust::sanctum::Sanctum>()
                .await?
                .use_personal_access_token_model::<AccountToken>();
            fortify.register_two_factor_authenticatable::<Member<K>>();
            fortify.register_email_verification::<Member<K>>();
            app.make::<openrust_fortify::passkeys::Passkeys>()
                .await?
                .use_user_model::<Member<K>>();
            openrust_fortify::starter::StarterKitServiceProvider
                .register(app)
                .await?;
            openrust_fortify::starter::client::register_routes::<Member<K>>(app)
        })
    }
}
fn current(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let value = openrust_fortify::starter::UserResource::current(request).await?;
        Response::json(StatusCode::OK, openrust::http::ApiResult::success(value))
    })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get_with("/member", current, |route| {
        route.middleware_name("auth:sanctum");
        Ok(())
    })?;
    Ok(())
}
struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn app<const K: u8>(root: &Root) -> Application {
    let mut app=Application::configure(root.0.clone()).with_config(|root,_|{
        let database:Json=serde_json::from_slice(&std::fs::read(root.join("database.json")).unwrap()).unwrap();
        Ok([
            ("app".into(),json!({"key":"a".repeat(32),"debug":true,"url":"http://localhost","timezone":"Asia/Shanghai"})),
            ("database".into(),json!({"default":"unselected","connections":{"accounts":database,"unselected":{"driver":"sqlite","database":":memory:"}}})),
            ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
            ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
            ("session".into(),json!({"driver":"file","files":root.join("sessions"),"cookie":"member","lottery":[0,100]})),
            ("queue".into(),json!({"default":"sync","connections":{"sync":{"driver":"sync"},"database":{"driver":"database","connection":"accounts"}}})),
            ("user".into(),json!({"profile":"api","registration":false,"email_verification":false,"two_factor":true,"passkeys":false,"avatar":{"enabled":false},"devices":{"enabled":false}})),
            ("fortify".into(),json!({"guard":"members","username":"email","middleware":["web"],"features":[],"limiters":{"login":null,"two-factor":null}})),
            ("auth".into(),json!({"defaults":{"guard":"members"},"guards":{"members":{"driver":"session","provider":"members"}},"providers":{"members":{"driver":"eloquent","model":if K == 0 {openrust::auth::model!(Member<0>,"custom-member-uuid",sanctum)}else{openrust::auth::model!(Member<1>,"custom-member-ulid",sanctum)},"connection":"accounts"}}})),
            ("sanctum".into(),json!({"guard":[]})),
            ("api".into(),json!({"prefix":"api/admin"})),
        ].into())
    }).with_providers(vec![Box::new(Kit::<K>),Box::new(FortifyServiceProvider)]).with_routing(openrust::foundation::configuration::Routing::default().api(routes)).create().unwrap();
    app.boot().await.unwrap();
    app
}
async fn call(
    kernel: &Arc<Kernel>,
    method: &str,
    path: &str,
    body: Json,
    token: Option<&str>,
) -> (u16, Json) {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("accept", "application/json")
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = kernel
        .clone()
        .handle_transport(
            request
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
    let code = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    (
        code,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)})),
    )
}
async fn verify<const K: u8>(mut database: Json) {
    let root = Root(std::env::temp_dir().join(format!(
        "userkit-custom-{K}-{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_microsecond()
    )));
    std::fs::create_dir_all(&root.0).unwrap();
    if database["driver"] == "sqlite" {
        let file = root.0.join("db.sqlite");
        std::fs::File::create(&file).unwrap();
        database["database"] = json!(file);
    }
    database["prefix"] = json!(format!(
        "custom_{K}_{}_",
        jiff::Timestamp::now().as_microsecond()
    ));
    std::fs::write(root.0.join("database.json"), database.to_string()).unwrap();
    let app = app::<K>(&root).await;
    let db = app.make::<DatabaseManager>().await.unwrap();
    let connection = db.connection(Some("accounts")).unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    let app = kernel.get_application();
    let run = async {
        let mut schema = connection.get_schema_builder();
        schema
            .create("members", |t| {
                t.string("identity", 64).primary();
                t.timestamp("deleted_at", 0).nullable();
                t.string("email", 255).unique();
                t.string("name", 255);
                t.string("password", 255);
                t.string("remember_token", 100).nullable();
                t.string("email_verified_at", 64).nullable();
                t.big_integer("credential_version").default(0);
                t.text("two_factor_secret").nullable();
                t.text("two_factor_recovery_codes").nullable();
                t.string("two_factor_confirmed_at", 64).nullable();
            })
            .await
            .unwrap();
        schema
            .create("personal_access_tokens", |t| {
                t.id();
                t.string("tokenable_type", 255);
                t.string("tokenable_id", 64);
                t.string("name", 255);
                t.string("token", 64).unique();
                t.text("abilities");
                t.string("credential_stamp", 64).nullable();
                t.timestamp("last_used_at", 0).nullable();
                t.timestamp("expires_at", 0).nullable();
                t.timestamps();
            })
            .await
            .unwrap();
        schema
            .create("user_auth_challenges", |t| {
                t.string("id", 64).primary();
                t.string("purpose", 64);
                t.string("owner_hash", 64).index();
                t.text("payload");
                t.big_integer("created_at");
                t.big_integer("expires_at");
                t.big_integer("consumed_at").nullable();
            })
            .await
            .unwrap();
        schema
            .create("user_resource_cleanups", |t| {
                t.string("id", 64).primary();
                t.string("receipt_hash", 64);
                t.string("owner_hash", 64);
                t.text("payload");
                t.big_integer("created_at");
                t.big_integer("expires_at");
                t.big_integer("completed_at").nullable();
            })
            .await
            .unwrap();
        let fortify = app.make::<Fortify>().await.unwrap();
        let hasher = app
            .make::<dyn openrust::contracts::hashing::Hasher>()
            .await
            .unwrap();
        let password = hasher
            .make(b"long-password-123", &HashOptions::new())
            .await
            .unwrap();
        let first = if K == 0 {
            "a258d159-41c2-4ac7-a665-2f17858bfc0a"
        } else {
            "01KARTSP9V0NYYF9XJXTVSSN1A"
        };
        let second = if K == 0 {
            "2889bfee-365c-48ac-bc3c-e3d311c6102e"
        } else {
            "01KARTSP9V0NYYF9XJXTVSSN1B"
        };
        let codes = fortify
            .current_encrypter()
            .unwrap()
            .encrypt(br#"["recovery-a","recovery-b"]"#, true)
            .unwrap();
        let secret = fortify
            .current_encrypter()
            .unwrap()
            .encrypt(b"JBSWY3DPEHPK3PXP", true)
            .unwrap();
        let mut original = Member::<K>::from_state(Default::default());
        for (key, value) in [
            ("identity", Value::from(first)),
            ("email", "same@example.test".into()),
            ("name", "自定义账户".into()),
            ("password", password.into()),
            ("credential_version", 0.into()),
            ("two_factor_secret", secret.into()),
            ("two_factor_recovery_codes", codes.into()),
            ("two_factor_confirmed_at", "2026-11-26 01:00:00".into()),
        ] {
            original.set_attribute(key, value);
        }
        original.save().await.unwrap();

        let login = || json!({"email":"same@example.test","password":"long-password-123","device_name":"custom","abilities":["account:read","account:update","tokens:manage"]});
        let (status, pending) = call(&kernel, "POST", "/api/admin/auth/token", login(), None).await;
        assert_eq!(status, 200, "{pending}");
        let pending = pending["data"]["challenge"].clone();
        // 保留所有认证材料，只把邮箱转给另一个真实主键。旧挑战必须失效。
        let mut successor = original.replicate(&[]).await.unwrap();
        successor.set_attribute("identity", second);
        original.set_attribute("email", "retired@example.test");
        original.save().await.unwrap();
        successor.save().await.unwrap();
        let (status, body) = call(
            &kernel,
            "POST",
            "/api/admin/auth/token/challenge",
            json!({"challenge":pending,"recovery_code":"recovery-a"}),
            None,
        )
        .await;
        assert_eq!(status, 401, "{body}");
        let (status, fresh) = call(&kernel, "POST", "/api/admin/auth/token", login(), None).await;
        assert_eq!(status, 200, "{fresh}");
        let (status, token) = call(
            &kernel,
            "POST",
            "/api/admin/auth/token/challenge",
            json!({"challenge":fresh["data"]["challenge"],"recovery_code":"recovery-a"}),
            None,
        )
        .await;
        assert_eq!(status, 201, "{token}");
        let token = token["data"]["token"].as_str().unwrap();
        let (status, current) =
            call(&kernel, "GET", "/api/admin/member", Json::Null, Some(token)).await;
        assert_eq!(status, 200, "{current}");
        assert_eq!(current["data"]["id"], "same@example.test");
        let rows = connection
            .table("personal_access_tokens")
            .get()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("tokenable_id"), Some(&Value::from(second)));
        let (status, proof) = call(
            &kernel,
            "POST",
            "/api/admin/auth/confirm",
            json!({"purpose":"tokens","password":"long-password-123"}),
            Some(token),
        )
        .await;
        assert_eq!(status, 201, "{proof}");
        let (status,issued)=call(&kernel,"POST","/api/admin/user/tokens",json!({"proof":proof["data"]["proof"],"device_name":"another-device","abilities":["account:read","account:update"]}),Some(token)).await;
        assert_eq!(status, 201, "{issued}");
        let (status, _) = call(
            &kernel,
            "POST",
            "/api/admin/auth/logout",
            json!({}),
            Some(token),
        )
        .await;
        assert_eq!(status, 204);
        assert_eq!(
            call(&kernel, "GET", "/api/admin/member", Json::Null, Some(token))
                .await
                .0,
            401
        );

        let child = issued["data"]["token"].as_str().unwrap();
        let (status, proof) = call(
            &kernel,
            "POST",
            "/api/admin/auth/confirm",
            json!({"purpose":"delete","password":"long-password-123"}),
            Some(child),
        )
        .await;
        assert_eq!(status, 201, "{proof}");
        let (status, deleted) = call(
            &kernel,
            "DELETE",
            "/api/admin/user",
            json!({"proof":proof["data"]["proof"]}),
            Some(child),
        )
        .await;
        assert_eq!(status, 204, "{deleted}");
        assert_eq!(
            call(&kernel, "GET", "/api/admin/member", Json::Null, Some(child))
                .await
                .0,
            401
        );
        assert_eq!(
            connection
                .table("personal_access_tokens")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .table("user_auth_challenges")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert!(
            Member::<K>::query_on(&connection)
                .find(second)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            Member::<K>::query_on(&connection)
                .find(first)
                .await
                .unwrap()
                .is_some()
        );
        let physical = connection
            .table("members")
            .r#where("identity", "=", second)
            .first()
            .await
            .unwrap();
        if K == 0 {
            assert!(physical.is_none());
        } else {
            assert_ne!(physical.unwrap().get("deleted_at"), Some(&Value::Null));
        }
        for table in [
            "user_resource_cleanups",
            "user_auth_challenges",
            "personal_access_tokens",
            "members",
        ] {
            connection
                .get_schema_builder()
                .drop_if_exists(table)
                .await
                .unwrap();
        }
    };
    App::set_facade_application(Some(app), run).await;
    db.purge(Some("accounts")).unwrap();
}
#[tokio::test]
async fn uuid_and_ulid_model_keys_are_preserved_and_old_factor_challenges_cannot_change_owner() {
    for kind in [0, 1] {
        if kind == 0 {
            verify::<0>(json!({"driver":"sqlite"})).await;
        } else {
            verify::<1>(json!({"driver":"sqlite"})).await;
        }
    }
}
#[tokio::test]
async fn mysql_custom_models_preserve_keys_and_factor_ownership() {
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").expect("独立MySQL验收URL");
    verify::<0>(json!({"driver":"mysql","url":url})).await;
    verify::<1>(json!({"driver":"mysql","url":url})).await;
}
#[tokio::test]
async fn postgres_custom_models_preserve_keys_and_factor_ownership() {
    let url = std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("独立PostgreSQL验收URL");
    verify::<0>(json!({"driver":"pgsql","url":url})).await;
    verify::<1>(json!({"driver":"pgsql","url":url})).await;
}
