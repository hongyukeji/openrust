use openrust::{
    auth::SessionGuard,
    contracts::{auth::StatefulGuard as _, config::Repository, session::Session as _},
    database::{DatabaseManager, Value, eloquent::Model},
    foundation::Application,
    http::{Body, Request},
    session::{
        ArraySessionHandler, DatabaseSessionMetadata, EncryptedStore, SessionManager, Store,
    },
    support::facades::{App, Auth, Facade as _},
};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, time::Duration};
#[path = "../../../app/models/user.rs"]
mod user;

fn settings(encrypt: bool) -> BTreeMap<String, Json> {
    let driver =
        std::env::var("OPENRUST_SESSION_DEVICE_DATABASE").unwrap_or_else(|_| "sqlite".into());
    let connection = if driver == "sqlite" {
        json!({"driver":"sqlite","database":":memory:"})
    } else {
        assert!(matches!(driver.as_str(), "mysql" | "pgsql"));
        json!({"url":std::env::var("OPENRUST_SESSION_DEVICE_DATABASE_URL").expect("external device tests require an isolated database URL")})
    };
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let suffix = format!(
        "{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    [
        ("app".into(),json!({"key":"s".repeat(32)})),
        ("database".into(),json!({"default":"device_test","connections":{"device_test":connection}})),
        ("session".into(),json!({"driver":"database","table":format!("device_sessions_{suffix}"),"lifetime":120,"encrypt":encrypt,"lottery":[0,100]})),
        ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"},"admin":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"database","table":format!("device_users_{suffix}")}},"session_fingerprint_on_login":true,"session_credential_version_attribute":"credential_version"})),
    ].into()
}
async fn restored_request(id: &str, attributes: &BTreeMap<String, Json>) -> Request {
    let mut store = Store::new(
        "validator_cookie",
        Box::new(ArraySessionHandler::new(Duration::from_secs(120))),
    )
    .unwrap();
    store.set_id(Some(id)).unwrap();
    store.start().await.unwrap();
    for (key, value) in attributes {
        store.put(key, value.clone()).unwrap();
    }
    let mut request: Request = http::Request::new(Body::empty()).into();
    request.set_session(store);
    request
}

async fn verify(encrypt: bool) {
    let builder = Application::configure(std::env::temp_dir());
    let builder = if encrypt {
        builder.with_config(|_, _| Ok(settings(true)))
    } else {
        builder.with_config(|_, _| Ok(settings(false)))
    };
    let mut app = builder.create().unwrap();
    app.boot().await.unwrap();
    let manager = app.make::<SessionManager>().await.unwrap();
    let config = app.make::<dyn Repository>().await.unwrap();
    let database = app.make::<DatabaseManager>().await.unwrap();
    let db = database.connection(None).unwrap();
    let table = config
        .get("session.table")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    db.get_schema_builder()
        .create(&table, |t| {
            t.string("id", 255).primary();
            t.big_integer("user_id").nullable();
            t.string("ip_address", 45).nullable();
            t.text("user_agent").nullable();
            t.long_text("payload");
            t.integer("last_activity");
        })
        .await
        .unwrap();
    let users = config
        .get("auth.providers.users.table")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    db.get_schema_builder()
        .create(&users, |t| {
            t.big_integer("id").primary();
            t.string("password", 255);
            t.big_integer("credential_version");
        })
        .await
        .unwrap();
    db.table(&users)
        .insert(&[[
            ("id".into(), Value::Integer(7)),
            ("password".into(), Value::Text("hash-before".into())),
            ("credential_version".into(), Value::Integer(0)),
        ]
        .into()])
        .await
        .unwrap();
    App::set_facade_application(Some(&app), async {
        let mut user = user::User::from_state(Default::default());
        user.set_attribute("id", Value::Integer(7));
        user.set_attribute("email", Value::Text("device@example.test".into()));
        user.set_attribute("password", Value::Text("hash-before".into()));
        user.set_attribute("credential_version", Value::Integer(0));
        let mut handler = manager.database_handler().unwrap();
        handler.set_metadata(DatabaseSessionMetadata {
            user_id: Some(Value::Integer(7)),
            ip_address: Some(Some("127.0.0.9".into())),
            user_agent: Some("Device test".into()),
        });
        let handler: Box<dyn openrust::contracts::session::SessionHandler> = if encrypt {
            Box::new(EncryptedStore::new(
                Box::new(handler),
                app.make::<dyn openrust::contracts::encryption::Encrypter>()
                    .await
                    .unwrap(),
            ))
        } else {
            Box::new(handler)
        };
        let mut store = Store::new("device_cookie", handler).unwrap();
        store.start().await.unwrap();
        let mut request: Request = http::Request::new(Body::empty()).into();
        request.set_session(store);
        let copied = user::User::from_state(user.state().clone());
        Auth::session_guard(&mut request, Some("web"))
            .unwrap()
            .login(Box::new(copied), false)
            .await
            .unwrap();
        request.session_mut().unwrap().save().await.unwrap();
        let id = request.session().unwrap().get_id().to_owned();
        let rows = manager
            .database_handler()
            .unwrap()
            .sessions_for(Value::Integer(7))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].ip_address.as_deref(), Some("127.0.0.9"));
        assert_eq!(rows[0].user_agent.as_deref(), Some("Device test"));
        assert!(
            manager
                .database_handler()
                .unwrap()
                .sessions_for(Value::Integer(8))
                .await
                .unwrap()
                .is_empty()
        );
        let before = db.table(&table).get().await.unwrap();
        let attributes = manager.read_database_session(&id).await.unwrap();
        assert!(
            SessionGuard::session_belongs_to_user(config.as_ref(), "web", &user, &attributes)
                .unwrap()
        );
        assert!(
            !SessionGuard::session_belongs_to_user(config.as_ref(), "admin", &user, &attributes)
                .unwrap()
        );
        use openrust::contracts::auth::Guard as _;
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let first_order = order.clone();
        Auth::add_session_validator("first", move |guard, user, session| {
            let first_order = first_order.clone();
            Box::pin(async move {
                assert_eq!(guard, "web");
                assert_eq!(user.get_auth_identifier().to_string(), "7");
                assert_eq!(session.get_id().len(), 40);
                tokio::task::yield_now().await;
                first_order.lock().unwrap().push(1);
                Ok(true)
            })
        })
        .unwrap();
        let deny_order = order.clone();
        Auth::add_session_validator("deny", move |_, _, _| {
            let deny_order = deny_order.clone();
            Box::pin(async move {
                deny_order.lock().unwrap().push(2);
                Ok(false)
            })
        })
        .unwrap();
        let mut restored = restored_request(&id, &attributes).await;
        restored
            .session_mut()
            .unwrap()
            .put("cart", serde_json::json!(["keep"]))
            .unwrap();
        restored
            .session_mut()
            .unwrap()
            .put("login_admin", serde_json::json!(99))
            .unwrap();
        assert!(
            Auth::session_guard(&mut restored, Some("web"))
                .unwrap()
                .user()
                .await
                .is_err()
        );
        assert_eq!(
            restored.session().unwrap().get("cart"),
            Some(&serde_json::json!(["keep"]))
        );
        assert_eq!(
            restored.session().unwrap().get("login_admin"),
            Some(&serde_json::json!(99))
        );
        assert!(restored.session().unwrap().get("login_web").is_none());
        assert_eq!(*order.lock().unwrap(), [1, 2]);
        assert!(!restored.session().unwrap().has("login_web"));
        let replacement_order = order.clone();
        order.lock().unwrap().clear();
        Auth::add_session_validator("first", move |_, _, _| {
            let replacement_order = replacement_order.clone();
            Box::pin(async move {
                replacement_order.lock().unwrap().push(3);
                Ok(false)
            })
        })
        .unwrap();
        let mut restored = restored_request(&id, &attributes).await;
        assert!(
            Auth::session_guard(&mut restored, Some("web"))
                .unwrap()
                .user()
                .await
                .is_err()
        );
        assert_eq!(
            *order.lock().unwrap(),
            [3],
            "同名替换保持顺序，拒绝后不继续调用其他规则"
        );
        assert!(Auth::remove_session_validator("first").unwrap());
        assert!(!Auth::remove_session_validator("first").unwrap());
        assert!(Auth::remove_session_validator("deny").unwrap());
        let mut restored = restored_request(&id, &attributes).await;
        assert!(
            Auth::session_guard(&mut restored, Some("web"))
                .unwrap()
                .user()
                .await
                .unwrap()
                .is_some()
        );
        Auth::add_session_validator("error", |_, _, _| {
            Box::pin(async {
                Err(openrust::auth::AuthError::Configuration("会话策略不可用"))
            })
        })
        .unwrap();
        let mut restored = restored_request(&id, &attributes).await;
        assert!(
            Auth::session_guard(&mut restored, Some("web"))
                .unwrap()
                .user()
                .await
                .is_err()
        );
        assert!(Auth::remove_session_validator("error").unwrap());
        assert!(Auth::add_session_validator(" ", |_, _, _| Box::pin(async { Ok(true) })).is_err());
        let mut forged = attributes.clone();
        forged.remove("password_hash_web");
        assert!(
            !SessionGuard::session_belongs_to_user(config.as_ref(), "web", &user, &forged).unwrap()
        );
        forged.insert("password_hash_web".into(), json!(vec![0u8; 32]));
        assert!(
            !SessionGuard::session_belongs_to_user(config.as_ref(), "web", &user, &forged).unwrap()
        );
        user.set_attribute("credential_version", Value::Integer(1));
        assert!(
            !SessionGuard::session_belongs_to_user(config.as_ref(), "web", &user, &attributes)
                .unwrap()
        );
        assert!(
            manager
                .read_database_session("not-a-session-id")
                .await
                .is_err()
        );
        let missing = manager
            .read_database_session(&"z".repeat(40))
            .await
            .unwrap();
        assert!(!missing.contains_key("login_web"));
        assert_eq!(
            db.table(&table).get().await.unwrap(),
            before,
            "只读加载不能保存、刷新时间或回收其他会话"
        );
        db.table(&table)
            .r#where("id", "=", id)
            .update([("last_activity".into(), Value::Integer(0))].into())
            .await
            .unwrap();
        assert!(
            manager
                .database_handler()
                .unwrap()
                .sessions_for(Value::Integer(7))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !manager
                .read_database_session(request.session().unwrap().get_id())
                .await
                .unwrap()
                .contains_key("login_web")
        );
        manager
            .extend("database", |_, _| {
                Ok(ArraySessionHandler::new(Duration::from_secs(120)))
            })
            .unwrap();
        assert!(
            manager.database_handler().is_err(),
            "同名自定义驱动不能冒充原生枚举支持"
        );
    })
    .await;
    db.get_schema_builder().drop(&table).await.unwrap();
    db.get_schema_builder().drop(&users).await.unwrap();
}
#[tokio::test]
async fn database_session_metadata_and_guard_ownership_are_read_only() {
    verify(false).await;
}
#[tokio::test]
async fn encrypted_database_sessions_use_native_store_and_key_resolution() {
    verify(true).await;
}
