use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        http::Kernel as _,
    },
    database::{
        DB, DatabaseManager, DatabaseResult, Row, Transaction, Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    pagination::{Cursor, CursorOptions, PageOptions},
    routing::{Router, RoutingError},
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Article(ModelState);

impl Model for Article {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("pagination_articles").timestamps(None, None);
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

fn ids(rows: &[Row]) -> Vec<i64> {
    rows.iter().map(|row| id(row.get("id"))).collect()
}

fn id(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Integer(id)) => *id,
        Some(Value::Unsigned(id)) => i64::try_from(*id).unwrap(),
        _ => panic!("missing integer id"),
    }
}

fn routes(router: &mut Router) -> Result<(), RoutingError> {
    router.get("/length", length_page)?;
    router.get("/simple", simple_page)?;
    router.get("/cursor", cursor_page)?;
    Ok(())
}

fn length_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut page = Article::query()
            .map_err(internal)?
            .paginateWith(PageOptions {
                perPage: Some(2),
                pageName: "articles",
                ..Default::default()
            })
            .await
            .map_err(internal)?;
        page.withQueryString().fragment(Some("results here"));
        Response::json(StatusCode::OK, page.map(|article| id(article.getKey())))
    })
}

fn simple_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let page = DB::table("pagination_articles")
            .map_err(internal)?
            .orderBy("id", "asc")
            .simplePaginate(2)
            .await
            .map_err(internal)?;
        Response::json(StatusCode::OK, page.map(|row| id(row.get("id"))))
    })
}

fn cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut page = Article::query()
            .map_err(internal)?
            .orderBy("rank_number", "asc")
            .orderBy("id", "desc")
            .cursorPaginate(2)
            .await
            .map_err(internal)?;
        page.withQueryString();
        Response::json(StatusCode::OK, page.map(|article| id(article.getKey())))
    })
}

fn internal(source: openrust::database::DatabaseError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn request(address: SocketAddr, path: &str) -> Json {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{path} {response}");
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }).await.unwrap()
}

async fn select_count(connection: &mut Transaction) -> DatabaseResult<u64> {
    let row = connection
        .selectOne("SHOW SESSION STATUS LIKE 'Com_select'", &[])
        .await?
        .unwrap();
    let Some(Value::Text(count)) = row.get_at(1) else {
        panic!("invalid MySQL counter");
    };
    Ok(count.parse().unwrap())
}

async fn verify(config: Json) -> DatabaseResult<()> {
    let mysql = config.get("url").is_some();
    let mut app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    repository
        .set("database.default", json!("pagination_test"))
        .unwrap();
    repository
        .set("database.connections.pagination_test", config)
        .unwrap();
    let connection = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)?;
    for sql in [
        "DROP TABLE IF EXISTS pagination_articles",
        "CREATE TABLE pagination_articles (id BIGINT PRIMARY KEY, rank_number INTEGER, category VARCHAR(30))",
        "INSERT INTO pagination_articles VALUES (1, 1, 'a'), (2, 1, 'a'), (3, 2, 'b'), (4, 2, 'b'), (5, 3, 'b')",
    ] {
        connection.statement(sql, &[]).await?;
    }
    let table = || connection.table("pagination_articles");
    if mysql {
        let mut session = connection.beginTransaction().await?;
        let before = select_count(&mut session).await?;
        session
            .table("pagination_articles")
            .orderBy("id", "asc")
            .paginate(2)
            .await?;
        let counted = select_count(&mut session).await?;
        session
            .table("pagination_articles")
            .orderBy("id", "asc")
            .simplePaginate(2)
            .await?;
        let simple = select_count(&mut session).await?;
        session
            .table("pagination_articles")
            .orderBy("id", "asc")
            .cursorPaginate(2)
            .await?;
        assert_eq!(counted - before, 2);
        assert_eq!(simple - counted, 1);
        assert_eq!(select_count(&mut session).await? - simple, 1);
        session.rollBack(None).await?;
    }
    let grouped = table()
        .select(["category"])
        .groupBy(&["category"])
        .orderBy("category", "asc")
        .paginate(1)
        .await?;
    assert_eq!(grouped.total(), 2);
    let distinct = table()
        .select(["rank_number"])
        .distinct()
        .orderBy("rank_number", "asc")
        .paginate(2)
        .await?;
    assert_eq!(distinct.total(), 3);
    let empty = table().r#where("id", "<", 0).paginate(2).await?;
    assert!(empty.items().is_empty());
    assert_eq!((empty.total(), empty.lastPage()), (0, 1));
    assert!(table().paginate(0).await.is_err());
    assert!(table().cursorPaginate(2).await.is_err());
    assert!(
        table()
            .paginateWith(PageOptions {
                perPage: Some(2),
                page: Some(u64::MAX),
                ..Default::default()
            })
            .await
            .is_err()
    );

    let seek = || {
        table()
            .select(["id", "rank_number as score"])
            .r#where("category", "=", "a")
            .orWhere("category", "=", "b")
            .orderBy("score", "asc")
            .orderBy("id", "desc")
    };
    let first = seek().cursorPaginate(2).await?;
    assert_eq!(ids(first.items()), vec![2, 1]);
    let token = first.nextCursor().unwrap().encode()?;
    let cursor = Cursor::fromEncoded(&token)?;
    let second = seek()
        .cursorPaginateWith(CursorOptions {
            perPage: Some(2),
            cursor: Some(&cursor),
            ..Default::default()
        })
        .await?;
    assert_eq!(ids(second.items()), vec![4, 3]);
    let previous = second.previousCursor().unwrap();
    let back = seek()
        .cursorPaginateWith(CursorOptions {
            perPage: Some(2),
            cursor: Some(previous),
            ..Default::default()
        })
        .await?;
    assert_eq!(ids(back.items()), vec![2, 1]);
    assert!(back.previousCursor().is_none());
    assert!(
        table()
            .orderBy("category", "asc")
            .cursorPaginateWith(CursorOptions {
                perPage: Some(2),
                cursor: Some(&cursor),
                ..Default::default()
            })
            .await
            .is_err()
    );

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
    let (length, simple) = tokio::join!(
        request(address, "/api/length?articles=2&state=in+review",),
        request(address, "/api/simple?page=3",),
    );
    assert_eq!(length["data"], json!([3, 4]));
    assert_eq!(length["total"], 5);
    assert_eq!(
        length["next_page_url"],
        "/api/length?state=in+review&articles=3#results%20here"
    );
    assert_eq!(length["links"][2]["active"], true);
    assert_eq!(length["links"][2]["page"], 2);
    assert_eq!(simple["data"], json!([5]));
    assert!(simple["next_page_url"].is_null());
    assert!(simple.get("total").is_none());
    let first = request(address, "/api/cursor?state=open&cursor=malformed").await;
    assert_eq!(first["data"], json!([2, 1]));
    let second = request(address, first["next_page_url"].as_str().unwrap()).await;
    assert_eq!(second["data"], json!([4, 3]));
    let last = request(address, second["next_page_url"].as_str().unwrap()).await;
    assert_eq!(last["data"], json!([5]));
    assert!(last["next_page_url"].is_null());
    assert_eq!(
        request(address, second["prev_page_url"].as_str().unwrap()).await["data"],
        json!([2, 1])
    );
    server.abort();
    connection
        .statement("DROP TABLE pagination_articles", &[])
        .await?;
    Ok(())
}

#[tokio::test]
async fn pagination_on_sqlite_and_http() {
    verify(json!({"driver":"sqlite","database":":memory:"}))
        .await
        .unwrap();
}

#[tokio::test]
async fn pagination_on_mysql_and_http() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url})).await.unwrap();
    }
}
