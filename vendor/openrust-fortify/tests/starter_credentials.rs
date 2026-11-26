#![cfg(feature = "sanctum")]

use openrust::{
    auth::{AuthIdentifier, AuthResult},
    contracts::{
        auth::{Authenticatable, StatefulGuard as _},
        config::Repository,
        http::Kernel as _,
    },
    database::{
        DatabaseManager, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    sanctum::{HasApiTokens, Sanctum, TokenOptions},
    support::facades::{App, Auth, Facade as _, Route},
};
use openrust_fortify::starter::credentials;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
struct User(ModelState);
impl Model for User {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("credential_users").timestamps(None, None);
        &META
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
}
impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.get_key().unwrap().try_into().unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        match self.get_attribute("password") {
            Some(Value::Text(value)) => Some(value),
            _ => None,
        }
    }
    fn get_remember_token(&self) -> Option<&str> {
        match self.get_attribute("remember_token") {
            Some(Value::Text(value)) => Some(value),
            _ => None,
        }
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.set_attribute("remember_token", token.map_or(Value::Null, Value::Text));
    }
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
    fn get_auth_attribute(&self, name: &str) -> AuthResult<Option<Value>> {
        Ok(self.get_attribute_value(name)?)
    }
}
impl HasApiTokens for User {}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/fixture-login", login)?;
    Route::get_with("/me", whoami, |route| {
        route.middleware_name("auth:web");
        Ok(())
    })?;
    Ok(())
}
fn api() -> Result<(), openrust::routing::RoutingError> {
    Route::get_with("/me", whoami, |route| {
        route.middleware_name("auth:sanctum");
        Ok(())
    })?;
    Ok(())
}
fn login(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let database = App::make::<DatabaseManager>().await?;
        let user = User::query_on(&database.connection(None)?)
            .use_write_pdo()
            .find(1)
            .await?
            .unwrap();
        Auth::manager()?
            .session_guard(request, Some("web"))?
            .login(Box::new(user), false)
            .await?;
        Ok(Response::no_content())
    })
}
fn whoami(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let marker = match openrust::sanctum::AccessToken::current_access_token(request) {
            Some(openrust::sanctum::AccessToken::Personal(token)) => {
                token.get_attribute_value("snapshot_marker")?
            }
            _ => None,
        };
        Response::json(
            StatusCode::OK,
            json!({"id":user.get_auth_identifier(),"marker":marker.and_then(|value|if let Value::Bool(value)=value{Some(value)}else{None})}),
        )
    })
}
async fn application(root: &std::path::Path) -> Application {
    let mut app=Application::configure(root.to_path_buf()).with_config(|root,_|{
        let database=serde_json::from_slice(&std::fs::read(root.join("database.json")).unwrap()).unwrap();
        Ok([
            ("database".into(),database),
            ("app".into(),json!({"key":"a".repeat(32),"debug":true,"timezone":"Asia/Shanghai"})),
            ("session".into(),json!({"driver":"file","files":root.join("sessions"),"cookie":"credentials","lottery":[0,100]})),
            ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":openrust::auth::model!(User,"credential-users",sanctum)}},"session_fingerprint_on_login":true,"session_credential_version_attribute":"credential_version"})),
            ("sanctum".into(),json!({"guard":[]})),
            ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
        ].into())
    }).with_routing(Routing::default().web(routes).api(api)).create().unwrap();
    app.boot().await.unwrap();
    credentials::configure(&app.make::<Sanctum>().await.unwrap());
    app
}
async fn request(kernel: Arc<Kernel>, path: &str, header: Option<(&str, &str)>) -> (u16, String) {
    let mut builder = axum::http::Request::builder()
        .uri(path)
        .header("host", "localhost")
        .header("accept", "application/json");
    if let Some((name, value)) = header {
        builder = builder.header(name, value);
    }
    let response = kernel
        .handle_transport(builder.body(axum::body::Body::empty()).unwrap())
        .await;
    let status = response.status().as_u16();
    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("credentials="))
        .map(|v| v.split(';').next().unwrap().to_owned());
    let bytes = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    if status >= 500 {
        eprintln!("{path}: {}", String::from_utf8_lossy(&bytes));
    }
    (
        status,
        cookie.unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned()),
    )
}

async fn verify(database: serde_json::Value) {
    let root = std::env::temp_dir().join(format!(
        "starter-credentials-{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_microsecond()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut database = database;
    if database["connections"]["fixture"]["driver"] == "sqlite" {
        let file = root.join("database.sqlite");
        std::fs::File::create(&file).unwrap();
        database["connections"]["fixture"]["database"] = json!(file);
    }
    if let Ok(url) = std::env::var("OPENRUST_USER_KIT_READ_URL") {
        database["connections"]["fixture"]["read"] = json!({"url":url});
    }
    std::fs::write(
        root.join("database.json"),
        serde_json::to_vec(&database).unwrap(),
    )
    .unwrap();
    let app = application(&root).await;
    let db = app.make::<DatabaseManager>().await.unwrap();
    let connection = db.connection(None).unwrap();
    let mut schema = connection.get_schema_builder();
    schema.drop_if_exists("credential_users").await.unwrap();
    schema
        .create("credential_users", |table| {
            table.id();
            table.string("password", 255).nullable();
            table.string("remember_token", 255).nullable();
            table.big_integer("credential_version").default(0);
        })
        .await
        .unwrap();
    schema
        .drop_if_exists("personal_access_tokens")
        .await
        .unwrap();
    schema
        .create("personal_access_tokens", |table| {
            table.id();
            table.string("tokenable_type", 255);
            table.unsigned_big_integer("tokenable_id");
            table.string("name", 255);
            table.string("token", 64).unique();
            table.text("abilities");
            table.string("credential_stamp", 64).nullable();
            table.timestamp("last_used_at", 0).nullable();
            table.timestamp("expires_at", 0).nullable();
            table.timestamps();
        })
        .await
        .unwrap();
    connection
        .table("credential_users")
        .insert(&[[
            ("id".into(), 1.into()),
            ("password".into(), Value::Null),
            ("remember_token".into(), "old-remember".into()),
            ("credential_version".into(), 0.into()),
        ]
        .into()])
        .await
        .unwrap();
    connection
        .table("credential_users")
        .insert(&[[
            ("id".into(), 2.into()),
            ("password".into(), Value::Null),
            ("remember_token".into(), Value::Null),
            ("credential_version".into(), 0.into()),
        ]
        .into()])
        .await
        .unwrap();
    let issued = App::set_facade_application(Some(&app), async {
        let mut user = User::query_on(&connection)
            .use_write_pdo()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        let mut another = User::from_state(user.state().clone());
        another.set_attribute("id", Value::Integer(2));
        assert_ne!(
            credentials::stamp(&user).unwrap(),
            credentials::stamp(&another).unwrap()
        );
        let config = app.make::<dyn Repository>().await.unwrap();
        let sanctum = app.make::<Sanctum>().await.unwrap();
        let token = credentials::issue(
            &mut user,
            &sanctum,
            &db,
            config.as_ref(),
            "mobile",
            &["account:read"],
        )
        .await
        .unwrap();
        assert!(
            serde_json::to_value(&token.access_token)
                .unwrap()
                .get("credential_stamp")
                .is_none()
        );
        assert!(matches!(
            token
                .access_token
                .get_attribute_value("credential_stamp")
                .unwrap(),
            Some(Value::Text(_))
        ));
        assert!(
            token
                .access_token
                .expires_at
                .unwrap()
                .duration_since(jiff::Timestamp::now())
                .as_secs()
                > 29 * 86400
        );
        assert!(
            credentials::issue(&mut user, &sanctum, &db, config.as_ref(), "admin", &["*"])
                .await
                .is_err()
        );
        let store = sanctum.personal_access_tokens(&db, "").unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        for field in ["token", "tokenable_id->nested", "created_at->year"] {
            assert!(
                store
                    .create_token_in_transaction(
                        &mut tx,
                        "credential-users",
                        1_u64,
                        TokenOptions::new("bad")
                            .attributes([(field.into(), "override".into())].into())
                    )
                    .await
                    .is_err()
            );
        }
        tx.roll_back(None).await.unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        let rolled = store
            .create_token_in_transaction(
                &mut tx,
                "credential-users",
                1_u64,
                TokenOptions::new("rollback"),
            )
            .await
            .unwrap();
        tx.roll_back(None).await.unwrap();
        assert!(
            store
                .find_token(&rolled.plain_text_token)
                .await
                .unwrap()
                .is_none()
        );
        // 同一事务读写配置的令牌模型，拒绝越权归属并验证回滚。
        let id = token.access_token.id.clone();
        let owner = token.access_token.tokenable_type.as_str();
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            store
                .find_owned_in_transaction(&mut tx, &owner.to_uppercase(), 1_u64, id.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .find_owned_in_transaction(&mut tx, owner, 2_u64, id.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .find_owned_in_transaction(&mut tx, owner, 1_u64, id.clone())
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .revoke_token_in_transaction(&mut tx, owner, 1_u64, id)
                .await
                .unwrap()
        );
        tx.roll_back(None).await.unwrap();
        assert!(
            store
                .find_token(&token.plain_text_token)
                .await
                .unwrap()
                .is_some()
        );
        credentials::CredentialToken::register_model_event(
            &app,
            openrust::database::eloquent::concerns::ModelEvent::Deleting,
            CancelTokenRevocation,
        )
        .unwrap();
        let protected = credentials::issue(
            &mut user,
            &sanctum,
            &db,
            config.as_ref(),
            "keep",
            &["account:read"],
        )
        .await
        .unwrap();
        let mut tx = connection.begin_transaction().await.unwrap();
        assert!(
            store
                .revoke_token_in_transaction(
                    &mut tx,
                    owner,
                    1_u64,
                    protected.access_token.id.clone()
                )
                .await
                .is_err()
        );
        tx.roll_back(None).await.unwrap();
        assert!(
            store
                .find_token(&protected.plain_text_token)
                .await
                .unwrap()
                .is_some()
        );
        token.plain_text_token.to_string()
    })
    .await;
    let other = application(&root).await;
    let independent = other
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let mut foreign = independent.begin_transaction().await.unwrap();
    assert!(
        User::query_on(&connection)
            .get_in_transaction(&mut foreign)
            .await
            .is_err()
    );
    let configured = app.make::<Sanctum>().await.unwrap();
    let store = configured.personal_access_tokens(&db, "").unwrap();
    assert!(
        store
            .find_owned_in_transaction(&mut foreign, "credential-users", 1_u64, 1_u64)
            .await
            .is_err()
    );
    assert!(
        store
            .revoke_token_in_transaction(&mut foreign, "credential-users", 1_u64, 1_u64)
            .await
            .is_err()
    );
    foreign.roll_back(None).await.unwrap();
    let mut own = connection.begin_transaction().await.unwrap();
    let redirected = User::query_on(&connection)
        .with_global_scope(
            "other_connection",
            SwitchUserConnection(independent.clone()),
        )
        .unwrap()
        .get_in_transaction(&mut own)
        .await;
    assert!(matches!(
        redirected,
        Err(openrust::database::DatabaseError::InvalidQuery(
            "模型Scope切换了事务连接"
        ))
    ));
    own.roll_back(None).await.unwrap();
    let remote_clock = other
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    let remote_sanctum = other.make::<Sanctum>().await.unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    let mut remote = Kernel::new(other);
    remote.bootstrap().await.unwrap();
    let remote = Arc::new(remote);
    let (status, cookie) = request(kernel.clone(), "/fixture-login", None).await;
    assert_eq!(status, 204);
    assert_eq!(
        request(remote.clone(), "/me", Some(("cookie", &cookie)))
            .await
            .0,
        200
    );
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {issued}")))
        )
        .await
        .0,
        200
    );
    let token_id = issued.split_once('|').unwrap().0.parse::<i64>().unwrap();
    let original_type = connection
        .table("personal_access_tokens")
        .use_write_pdo()
        .r#where("id", "=", token_id)
        .value("tokenable_type")
        .await
        .unwrap();
    connection
        .table("personal_access_tokens")
        .r#where("id", "=", token_id)
        .update([("tokenable_id".into(), 2.into())].into())
        .await
        .unwrap();
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {issued}")))
        )
        .await
        .0,
        401
    );
    connection
        .table("personal_access_tokens")
        .r#where("id", "=", token_id)
        .update(
            [
                ("tokenable_id".into(), 1.into()),
                ("tokenable_type".into(), "credential-users".into()),
            ]
            .into(),
        )
        .await
        .unwrap();
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {issued}")))
        )
        .await
        .0,
        401
    );
    connection
        .table("personal_access_tokens")
        .r#where("id", "=", token_id)
        .update([("tokenable_type".into(), original_type)].into())
        .await
        .unwrap();
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {issued}")))
        )
        .await
        .0,
        200
    );
    // 模拟第二节点持有旧模型；撤销提交后新签发不能使用这个旧证明。
    let admin = application(&root).await;
    let db = admin.make::<DatabaseManager>().await.unwrap();
    let connection = db.connection(None).unwrap();
    let mut stale = User::query_on(&connection)
        .use_write_pdo()
        .find(1)
        .await
        .unwrap()
        .unwrap();
    let config = admin.make::<dyn Repository>().await.unwrap();
    let sanctum = admin.make::<Sanctum>().await.unwrap();
    App::set_facade_application(Some(&admin), async {
        let mut fresh = User::query_on(&connection)
            .use_write_pdo()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        credentials::revoke_all(&mut fresh).await.unwrap();
        assert!(fresh.get_auth_password().is_none());
        assert!(fresh.get_remember_token().is_none());
        assert!(
            credentials::issue(
                &mut stale,
                &sanctum,
                &db,
                config.as_ref(),
                "stale",
                &["account:read"]
            )
            .await
            .is_err()
        );
        use openrust::contracts::auth::UserProvider as _;
        let hasher = admin
            .make::<openrust::hashing::HashManager>()
            .await
            .unwrap()
            .driver(None)
            .unwrap();
        let provider =
            openrust::auth::EloquentUserProvider::<User>::new(connection.clone(), hasher);
        assert!(
            provider
                .update_remember_token(&mut stale, "stale-remember")
                .await
                .is_err()
        );
        assert!(
            User::query_on(&connection)
                .use_write_pdo()
                .find(1)
                .await
                .unwrap()
                .unwrap()
                .get_remember_token()
                .is_none()
        );
    })
    .await;
    assert_eq!(
        request(remote.clone(), "/me", Some(("cookie", &cookie)))
            .await
            .0,
        401
    );
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {issued}")))
        )
        .await
        .0,
        401
    );
    let concurrent = App::set_facade_application(Some(&admin), async {
        use openrust::database::eloquent::concerns::ModelEvent;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        credentials::CredentialToken::register_model_event(
            &admin,
            ModelEvent::Creating,
            PauseCreation {
                entered: entered.clone(),
                release: release.clone(),
            },
        )
        .unwrap();
        let mut issuer = User::query_on(&connection)
            .use_write_pdo()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        let mut revoker = User::query_on(&connection)
            .use_write_pdo()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        let issuance = credentials::issue(
            &mut issuer,
            &sanctum,
            &db,
            config.as_ref(),
            "concurrent",
            &["account:read"],
        );
        let revocation = async {
            entered.notified().await;
            let change = credentials::revoke_all(&mut revoker);
            tokio::pin!(change);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), &mut change)
                    .await
                    .is_err(),
                "签发持有用户行锁时撤销不能提前提交"
            );
            release.notify_one();
            change.await.unwrap();
        };
        let (token, ()) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            tokio::join!(issuance, revocation)
        })
        .await
        .unwrap();
        let token = token.unwrap();
        let mut fresh = User::query_on(&connection)
            .use_write_pdo()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        let before = credentials::stamp(&fresh).unwrap();
        User::register_model_event(&admin, ModelEvent::Saving, CancelCredentialChange).unwrap();
        assert!(credentials::revoke_all(&mut fresh).await.is_err());
        assert_eq!(credentials::stamp(&fresh).unwrap(), before);
        assert_eq!(
            credentials::stamp(
                &User::query_on(&connection)
                    .use_write_pdo()
                    .find(1)
                    .await
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            before
        );
        token.plain_text_token.to_string()
    })
    .await;
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {concurrent}")))
        )
        .await
        .0,
        401
    );
    sanctum.use_personal_access_token_model::<SnapshotToken>();
    remote_sanctum.use_personal_access_token_model::<SnapshotToken>();
    let snapshot_credential = App::set_facade_application(Some(&admin), async {
        let mut user = User::query_on(&connection)
            .use_write_pdo()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        let created = credentials::issue(
            &mut user,
            &sanctum,
            &db,
            config.as_ref(),
            "snapshot",
            &["account:read"],
        )
        .await
        .unwrap();
        let loaded = sanctum
            .personal_access_tokens(&db, "")
            .unwrap()
            .find_token(&created.plain_text_token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            loaded.get_attribute_value("snapshot_marker").unwrap(),
            Some(Value::Bool(true))
        );
        created.plain_text_token.to_string()
    })
    .await;
    let (status, body) = request(
        remote.clone(),
        "/api/me",
        Some(("authorization", &format!("Bearer {snapshot_credential}"))),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["marker"],
        true
    );
    let rehashed = App::set_facade_application(Some(&admin), async {
        use openrust::contracts::{auth::UserProvider as _, hashing::Hasher as _};
        let old = openrust::hashing::BcryptHasher::new(openrust::hashing::BcryptConfig {
            rounds: 4,
            ..Default::default()
        });
        let hash = old
            .make(b"same-password", &Default::default())
            .await
            .unwrap();
        connection
            .table("credential_users")
            .insert(&[[
                ("id".into(), 3.into()),
                ("password".into(), hash.clone().into()),
                ("credential_version".into(), 0.into()),
            ]
            .into()])
            .await
            .unwrap();
        let mut user = User::query_on(&connection)
            .use_write_pdo()
            .find(3)
            .await
            .unwrap()
            .unwrap();
        let token = credentials::issue(
            &mut user,
            &sanctum,
            &db,
            config.as_ref(),
            "rehash",
            &["account:read"],
        )
        .await
        .unwrap();
        let provider = openrust::auth::EloquentUserProvider::<User>::new(
            connection.clone(),
            Arc::new(openrust::hashing::BcryptHasher::new(
                openrust::hashing::BcryptConfig {
                    rounds: 5,
                    ..Default::default()
                },
            )),
        );
        provider
            .rehash_password_if_required(
                &mut user,
                &openrust::auth::Credentials::new([("id", 3.into())], "same-password"),
                false,
            )
            .await
            .unwrap();
        let mut fresh = User::query_on(&connection)
            .use_write_pdo()
            .find(3)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(fresh.get_auth_password(), Some(hash.as_str()));
        assert_eq!(
            fresh.get_attribute("credential_version"),
            Some(&Value::Integer(0))
        );
        provider
            .rehash_password_if_required(
                &mut fresh,
                &openrust::auth::Credentials::new([("id", 3.into())], "same-password"),
                true,
            )
            .await
            .unwrap();
        token.plain_text_token.to_string()
    })
    .await;
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {rehashed}")))
        )
        .await
        .0,
        200
    );
    let frozen = "2030-01-02T03:04:05Z".parse::<jiff::Timestamp>().unwrap();
    let at = |instant| {
        Some(openrust::support::Date::from_utc(
            instant,
            jiff::tz::TimeZone::UTC,
        ))
    };
    let clock = admin
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    clock.set_test_now(at(frozen));
    remote_clock.set_test_now(at(frozen));
    let (regular, short) = App::set_facade_application(Some(&admin), async {
        let mut user = User::query_on(&connection)
            .use_write_pdo()
            .find(1)
            .await
            .unwrap()
            .unwrap();
        let regular = credentials::issue(
            &mut user,
            &sanctum,
            &db,
            config.as_ref(),
            "clock",
            &["account:read"],
        )
        .await
        .unwrap();
        let deadline = frozen
            .checked_add(jiff::SignedDuration::from_secs(30 * 86400))
            .unwrap();
        assert_eq!(regular.access_token.created_at, Some(frozen));
        assert_eq!(regular.access_token.expires_at, Some(deadline));
        let short = credentials::issue_with_expiration(
            &mut user,
            &sanctum,
            &db,
            config.as_ref(),
            "short",
            &["account:read"],
            Some(
                frozen
                    .checked_add(jiff::SignedDuration::from_secs(3600))
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
        let long = credentials::issue_with_expiration(
            &mut user,
            &sanctum,
            &db,
            config.as_ref(),
            "long",
            &["account:read"],
            Some(
                frozen
                    .checked_add(jiff::SignedDuration::from_secs(60 * 86400))
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(long.access_token.expires_at, Some(deadline));
        (
            regular.plain_text_token.to_string(),
            short.plain_text_token.to_string(),
        )
    })
    .await;
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {regular}")))
        )
        .await
        .0,
        200
    );
    remote_clock.set_test_now(at(frozen
        .checked_add(jiff::SignedDuration::from_secs(3601))
        .unwrap()));
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {short}")))
        )
        .await
        .0,
        401
    );
    assert_eq!(
        request(
            remote.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {regular}")))
        )
        .await
        .0,
        200
    );
    let limited = application(&root).await;
    limited
        .make::<dyn Repository>()
        .await
        .unwrap()
        .set("sanctum.expiration", json!(5))
        .unwrap();
    let limited_clock = limited
        .make::<openrust::support::DateFactory>()
        .await
        .unwrap();
    limited_clock.set_test_now(at(frozen));
    let mut limited = Kernel::new(limited);
    limited.bootstrap().await.unwrap();
    let limited = Arc::new(limited);
    assert_eq!(
        request(
            limited.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {regular}")))
        )
        .await
        .0,
        200
    );
    limited_clock.set_test_now(at(frozen
        .checked_add(jiff::SignedDuration::from_secs(300))
        .unwrap()));
    assert_eq!(
        request(
            limited.clone(),
            "/api/me",
            Some(("authorization", &format!("Bearer {regular}")))
        )
        .await
        .0,
        401
    );
    clock.set_test_now(at(frozen
        .checked_add(jiff::SignedDuration::from_secs(30 * 86400 + 1))
        .unwrap()));
    assert_eq!(
        admin
            .handle_command(&["sanctum:prune-expired".into(), "--hours=0".into()])
            .await
            .unwrap(),
        0
    );
    assert!(
        sanctum
            .personal_access_tokens(&db, "")
            .unwrap()
            .find_token(&regular)
            .await
            .unwrap()
            .is_none()
    );
    drop(limited);
    drop(remote);
    drop(kernel);
    let _ = std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn sqlite_passwordless_revocation_rejects_sessions_tokens_and_stale_issuance() {
    verify(json!({"default":"fixture","connections":{"fixture":{"driver":"sqlite","database":":memory:"}}})).await;
}
#[tokio::test]
#[ignore = "必须提供独占MySQL 8.4连接"]
async fn mysql_passwordless_revocation_rejects_sessions_tokens_and_stale_issuance() {
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").expect("必须提供独占MySQL");
    verify(json!({"default":"fixture","connections":{"fixture":{"driver":"mysql","url":url}}}))
        .await;
}
#[tokio::test]
#[ignore = "必须提供独占PostgreSQL 18连接"]
async fn postgres_passwordless_revocation_rejects_sessions_tokens_and_stale_issuance() {
    let url = std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("必须提供独占PostgreSQL");
    verify(json!({"default":"fixture","connections":{"fixture":{"driver":"pgsql","url":url}}}))
        .await;
}

struct PauseCreation {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl openrust::database::eloquent::concerns::ModelEventListener<credentials::CredentialToken>
    for PauseCreation
{
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut credentials::CredentialToken,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            if model.get_attribute("name") == Some(&Value::Text("concurrent".into())) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
}
struct CancelCredentialChange;
impl openrust::database::eloquent::concerns::ModelEventListener<User> for CancelCredentialChange {
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        _: &'a mut User,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async { Ok(openrust::database::eloquent::concerns::ModelEventResponse::Cancel) })
    }
}

#[derive(Clone)]
struct SnapshotToken {
    state: ModelState,
    marker: bool,
}
impl Model for SnapshotToken {
    fn metadata() -> &'static ModelMetadata {
        credentials::CredentialToken::metadata()
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        credentials::CredentialToken::casts()
    }
    fn from_state(state: ModelState) -> Self {
        Self {
            state,
            marker: false,
        }
    }
    fn state(&self) -> &ModelState {
        &self.state
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.state
    }
    fn get_attribute_value(&self, name: &str) -> openrust::database::DatabaseResult<Option<Value>> {
        if name == "snapshot_marker" {
            Ok(Some(Value::Bool(self.marker)))
        } else {
            credentials::CredentialToken::from_state(self.state.clone()).get_attribute_value(name)
        }
    }
}
impl openrust::sanctum::PersonalAccessTokenModel for SnapshotToken {
    fn clone_token_model(&self) -> Self {
        self.clone()
    }
    async fn find_token(
        query: openrust::database::eloquent::Builder<'static, Self>,
        connection: &openrust::database::Connection,
        credential: &str,
    ) -> openrust::database::DatabaseResult<Option<Self>> {
        let mut result = Self::find_token_by_digest(query, connection, credential).await?;
        if let Some(model) = &mut result {
            model.marker = true;
        }
        Ok(result)
    }
}

struct CancelTokenRevocation;
impl openrust::database::eloquent::concerns::ModelEventListener<credentials::CredentialToken>
    for CancelTokenRevocation
{
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut credentials::CredentialToken,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            Ok(
                if model.get_attribute("name") == Some(&Value::Text("keep".into())) {
                    openrust::database::eloquent::concerns::ModelEventResponse::Cancel
                } else {
                    openrust::database::eloquent::concerns::ModelEventResponse::Continue
                },
            )
        })
    }
}

struct SwitchUserConnection(openrust::database::Connection);
impl openrust::database::eloquent::Scope<User> for SwitchUserConnection {
    fn apply(
        &self,
        query: &mut openrust::database::eloquent::Builder<'_, User>,
    ) -> openrust::database::DatabaseResult<()> {
        *query = User::query_on(&self.0);
        Ok(())
    }
}
