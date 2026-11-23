use openrust::{
    contracts::{container::Container as _, foundation::Application as _, hashing::Hasher},
    database::{
        DatabaseError, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            casts::{Attribute, AttributeCast},
        },
    },
    hashing::HashOptions,
};

#[path = "fixtures/force_fill.rs"]
mod documented;

struct Guarded(ModelState);
impl Model for Guarded {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("guarded").hidden(&["secret", "password"]);
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
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("secret", AttributeCast::Hashed),
            ("password", AttributeCast::Hashed),
            ("payload", AttributeCast::Json),
        ]
    }
    fn attribute(name: &str) -> Option<Attribute> {
        (name == "label").then(|| {
            Attribute::set(|value, _| match value {
                Value::Text(value) => Ok(Attributes::from([(
                    "label".into(),
                    value.to_uppercase().into(),
                )])),
                _ => Err(DatabaseError::InvalidQuery("label must be text")),
            })
        })
    }
}
struct CustomFill(ModelState);
impl Model for CustomFill {
    fn metadata() -> &'static ModelMetadata {
        Guarded::metadata()
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
    fn fill(&mut self, _: Attributes) -> openrust::database::DatabaseResult<&mut Self> {
        self.0.set_attribute("custom_fill", true);
        Ok(self)
    }
    async fn fill_async(&mut self, _: Attributes) -> openrust::database::DatabaseResult<&mut Self> {
        self.0.set_attribute("custom_async_fill", true);
        Ok(self)
    }
}

fn attrs(name: &str, value: impl Into<Value>) -> Attributes {
    Attributes::from([(name.to_owned(), value.into())])
}

#[test]
fn force_fill_retains_mutators_json_and_errors_without_leaking_guard_state() {
    let mut custom = CustomFill::from_state(ModelState::default());
    custom.force_fill(Attributes::new()).unwrap();
    assert_eq!(
        custom.get_attribute("custom_fill"),
        Some(&Value::Bool(true))
    );
    let mut model = Guarded::new_instance(Attributes::new()).unwrap();
    assert!(matches!(
        model.fill(attrs("label", "blocked")),
        Err(DatabaseError::MassAssignment(_))
    ));
    model.force_fill(attrs("label", "converted")).unwrap();
    assert_eq!(
        model.get_attribute("label"),
        Some(&Value::from("CONVERTED"))
    );
    let json = serde_json::json!({"nested": [1, true, null]});
    model
        .force_fill(attrs("payload", Value::Json(json.clone())))
        .unwrap();
    assert_eq!(
        model.get_attribute_value("payload").unwrap(),
        Some(Value::Json(json))
    );
    let before = model.state().get_attributes().clone();
    assert!(model.force_fill(attrs("label", Value::Null)).is_err());
    assert_eq!(model.state().get_attributes(), &before);
    assert!(matches!(
        model.fill(attrs("label", "still blocked")),
        Err(DatabaseError::MassAssignment(_))
    ));
}

#[tokio::test]
async fn force_fill_hashes_secrets_and_keeps_other_tasks_and_error_paths_guarded() {
    let mut custom = CustomFill::from_state(ModelState::default());
    custom.force_fill_async(Attributes::new()).await.unwrap();
    assert_eq!(
        custom.get_attribute("custom_async_fill"),
        Some(&Value::Bool(true))
    );
    // Lazy MySQL connection: no database I/O is used by these in-memory hash assignments.
    let mut app = openrust::foundation::Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap();
    config
        .set("hashing.bcrypt.rounds", serde_json::json!(4))
        .unwrap();
    config
        .set("database.default", serde_json::json!("test"))
        .unwrap();
    config
        .set(
            "database.connections.test",
            serde_json::json!({"driver":"mysql","database":"unused"}),
        )
        .unwrap();
    let db = app
        .container()
        .make::<openrust::database::DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let hasher = app.container().make::<dyn Hasher>().unwrap();
    let mut model = Guarded::new_instance_on(&db, Attributes::new()).unwrap();
    assert!(model.force_fill(attrs("secret", "plaintext")).is_err());
    assert!(model.get_attribute("secret").is_none());
    let mut other = Guarded::new_instance_on(&db, Attributes::new()).unwrap();
    let (assigned, guarded) = tokio::join!(
        model.force_fill_async(attrs("secret", "plaintext")),
        other.fill_async(attrs("secret", "other"))
    );
    assigned.unwrap();
    assert!(matches!(guarded, Err(DatabaseError::MassAssignment(_))));
    let Some(Value::Text(hash)) = model.get_attribute("secret") else {
        panic!("missing hashed secret")
    };
    assert_ne!(hash, "plaintext");
    assert!(
        hasher
            .check(b"plaintext", hash, &HashOptions::new())
            .await
            .unwrap()
    );
    assert!(!model.to_array().unwrap().contains_key("secret"));
    let hash = hash.clone();
    model.force_fill(attrs("secret", hash.clone())).unwrap();
    assert_eq!(model.get_attribute("secret"), Some(&Value::Text(hash)));
    let mut snapshot = model.state().get_attributes().clone();
    let invalid = bcrypt::hash_with_result("invalid cost", 5)
        .unwrap()
        .format_for_version(bcrypt::Version::TwoY);
    assert!(
        model
            .force_fill_async(Attributes::from([
                ("label".into(), "changed".into()),
                ("secret".into(), invalid.into())
            ]))
            .await
            .is_err()
    );
    // 后一个转换器失败时，保留先前已经完成的字段赋值。
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/eloquent-strictness.json")).unwrap();
    snapshot.insert(
        "label".into(),
        reference["fill_setter_failure"]["label"]
            .as_str()
            .unwrap()
            .into(),
    );
    assert_eq!(model.state().get_attributes(), &snapshot);
    assert!(matches!(
        model
            .fill_async(attrs("label", "blocked after failure"))
            .await,
        Err(DatabaseError::MassAssignment(_))
    ));
    model
        .force_fill_async(attrs("secret", Value::Null))
        .await
        .unwrap();
    assert_eq!(model.get_attribute("secret"), Some(&Value::Null));
}

fn http_routes(
    _: &openrust::foundation::Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.get("/create", http_create)?;
    router.get("/invalid", http_invalid)?;
    Ok(router)
}
fn http_error(source: DatabaseError) -> openrust::http::HttpError {
    openrust::http::HttpError::Internal {
        source: Box::new(source),
    }
}
fn http_create(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let mut model = Guarded::query()
            .map_err(http_error)?
            .make(Attributes::new())
            .map_err(http_error)?;
        model
            .force_fill_async(Attributes::from([
                ("label".into(), "http label".into()),
                ("secret".into(), "http secret".into()),
                (
                    "payload".into(),
                    Value::Json(serde_json::json!({"source": "http"})),
                ),
            ]))
            .await
            .map_err(http_error)?;
        documented::force_change_password(&mut model, "documented password")
            .await
            .map_err(http_error)?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            model.to_array().map_err(http_error)?,
        )
    })
}
fn http_invalid(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let mut model = Guarded::query()
            .map_err(http_error)?
            .make(Attributes::new())
            .map_err(http_error)?;
        model
            .force_fill_async(attrs("label", Value::Null))
            .await
            .map_err(http_error)?;
        panic!("invalid mutator must return an error")
    })
}
fn configuration(
    _: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    Ok(std::collections::BTreeMap::from([
        (
            "app".into(),
            serde_json::json!({"env":"testing", "debug":false, "host":"127.0.0.1", "port":0}),
        ),
        (
            "database".into(),
            serde_json::json!({"default":"test", "connections":{"test":{"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}}}),
        ),
        (
            "hashing".into(),
            serde_json::json!({"driver":"bcrypt", "bcrypt":{"rounds":4}}),
        ),
    ]))
}

#[tokio::test]
async fn mysql_http_force_fill_persists_hashes_renders_errors_and_recovers() {
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    if url.is_none() {
        return;
    }
    let mut app = openrust::foundation::Application::configure(std::env::temp_dir())
        .with_config(configuration)
        .with_routing(openrust::foundation::configuration::Routing::new(
            http_routes,
        ))
        .with_exceptions(|exceptions| {
            exceptions.should_render_json_when(|request, _| request.expects_json());
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let db = app
        .container()
        .make::<openrust::database::DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let hasher = app.container().make::<dyn Hasher>().unwrap();
    db.statement("DROP TABLE IF EXISTS guarded", &[])
        .await
        .unwrap();
    db.statement("CREATE TABLE guarded (id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, label VARCHAR(80), secret VARCHAR(255), password VARCHAR(255), payload TEXT, created_at TIMESTAMP NULL, updated_at TIMESTAMP NULL)", &[]).await.unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = std::sync::Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handle_transport(request));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    for _ in 0..2 {
        let response = client
            .get(format!("http://{address}/create"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = response.text().await.unwrap();
        assert!(!body.contains("http secret") && !body.contains("$2y$"));
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["label"], "HTTP LABEL");
        assert_eq!(body["payload"], serde_json::json!({"source":"http"}));
        assert!(body.get("secret").is_none() && body.get("password").is_none());
        let record = Guarded::query_on(&db)
            .find_or_fail(body["id"].as_i64().unwrap())
            .await
            .unwrap();
        let Some(Value::Text(hash)) = record.get_attribute("secret") else {
            panic!("missing database hash")
        };
        assert!(
            hasher
                .check(b"http secret", hash, &HashOptions::new())
                .await
                .unwrap()
        );
        let Some(Value::Text(password)) = record.get_attribute("password") else {
            panic!("missing documentation password hash")
        };
        assert!(
            hasher
                .check(b"documented password", password, &HashOptions::new())
                .await
                .unwrap()
        );
        let response = client
            .get(format!("http://{address}/invalid"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert!(
            response.headers()["Content-Type"]
                .to_str()
                .unwrap()
                .contains("application/json")
        );
        let _: serde_json::Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
    }
    assert_eq!(Guarded::query_on(&db).get().await.unwrap().len(), 2);
    server.abort();
    let _ = server.await;
    db.statement("DROP TABLE guarded", &[]).await.unwrap();
}

struct PendingFill(ModelState);
impl Model for PendingFill {
    fn metadata() -> &'static ModelMetadata {
        Guarded::metadata()
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
    async fn fill_async(&mut self, _: Attributes) -> openrust::database::DatabaseResult<&mut Self> {
        let mut nested = Guarded::new_instance(Attributes::new())?;
        nested.force_fill_async(attrs("label", "nested")).await?;
        // Returning from an inner bypass must restore the still-active outer scope.
        nested.fill(attrs("label", "outer"))?;
        std::future::pending::<()>().await;
        Ok(self)
    }
}

#[tokio::test]
async fn force_fill_scope_is_restored_between_polls_and_after_cancellation() {
    use std::{future::Future as _, task::Poll};
    let mut pending = PendingFill::from_state(ModelState::default());
    {
        let mut work = Box::pin(pending.force_fill_async(Attributes::new()));
        std::future::poll_fn(|cx| {
            assert!(matches!(work.as_mut().poll(cx), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        let mut other = Guarded::new_instance(Attributes::new()).unwrap();
        assert!(matches!(
            other.fill(attrs("label", "outside poll")),
            Err(DatabaseError::MassAssignment(_))
        ));
        assert!(!Guarded::is_unguarded());
        // Dropping the suspended future cancels the bypass rather than leaving a global flag.
    }
    let mut after = Guarded::new_instance(Attributes::new()).unwrap();
    assert!(matches!(
        after.fill_async(attrs("label", "after cancellation")).await,
        Err(DatabaseError::MassAssignment(_))
    ));
}

#[tokio::test]
async fn public_guard_scope_reports_nested_state_and_restores_after_error() {
    assert!(!Guarded::is_unguarded());
    let guarded = Guarded::new_instance(Attributes::new()).unwrap();
    assert!(guarded.get_fillable().is_empty());
    assert_eq!(guarded.get_guarded(), &["*"]);
    assert_eq!(guarded.get_deleted_at_column(), None);
    let result: Result<(), &'static str> = Guarded::unguarded(async {
        assert!(Guarded::is_unguarded());
        let mut model = Guarded::new_instance(Attributes::new()).unwrap();
        model.fill(attrs("label", "unguarded")).unwrap();
        Guarded::unguarded(async { assert!(Guarded::is_unguarded()) }).await;
        assert!(Guarded::is_unguarded());
        assert!(
            !tokio::spawn(async { Guarded::is_unguarded() })
                .await
                .unwrap()
        );
        Err("stop")
    })
    .await;
    assert_eq!(result, Err("stop"));
    assert!(!Guarded::is_unguarded());
    let mut after = Guarded::new_instance(Attributes::new()).unwrap();
    assert!(after.fill(attrs("label", "blocked")).is_err());
}
