use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        config::Repository,
        container::Container as _,
        foundation::Application as _,
        http::{Kernel as _, Middleware},
    },
    database::{
        DatabaseManager, Value as DbValue,
        eloquent::{Model, ModelMetadata, ModelState, relations::HasMany},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{BindingContext, Router, RoutingError},
    support::facades::Route,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Account(ModelState);

impl Model for Account {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("route_accounts").timestamps(None, None);
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

struct Post(ModelState);

impl Model for Post {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("route_posts")
            .timestamps(None, None)
            .softDeletes("deleted_at");
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

    fn getRouteKeyName() -> &'static str {
        "slug"
    }

    fn resolveChildRouteBinding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        Box::pin(async move {
            let Some(account) = binding.parent::<Account>() else {
                return Ok(None);
            };
            HasMany::<Account, Post>::new("id", "account_id")
                .resolveRouteBinding(account, binding)
                .await
        })
    }
}

struct RequiresBoundPost;

impl Middleware for RequiresBoundPost {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.route_model::<Post>("post").is_none() {
                return Err(HttpError::InvalidRequest);
            }
            Ok(next.run(request).await)
        })
    }
}

fn lookup(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let post = request
            .route_model::<Post>("post")
            .or_else(|| request.route_model::<Post>("explicit"))
            .ok_or(HttpError::NotFound)?;
        let Some(DbValue::Text(slug)) = post.getRouteKey() else {
            return Err(HttpError::InvalidRequest);
        };
        Response::json(StatusCode::OK, json!({"slug":slug}))
    })
}

fn custom_missing(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::GONE)) })
}

fn api(router: &mut Router) -> Result<(), RoutingError> {
    router
        .get("/accounts/{account}/posts/{post:slug}", lookup)?
        .model::<Account>("account")?
        .model::<Post>("post")?
        .middleware(Arc::new(RequiresBoundPost));
    router
        .get("/unscoped/{account}/{post:slug}", lookup)?
        .model::<Account>("account")?
        .model::<Post>("post")?
        .withoutScopedBindings();
    router
        .get("/scoped/{account}/{post}", lookup)?
        .model::<Account>("account")?
        .model::<Post>("post")?
        .scopeBindings();
    router.get("/id/{post:id}", lookup)?.model::<Post>("post")?;
    router
        .get("/trashed/{post}", lookup)?
        .model::<Post>("post")?
        .withTrashed();
    router
        .get("/missing/{post}", lookup)?
        .model::<Post>("post")?
        .missing(custom_missing);
    Ok(())
}

fn web() -> Result<(), RoutingError> {
    Route::model::<Post>("explicit")?;
    Route::bind("alias", |binding| {
        Box::pin(async move { Ok(Some(binding.value().to_uppercase())) })
    })?;
    Route::get("/posts/{explicit}", lookup)?;
    Route::get("/alias/{alias}", alias)?;
    Ok(())
}

fn alias(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({"alias":request.route_model::<String>("alias")}),
        )
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(address: SocketAddr, path: &str, status: u16) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path} {response}");
        response.split_once("\r\n\r\n").unwrap().1.to_owned()
    }).await.unwrap()
}

async fn verify(config: Value) {
    let mut app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::default().api(api).web(web))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    repository
        .set("database.default", json!("route_test"))
        .unwrap();
    repository
        .set("database.connections.route_test", config)
        .unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    for sql in [
        "DROP TABLE IF EXISTS route_posts",
        "DROP TABLE IF EXISTS route_accounts",
        "CREATE TABLE route_accounts (id BIGINT PRIMARY KEY)",
        "CREATE TABLE route_posts (id BIGINT PRIMARY KEY, account_id BIGINT, slug VARCHAR(40), deleted_at VARCHAR(30))",
        "INSERT INTO route_accounts VALUES (1), (2)",
        "INSERT INTO route_posts VALUES (1, 1, 'first', NULL), (2, 2, 'second', NULL), (3, 1, 'deleted', '2026-01-01 00:00:00')",
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
    for (path, slug) in [
        ("/posts/first", "first"),
        ("/api/id/1", "first"),
        ("/api/accounts/1/posts/first", "first"),
        ("/api/unscoped/1/second", "second"),
        ("/api/scoped/2/second", "second"),
        ("/api/trashed/deleted", "deleted"),
    ] {
        let response = request(address, path, 200).await;
        assert_eq!(
            serde_json::from_str::<Value>(&response).unwrap()["slug"],
            slug
        );
    }
    for path in [
        "/posts/deleted",
        "/api/id/1x",
        "/api/id/18446744073709551615",
        "/api/accounts/1/posts/second",
        "/api/accounts/42/posts/first",
        "/api/scoped/1/second",
    ] {
        request(address, path, 404).await;
    }
    request(address, "/api/missing/absent", 410).await;
    assert_eq!(
        request(address, "/alias/test", 200).await,
        r#"{"alias":"TEST"}"#
    );
    server.abort();
    for table in ["route_posts", "route_accounts"] {
        connection
            .statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn eloquent_bindings_run_in_http_middleware_on_sqlite() {
    verify(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn eloquent_bindings_run_in_http_middleware_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await;
    }
}
