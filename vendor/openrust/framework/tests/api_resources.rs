use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    database::{
        DatabaseError, DatabaseManager, Value,
        eloquent::{Attributes, Model, ModelMetadata, ModelState, relations::HasMany},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        HttpError, HttpFuture, Method, Request, Response,
        resources::json::{
            Conditional, JsonResource, ModelResource, ResourceCollection, ResourceItems,
            ResourceResult,
        },
    },
    routing::{Router, RoutingError},
};
use serde_json::{Map, Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Account(ModelState);

impl Model for Account {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("resource_accounts")
            .timestamps(None, None)
            .fillable(&["name", "password"])
            .hidden(&["password"]);
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

struct AccountResource(Account);

impl JsonResource for AccountResource {
    fn toArray(&self, request: &Request) -> ResourceResult<Json> {
        let mut fields = self.0.attributesToArray().map_err(internal)?;
        Conditional::insert(
            &mut fields,
            "admin",
            Conditional::when(request.headers().contains_key("x-test-admin"), || {
                json!(true)
            }),
        );
        Conditional::insert(
            &mut fields,
            "reports",
            Conditional::whenLoaded::<Vec<Arc<Account>>>(&self.0, "directReports")
                .map(|accounts| json!(accounts.len())),
        );
        Conditional::insert(
            &mut fields,
            "report_count",
            Conditional::whenCounted(&self.0, "directReports").map(|count| match count {
                Value::Integer(value) => json!(value),
                Value::Unsigned(value) => json!(value),
                _ => panic!("expected an integer count"),
            }),
        );
        Ok(Json::Object(fields))
    }

    fn with(&self, _: &Request) -> ResourceResult<Map<String, Json>> {
        Ok(Map::from_iter([(
            "meta".into(),
            json!({"source":"resource"}),
        )]))
    }

    fn withResponse(&self, _: &Request, response: &mut Response) -> ResourceResult<()> {
        response
            .parts_mut()
            .headers
            .insert("x-resource", "account".parse().unwrap());
        Ok(())
    }
}

struct Plain(Json);

impl JsonResource for Plain {
    fn toArray(&self, _: &Request) -> ResourceResult<Json> {
        Ok(self.0.clone())
    }
}

fn internal(source: DatabaseError) -> HttpError {
    if matches!(source, DatabaseError::ModelNotFound(_)) {
        return HttpError::NotFound;
    }
    HttpError::Internal {
        source: Box::new(source),
    }
}

fn routes(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/one", || async {
        let account = Account::query()
            .map_err(internal)?
            .with(
                "directReports",
                HasMany::<Account, Account>::new("id", "manager_id"),
            )
            .withCount(
                "directReports",
                &HasMany::<Account, Account>::new("id", "manager_id"),
            )
            .map_err(internal)?
            .findOrFail(1)
            .await
            .map_err(internal)?;
        Ok(AccountResource(account))
    })?;
    router.get("/missing", || async {
        let account = Account::findOrFail(99).await.map_err(internal)?;
        Ok(AccountResource(account))
    })?;
    router.get("/list", || async {
        let accounts = Account::query()
            .map_err(internal)?
            .orderBy("id", "asc")
            .get()
            .await
            .map_err(internal)?;
        Ok(ResourceCollection::new(
            accounts
                .into_iter()
                .map(AccountResource)
                .collect::<Vec<_>>(),
        ))
    })?;
    router.get("/page", page)?;
    router.get("/simple", || async {
        let page = Account::query()
            .map_err(internal)?
            .simplePaginate(2)
            .await
            .map_err(internal)?;
        Ok(ResourceCollection::new(
            ResourceItems::from(page).map(AccountResource),
        ))
    })?;
    router.get("/cursor", || async {
        let page = Account::query()
            .map_err(internal)?
            .cursorPaginate(2)
            .await
            .map_err(internal)?;
        Ok(ResourceCollection::new(
            ResourceItems::from(page).map(AccountResource),
        ))
    })?;
    router.get("/empty", || ResourceCollection::new(Vec::<Plain>::new()))?;
    router.get("/already", || Plain(json!({"data":{"id":9}})))?;
    router.get("/unwrapped", unwrapped)?;
    router.get("/metadata", metadata)?;
    router.get("/failed", || -> ResourceResult<Plain> {
        Err(HttpError::InvalidRequest)
    })?;
    router.r#match(&[Method::POST], "/new", || async {
        let new = Account::create(Attributes::from([
            ("name".into(), "New".into()),
            ("password".into(), "hidden".into()),
        ]))
        .await
        .map_err(internal)?;
        Ok(ModelResource(new))
    })?;
    Ok(())
}

fn page(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        let page = Account::query()
            .map_err(internal)?
            .paginate(2)
            .await
            .map_err(internal)?;
        ResourceCollection::new(ResourceItems::from(page).map(AccountResource))
            .preserveQuery()
            .withPaginationInformation(|_, mut info| {
                info.meta.insert("source".into(), json!("page"));
                Ok(info)
            })
            .response()
            .withoutWrapping()
            .additional(Map::from_iter([(
                "meta".into(),
                json!({"request":"accepted"}),
            )]))
            .toResponse(request)
    })
}

fn unwrapped(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        Plain(json!({"id":1}))
            .response()
            .withoutWrapping()
            .toResponse(request)
    })
}

fn metadata(request: &mut Request) -> HttpFuture<'_, ResourceResult<Response>> {
    Box::pin(async move {
        Plain(json!({"id":1}))
            .response()
            .withoutWrapping()
            .additional(Map::from_iter([(
                "meta".into(),
                json!({"tags":["a"],"version":1}),
            )]))
            .additional(Map::from_iter([(
                "meta".into(),
                json!({"tags":["b"],"version":2}),
            )]))
            .toResponse(request)
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
    status: u16,
) -> (String, Json) {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n{headers}\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{path} {response}");
        let (header, body) = response.split_once("\r\n\r\n").unwrap();
        (header.to_owned(), serde_json::from_str(body).unwrap())
    }).await.unwrap()
}

async fn verify(config: Json) {
    let mysql = config.get("url").is_some();
    let mut app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    repository
        .set("database.default", json!("resource_test"))
        .unwrap();
    repository
        .set("database.connections.resource_test", config)
        .unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let id = if mysql {
        "BIGINT AUTO_INCREMENT PRIMARY KEY"
    } else {
        "INTEGER PRIMARY KEY"
    };
    for sql in [
        "DROP TABLE IF EXISTS resource_accounts".into(),
        format!("CREATE TABLE resource_accounts (id {id}, manager_id BIGINT, name VARCHAR(40), password VARCHAR(40))"),
        "INSERT INTO resource_accounts VALUES (1, NULL, 'A', 'secret'), (2, 1, 'B', 'secret'), (3, NULL, 'C', 'secret')".into(),
    ] {
        connection.statement(&sql, &[]).await.unwrap();
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
    let ((headers, admin), (_, guest)) = tokio::join!(
        request(address, "GET", "/api/one", "X-Test-Admin: yes\r\n", 200),
        request(address, "GET", "/api/one", "", 200),
    );
    assert!(headers.contains("x-resource: account"));
    assert_eq!(admin["data"]["admin"], true);
    assert!(guest["data"].get("admin").is_none());
    assert!(guest["data"].get("password").is_none());
    assert_eq!(guest["data"]["manager_id"], Json::Null);
    assert_eq!(guest["data"]["reports"], 1);
    assert_eq!(guest["data"]["report_count"], 1);
    assert_eq!(guest["meta"]["source"], "resource");
    request(address, "GET", "/api/missing", "", 404).await;
    request(address, "GET", "/api/failed", "", 400).await;

    let (_, list) = request(address, "GET", "/api/list", "", 200).await;
    assert_eq!(list["data"].as_array().unwrap().len(), 3);
    assert!(list["data"][0].get("reports").is_none());
    assert!(list.get("meta").is_none());
    assert_eq!(
        request(address, "GET", "/api/empty", "", 200).await.1,
        json!({"data":[]})
    );
    assert_eq!(
        request(address, "GET", "/api/already", "", 200).await.1,
        json!({"data":{"id":9}})
    );
    assert_eq!(
        request(address, "GET", "/api/unwrapped", "", 200).await.1,
        json!({"id":1})
    );

    assert_eq!(
        request(address, "GET", "/api/metadata", "", 200).await.1,
        json!({"data":{"id":1},"meta":{"tags":["a","b"],"version":[1,2]}})
    );
    let (_, page) = request(address, "GET", "/api/page?filter=all", "", 200).await;
    assert_eq!(page["data"].as_array().unwrap().len(), 2);
    assert_eq!(page["links"]["next"], "/api/page?filter=all&page=2");
    assert_eq!(page["meta"]["total"], 3);
    assert_eq!(page["meta"]["source"], "page");
    assert_eq!(page["meta"]["request"], "accepted");
    assert!(page["meta"].get("next_page_url").is_none());
    assert_eq!(
        request(address, "GET", "/api/page?page=99", "", 200)
            .await
            .1["data"],
        json!([])
    );
    let (_, simple) = request(address, "GET", "/api/simple", "", 200).await;
    assert!(simple["meta"].get("total").is_none());
    let (_, cursor) = request(address, "GET", "/api/cursor", "", 200).await;
    assert!(cursor["meta"]["next_cursor"].is_string());
    let (_, last) = request(
        address,
        "GET",
        cursor["links"]["next"].as_str().unwrap(),
        "",
        200,
    )
    .await;
    assert_eq!(last["data"][0]["id"], 3);
    assert!(last["links"]["next"].is_null());
    let (_, created) = request(address, "POST", "/api/new", "", 201).await;
    assert_eq!(created["data"]["name"], "New");
    assert!(created["data"].get("password").is_none());
    server.abort();
    connection
        .statement("DROP TABLE resource_accounts", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn resource_responses_on_sqlite() {
    verify(json!({"driver":"sqlite","database":":memory:"})).await;
}

#[tokio::test]
async fn resource_responses_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await;
    }
}
