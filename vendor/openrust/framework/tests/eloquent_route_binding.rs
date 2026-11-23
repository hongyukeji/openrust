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
        DatabaseError, DatabaseManager, Value as DbValue,
        eloquent::{
            Model, ModelMetadata, ModelState, UniqueId,
            casts::{Attribute, AttributeCast},
            relations::HasMany,
        },
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{BindingContext, RouteParameter, RoutingError, SignatureMode, UrlError},
    support::facades::{Route, Url},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Ticket<const ULID: bool>(ModelState);

impl<const ULID: bool> Model for Ticket<ULID> {
    fn metadata() -> &'static ModelMetadata {
        static UUID: ModelMetadata = ModelMetadata::new("route_tickets")
            .timestamps(None, None)
            .unique_id(UniqueId::Uuid)
            .unique_ids(&["id", "reference"]);
        static ULID_META: ModelMetadata = ModelMetadata::new("route_tickets")
            .timestamps(None, None)
            .unique_id(UniqueId::Ulid)
            .unique_ids(&["id", "reference"]);
        if ULID { &ULID_META } else { &UUID }
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

async fn create_ticket<const ULID: bool>(db: &openrust::database::Connection) -> (String, String) {
    let mut ticket = Ticket::<ULID>::new_instance_on(db, Default::default()).unwrap();
    ticket.set_attribute("id", "0");
    let supplied = if ULID {
        "01arz3ndektsv4rrffq69g5fav"
    } else {
        "00000000-0000-4000-8000-000000000042"
    };
    ticket.set_attribute("reference", supplied);
    assert!(ticket.save().await.unwrap());
    assert_eq!(ticket.get_attribute("reference"), Some(&supplied.into()));

    let mut replica = ticket.replicate(&[]).await.unwrap();
    assert!(replica.get_key().is_none());
    assert!(replica.get_attribute("reference").is_none());
    assert!(replica.save().await.unwrap());
    let (Some(DbValue::Text(id)), Some(DbValue::Text(reference))) =
        (replica.get_key(), replica.get_attribute("reference"))
    else {
        panic!("replica must generate both identifiers");
    };
    assert_ne!(replica.get_key(), ticket.get_key());
    assert_ne!(id, reference);
    for value in [id, reference] {
        if ULID {
            assert_eq!(value.len(), 26);
            assert_eq!(value, &value.to_ascii_lowercase());
            assert!(ulid::Ulid::from_string(value).is_ok());
        } else {
            assert_eq!(uuid::Uuid::parse_str(value).unwrap().get_version_num(), 7);
        }
    }
    (id.clone(), reference.clone())
}

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
            .soft_deletes("deleted_at");
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

    fn attribute(name: &str) -> Option<Attribute> {
        match name {
            "slug" => Some(Attribute::get(|raw, _| match raw {
                Some(DbValue::Text(slug)) => Ok(slug.to_uppercase().into()),
                _ => Ok(DbValue::Null),
            })),
            "public_slug" => Some(Attribute::get(|_, attributes| {
                match attributes.get("slug") {
                    Some(DbValue::Text(slug)) => Ok(format!("public-{slug}").into()),
                    _ => Ok(DbValue::Null),
                }
            })),
            "broken_slug" => Some(Attribute::get(|_, _| Err(DatabaseError::InvalidScalar))),
            _ => None,
        }
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("padded", AttributeCast::Integer)]
    }

    fn get_route_key_name() -> &'static str {
        "slug"
    }

    fn resolve_child_route_binding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        Box::pin(async move {
            let Some(account) = binding.parent::<Account>() else {
                return Ok(None);
            };
            HasMany::<Account, Post>::new("id", "account_id")
                .resolve_route_binding(account, binding)
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
        let Some(DbValue::Text(slug)) = post.get_attribute("slug") else {
            return Err(HttpError::InvalidRequest);
        };
        let named = RouteParameter::model("post", post)?;
        let positional = RouteParameter::positional_model(post)?;
        assert!(matches!(
            Url::route("missing-column", std::slice::from_ref(&named), false),
            Err(UrlError::MissingParameter)
        ));
        assert!(matches!(
            Url::route("broken-column", std::slice::from_ref(&named), false),
            Err(UrlError::ModelAttribute(DatabaseError::InvalidScalar))
        ));
        Response::json(
            StatusCode::OK,
            json!({
                "slug": slug,
                "by_id": Url::route("post-id", std::slice::from_ref(&named), false)?,
                "named_accessor": Url::route("post-accessor", std::slice::from_ref(&named), false)?,
                "position_accessor": Url::route("post-accessor", std::slice::from_ref(&positional), false)?,
                "by_cast": Url::route("post-cast", std::slice::from_ref(&named), false)?,
                "signed_accessor": Url::signed_route(
                    "post-accessor-signed", std::slice::from_ref(&named), None, SignatureMode::Relative
                )?,
                "signed": Url::signed_route("post-signed", &[named], None, SignatureMode::Relative)?,
                "by_slug": Url::route("post-slug", std::slice::from_ref(&positional), false)?,
                "action": Url::action("PostController@byId", &[positional], false)?,
            }),
        )
    })
}

fn empty(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn custom_missing(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::GONE)) })
}

fn api() -> Result<(), RoutingError> {
    Route::get_with("/uuid/{ticket}", empty, |route| {
        route.model::<Ticket<false>>("ticket")?;
        Ok(())
    })?;
    Route::get_with("/ulid/{ticket:reference}", empty, |route| {
        route.model::<Ticket<true>>("ticket")?;
        Ok(())
    })?;
    Route::get_with("/accounts/{account}/posts/{post:slug}", lookup, |route| {
        route
            .model::<Account>("account")?
            .model::<Post>("post")?
            .middleware(Arc::new(RequiresBoundPost));
        Ok(())
    })?;
    Route::get_with("/unscoped/{account}/{post:slug}", lookup, |route| {
        route
            .model::<Account>("account")?
            .model::<Post>("post")?
            .without_scoped_bindings();
        Ok(())
    })?;
    Route::get_with("/scoped/{account}/{post}", lookup, |route| {
        route
            .model::<Account>("account")?
            .model::<Post>("post")?
            .scope_bindings();
        Ok(())
    })?;
    Route::get_with("/id/{post:id}", lookup, |route| {
        route
            .model::<Post>("post")?
            .name("post-id")
            .set_action("PostController@byId")?;
        Ok(())
    })?;
    Route::get_with("/signed/{post:id}", lookup, |route| {
        route
            .model::<Post>("post")?
            .name("post-signed")
            .middleware_name("signed:relative");
        Ok(())
    })?;
    for (path, name) in [
        ("/accessor/{post:public_slug}", "post-accessor"),
        ("/cast/{post:padded}", "post-cast"),
        ("/broken/{post:broken_slug}", "broken-column"),
    ] {
        Route::get_with(path, empty, |route| {
            route.name(name);
            Ok(())
        })?;
    }
    Route::get_with("/signed-accessor/{post:public_slug}", empty, |route| {
        route
            .name("post-accessor-signed")
            .middleware_name("signed:relative");
        Ok(())
    })?;
    Route::get_with("/missing-column/{post:unknown}", lookup, |route| {
        route.name("missing-column");
        Ok(())
    })?;
    Route::get_with("/trashed/{post}", lookup, |route| {
        route.model::<Post>("post")?.with_trashed();
        Ok(())
    })?;
    Route::get_with("/missing/{post}", lookup, |route| {
        route.model::<Post>("post")?.missing(custom_missing);
        Ok(())
    })
}

fn web() -> Result<(), RoutingError> {
    Route::model::<Post>("explicit")?;
    Route::bind("alias", |binding| {
        Box::pin(async move { Ok(Some(binding.value().to_uppercase())) })
    })?;
    Route::get_with("/posts/{explicit}", lookup, |route| {
        route.name("post-slug");
        Ok(())
    })?;
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
    kernel.handle_transport(request).await
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
    let root = tempfile::tempdir().unwrap();
    let application = || {
        Application::configure(root.path().to_owned())
            .with_config(|_, _| Ok([("app".into(), json!({"key":"a".repeat(32)}))].into()))
            .with_routing(Routing::default().api(api).web(web))
            .create()
            .unwrap()
    };
    assert_eq!(
        application()
            .handle_command(&["route:cache".into()])
            .await
            .unwrap(),
        0
    );
    let mut app = application();
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
        "DROP TABLE IF EXISTS route_tickets",
        "DROP TABLE IF EXISTS route_posts",
        "DROP TABLE IF EXISTS route_accounts",
        "CREATE TABLE route_tickets (id VARCHAR(36) PRIMARY KEY, reference VARCHAR(36) NOT NULL UNIQUE)",
        "CREATE TABLE route_accounts (id BIGINT PRIMARY KEY)",
        "CREATE TABLE route_posts (id BIGINT PRIMARY KEY, account_id BIGINT, slug VARCHAR(40), padded VARCHAR(12), deleted_at VARCHAR(30))",
        "INSERT INTO route_accounts VALUES (1), (2)",
        "INSERT INTO route_posts VALUES (1, 1, 'first', '007', NULL), (2, 2, 'second', '008', NULL), (3, 1, 'deleted', '009', '2026-01-01 00:00:00')",
    ] {
        connection.statement(sql, &[]).await.unwrap();
    }
    let (uuid, _) = create_ticket::<false>(&connection).await;
    let (_, ulid_reference) = create_ticket::<true>(&connection).await;
    connection
        .statement(
            "INSERT INTO route_tickets VALUES ('bad-uuid', 'bad-ulid')",
            &[],
        )
        .await
        .unwrap();
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
    let generated: Value =
        serde_json::from_str(&request(address, "/posts/first", 200).await).unwrap();
    assert_eq!(generated["by_id"], "/api/id/1");
    assert_eq!(generated["action"], generated["by_id"]);
    assert_eq!(generated["by_slug"], "/posts/FIRST");
    assert_eq!(generated["named_accessor"], "/api/accessor/public-first");
    assert_eq!(generated["position_accessor"], generated["named_accessor"]);
    assert_eq!(generated["by_cast"], "/api/cast/7");
    request(address, generated["named_accessor"].as_str().unwrap(), 204).await;
    let signed_accessor = generated["signed_accessor"].as_str().unwrap();
    assert!(signed_accessor.starts_with("/api/signed-accessor/public-first?signature="));
    request(address, signed_accessor, 204).await;
    request(
        address,
        &signed_accessor.replace("public-first", "public-second"),
        403,
    )
    .await;
    request(address, generated["by_id"].as_str().unwrap(), 200).await;
    request(address, "/api/id/first", 404).await;
    let signed = generated["signed"].as_str().unwrap();
    assert!(signed.starts_with("/api/signed/1?signature="));
    request(address, signed, 200).await;
    request(
        address,
        &signed.replacen("/signed/1?", "/signed/2?", 1),
        403,
    )
    .await;
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
    request(address, &format!("/api/uuid/{uuid}"), 204).await;
    request(address, &format!("/api/ulid/{ulid_reference}"), 204).await;
    request(address, "/api/uuid/bad-uuid", 404).await;
    request(address, "/api/ulid/bad-ulid", 404).await;
    request(address, "/api/missing/absent", 410).await;
    assert_eq!(
        request(address, "/alias/test", 200).await,
        r#"{"alias":"TEST"}"#
    );
    server.abort();
    for table in ["route_posts", "route_accounts", "route_tickets"] {
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
