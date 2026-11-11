use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{
        AuthFuture, AuthIdentifier, AuthManager, AuthResult,
        access::{Argument, GateRegistry, Policy, PolicyTarget, Response as Decision},
        middleware::Authorize,
    },
    contracts::{
        auth::Authenticatable, config::Repository, container::Container as _,
        foundation::Application as _, http::Kernel as _,
    },
    database::{
        DatabaseManager, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{Router, RoutingError},
    support::facades::Gate,
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

#[derive(Clone)]
struct Identity(&'static str);

impl Authenticatable for Identity {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        AuthIdentifier::String(self.0.into())
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}

fn identity(user: Option<&dyn Authenticatable>, expected: &str) -> bool {
    matches!(user.map(Authenticatable::get_auth_identifier),
        Some(AuthIdentifier::String(id)) if id == expected)
}

struct Document(ModelState);

impl Model for Document {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("gate_test_documents").timestamps(None, None);
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

struct DocumentPolicy;

impl Policy<Document> for DocumentPolicy {
    fn has_ability(&self, ability: &str) -> bool {
        matches!(ability, "update" | "create" | "conceal")
    }

    fn before<'a>(
        &'a self,
        user: Option<&'a dyn Authenticatable>,
        _: &'a str,
        _: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Option<Decision>>> {
        Box::pin(async move { Ok(identity(user, "editor").then(Decision::allow)) })
    }

    fn evaluate<'a>(
        &'a self,
        ability: &'a str,
        user: Option<&'a dyn Authenticatable>,
        document: Option<&'a Document>,
        args: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Option<Decision>>> {
        Box::pin(async move {
            let decision = match ability {
                "conceal" => Decision::denyAsNotFound("Document is unavailable."),
                "create" => Decision::from(document.is_none() && identity(user, "owner")),
                "update"
                    if document.is_some_and(|document| {
                        document.getAttribute("owner") == Some(&Value::from("owner"))
                    }) && identity(user, "owner")
                        && args
                            .first()
                            .and_then(|arg| arg.downcast_ref::<String>())
                            .is_some_and(|scope| scope == "internal") =>
                {
                    Decision::allow()
                }
                _ => Decision::deny("Only the owner may edit this document."),
            };
            Ok(Some(decision))
        })
    }
}

fn status(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Response::json(StatusCode::OK, json!({"ok":true})) })
}

fn explicit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let _ = Gate::authorize(request, "user-only", &[]).await?;
        let scope = Gate::forUser(Some(Arc::new(Identity("owner"))))?;
        assert!(scope.check(&["user-only", "public"], &[]).await?);
        assert!(scope.any(&["unknown", "public"], &[]).await?);
        assert!(scope.none(&["unknown", "denied"], &[]).await?);
        assert!(scope.allowIf(|user| identity(Some(user), "owner")).is_ok());
        assert!(scope.denyIf(|user| identity(Some(user), "owner")).is_err());
        Response::json(StatusCode::OK, json!({"ok":true}))
    })
}

fn api(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/public", status)?.can("public", &[]);
    router.get("/user", explicit)?;
    router.get("/before", status)?.can("before", &[]);
    router.get("/after", status)?.can("after", &[]);
    router.get("/denied", status)?.can("denied", &[]);
    router.get("/unknown", status)?.can("unknown", &[]);
    router.get("/status", status)?.can("status", &[]);
    router
        .get("/update/{document}", status)?
        .model::<Document>("document")?
        .can("update", &["document", "'internal'"])
        .middleware_name("auth");
    router
        .get("/conceal/{document}", status)?
        .model::<Document>("document")?
        .can("conceal", &["document"]);
    router.get("/create", status)?.middleware(Arc::new(
        Authorize::new("create", std::iter::empty::<String>())
            .with_argument(PolicyTarget::of::<Document>()),
    ));
    Ok(())
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(address: SocketAddr, path: &str, user: &str, status: u16) -> Json {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET /api{path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\nX-Test-User: {user}\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path} {response}");
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }).await.unwrap()
}

fn register(registry: &GateRegistry, observed: &Arc<AtomicUsize>, public: bool) {
    registry.defineGuest("public", move |_, _| {
        Box::pin(async move { Ok(public.into()) })
    });
    registry.define("user-only", |_, _| {
        Box::pin(async { Ok(Decision::allow()) })
    });
    registry.define("denied", |_, _| {
        Box::pin(async { Ok(Decision::deny("Explicit denial.")) })
    });
    registry.define("status", |_, _| {
        Box::pin(async {
            Ok(Decision::denyWithStatus(
                StatusCode::CONFLICT,
                "Document is locked.",
            ))
        })
    });
    // A typed policy with a matching ability takes precedence over the named gate.
    registry.define("update", |_, _| {
        Box::pin(async { Ok(Decision::deny("Named callback was reached.")) })
    });
    registry.policy(DocumentPolicy);
    registry.defaultDenialResponse(Decision::deny("No applicable ability."));
    registry.before(|user, ability, _| {
        Box::pin(async move {
            Ok((ability == "before" && identity(user, "admin")).then(Decision::allow))
        })
    });
    let observed = observed.clone();
    registry.after(move |_, ability, result, _| {
        if ability == "before" && result.is_some_and(Decision::allowed) {
            observed.fetch_add(1, Ordering::Relaxed);
        }
        Box::pin(async move { Ok(matches!(ability, "after" | "denied").then(Decision::allow)) })
    });
}

async fn start(
    config: Json,
    public: bool,
) -> (
    SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<AtomicUsize>,
    openrust::database::Connection,
) {
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "auth".into(),
                json!({
                    "defaults":{"guard":"test"},
                    "guards":{"test":{"driver":"header"}}
                }),
            )]
            .into())
        })
        .withRouting(Routing::default().api(api))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    repository.set("database.default", json!("gate")).unwrap();
    repository.set("database.connections.gate", config).unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let auth = app.container().make::<AuthManager>().unwrap();
    auth.viaRequest("header", |request| {
        Box::pin(async move {
            let user = match request
                .headers()
                .get("x-test-user")
                .and_then(|header| header.to_str().ok())
            {
                Some("owner") => Some(Identity("owner")),
                Some("editor") => Some(Identity("editor")),
                Some("admin") => Some(Identity("admin")),
                Some("other") => Some(Identity("other")),
                _ => None,
            };
            Ok(user.map(|user| Box::new(user) as Box<dyn Authenticatable>))
        })
    });
    let observed = Arc::new(AtomicUsize::new(0));
    register(
        &app.container().make::<GateRegistry>().unwrap(),
        &observed,
        public,
    );
    for sql in [
        "DROP TABLE IF EXISTS gate_test_documents",
        "CREATE TABLE gate_test_documents (id BIGINT PRIMARY KEY, owner VARCHAR(30))",
        "INSERT INTO gate_test_documents VALUES (1, 'owner')",
    ] {
        connection.statement(sql, &[]).await.unwrap();
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (address, server, observed, connection)
}

async fn verify(config: Json) {
    let (address, server, observed, connection) = start(config, true).await;
    request(address, "/public", "", 200).await;
    request(address, "/user", "", 403).await;
    request(address, "/user", "owner", 200).await;
    request(address, "/before", "admin", 200).await;
    assert_eq!(observed.load(Ordering::Relaxed), 1);
    request(address, "/after", "", 200).await;
    assert_eq!(
        request(address, "/denied", "owner", 403).await["message"],
        "Explicit denial."
    );
    assert_eq!(
        request(address, "/unknown", "", 403).await["message"],
        "No applicable ability."
    );
    assert_eq!(
        request(address, "/status", "owner", 409).await["message"],
        "Document is locked."
    );
    request(address, "/update/99", "", 401).await;
    request(address, "/update/99", "owner", 404).await;
    let (_, denied) = tokio::join!(
        request(address, "/update/1", "owner", 200),
        request(address, "/update/1", "other", 403),
    );
    assert_eq!(denied["message"], "Only the owner may edit this document.");
    request(address, "/update/1", "editor", 200).await;
    assert_eq!(
        request(address, "/conceal/1", "owner", 404).await["message"],
        "Document is unavailable."
    );
    request(address, "/create", "owner", 200).await;

    let (isolated, other, _, isolated_db) =
        start(json!({"driver":"sqlite","database":":memory:"}), false).await;
    let _ = tokio::join!(
        request(address, "/public", "", 200),
        request(isolated, "/public", "", 403)
    );
    other.abort();
    server.abort();
    isolated_db
        .statement("DROP TABLE gate_test_documents", &[])
        .await
        .unwrap();
    connection
        .statement("DROP TABLE gate_test_documents", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn authorization_over_http_with_sqlite() {
    verify(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn authorization_over_http_with_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await;
    }
}
