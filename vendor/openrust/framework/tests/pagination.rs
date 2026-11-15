use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _,
        http::Kernel as _, view::View as _,
    },
    database::{
        DB, DatabaseManager, DatabaseResult, Row, Transaction, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            relations::{BelongsToMany, HasMany, HasManyThrough, MorphMany, MorphToMany, Pivot},
        },
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    pagination::{
        Cursor, CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, PaginationError,
        Paginator, PaginatorOptions,
    },
    routing::RoutingError,
    support::facades::{Lang, Route},
    support::{ProviderResult, ServiceProvider},
    view::{ViewData, ViewError, ViewResult},
};
use serde_json::{Value as Json, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
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

struct Comment(ModelState);

impl Model for Comment {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pagination_comments")
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
}

struct Reaction(ModelState);

impl Model for Reaction {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("pagination_reactions").timestamps(None, None);
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

struct Label(ModelState);

impl Model for Label {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("pagination_labels").timestamps(None, None);
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

struct PageMembership(ModelState);

impl Model for PageMembership {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("pagination_memberships").timestamps(None, None);
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

const REACTIONS: HasManyThrough<Article, Comment, Reaction> =
    HasManyThrough::new("article_id", "comment_id", "id", "id");

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

fn routes() -> Result<(), RoutingError> {
    Route::get("/length", length_page)?;
    Route::get("/simple", simple_page)?;
    Route::get("/cursor", cursor_page)?;
    Route::get("/links", pagination_links)?;
    Route::get("/manual", manual_length)?;
    Route::get("/manual-simple", manual_simple)?;
    Route::get("/manual-cursor", manual_cursor)?;
    Route::get("/related-length", related_length_page)?;
    Route::get("/related-cursor", related_cursor_page)?;
    Route::get("/through", through_page)?;
    Route::get("/pivot-length", pivot_length_page)?;
    Route::get("/pivot-cursor", pivot_cursor_page)?;
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

fn related_length_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()
            .map_err(internal)?
            .find(1)
            .await
            .map_err(internal)?
            .unwrap();
        let mut page = HasMany::<Article, Comment>::new("id", "article_id")
            .constrain(|mut query| {
                query.r#where("visible", "=", 1).orWhere("id", "=", 12);
                query
            })
            .paginateWith(
                &parent,
                PageOptions {
                    perPage: Some(2),
                    pageName: "comments",
                    ..Default::default()
                },
            )
            .await
            .map_err(internal)?;
        page.withQueryString();
        Response::json(StatusCode::OK, page.map(|comment| id(comment.getKey())))
    })
}

fn related_cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()
            .map_err(internal)?
            .find(1)
            .await
            .map_err(internal)?
            .unwrap();
        let mut page =
            MorphMany::<Article, Comment>::new("id", "article_id", "subject_type", "article")
                .cursorPaginateWith(
                    &parent,
                    CursorOptions {
                        perPage: Some(2),
                        cursorName: "comments",
                        ..Default::default()
                    },
                )
                .await
                .map_err(internal)?;
        page.withQueryString();
        Response::json(StatusCode::OK, page.map(|comment| id(comment.getKey())))
    })
}

fn through_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()
            .map_err(internal)?
            .find(1)
            .await
            .map_err(internal)?
            .unwrap();
        let mut page = REACTIONS
            .paginateWith(
                &parent,
                PageOptions {
                    perPage: Some(2),
                    pageName: "reactions",
                    columns: &["id"],
                    ..Default::default()
                },
            )
            .await
            .map_err(internal)?;
        page.withQueryString();
        Response::json(
            StatusCode::OK,
            page.map(|reaction| {
                assert!(reaction.getAttribute("comment_id").is_none());
                id(reaction.getKey())
            }),
        )
    })
}

fn labels() -> BelongsToMany<Article, Label> {
    BelongsToMany::new("pagination_memberships", "owner_id", "label_id", "id", "id")
        .wherePivot("subject_type", "=", "article")
        .withPivot(["label"])
        .orderByPivot("label_id", "asc")
}

fn pivot_length_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()
            .map_err(internal)?
            .find(1)
            .await
            .map_err(internal)?
            .unwrap();
        let mut page = labels()
            .paginateWith(
                &parent,
                PageOptions {
                    perPage: Some(2),
                    pageName: "labels",
                    ..Default::default()
                },
            )
            .await
            .map_err(internal)?;
        page.withQueryString();
        Response::json(
            StatusCode::OK,
            page.map(|label| {
                let pivot = label.getRelation::<Pivot>("pivot").unwrap();
                let Some(Value::Text(color)) = pivot.getAttribute("label") else {
                    panic!("missing label in pivot");
                };
                json!([id(label.getKey()), color])
            }),
        )
    })
}

fn pivot_cursor_page(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let parent = Article::query()
            .map_err(internal)?
            .find(1)
            .await
            .map_err(internal)?
            .unwrap();
        let mut page = MorphToMany::<Article, Label>::new(
            "pagination_memberships",
            "owner_id",
            "label_id",
            "id",
            "id",
            "subject_type",
            "article",
        )
        .using::<PageMembership>()
        .r#as("membership")
        .withPivot(["label"])
        .orderByPivot("weight", "asc")
        .orderByPivot("label_id", "asc")
        .cursorPaginateWith(
            &parent,
            CursorOptions {
                perPage: Some(2),
                cursorName: "labels",
                columns: &["id"],
                ..Default::default()
            },
        )
        .await
        .map_err(internal)?;
        page.withQueryString();
        Response::json(
            StatusCode::OK,
            page.map(|label| {
                assert!(
                    label
                        .state()
                        .getAttributes()
                        .keys()
                        .all(|key| !key.starts_with("laravel_pivot_"))
                );
                assert!(
                    label
                        .state()
                        .getOriginal()
                        .keys()
                        .all(|key| !key.starts_with("laravel_pivot_"))
                );
                let pivot = label.getRelation::<PageMembership>("membership").unwrap();
                assert!(pivot.getAttribute("weight").is_none());
                assert_eq!(id(pivot.getAttribute("owner_id")), 1);
                assert_eq!(id(pivot.getAttribute("label_id")), id(label.getKey()));
                let Some(Value::Text(color)) = pivot.getAttribute("label") else {
                    panic!("missing label in custom pivot");
                };
                json!([id(label.getKey()), id(pivot.getKey()), color])
            }),
        )
    })
}

fn pagination_links(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.query(Some("mode"))?.unwrap_or_default();
        let locale = request.query(Some("locale"))?.unwrap_or_default();
        let locale = locale.as_str().unwrap_or("en");
        Lang::withLocale(locale, async {
            let query = Article::query().map_err(internal)?.orderBy("id", "asc");
            let mut view = match mode.as_str() {
                Some("json") => {
                    let page = query.paginate(2).await.map_err(internal)?;
                    return Response::json(StatusCode::OK, page.map(|_| ()));
                }
                Some("window") => {
                    let mut page = query.paginate(1).await.map_err(internal)?;
                    page.onEachSide(0);
                    page.links()
                }
                Some("simple") => {
                    let mut page = query.simplePaginate(2).await.map_err(internal)?;
                    page.withQueryString();
                    page.links()
                }
                Some("cursor") => {
                    let mut page = query.cursorPaginate(2).await.map_err(internal)?;
                    page.withQueryString();
                    page.links()
                }
                Some("semantic") => query
                    .paginate(2)
                    .await
                    .map_err(internal)?
                    .linksWith("pagination::semantic-ui", ViewData::new()),
                Some("custom") => {
                    let page = query.paginate(2).await.map_err(internal)?;
                    page.linksWith(
                        "pagination::custom",
                        ViewData::from_iter([
                            ("title".into(), json!("<Custom>")),
                            ("paginator".into(), Json::Null),
                        ]),
                    )
                }
                Some("single") => query.paginate(20).await.map_err(internal)?.links(),
                _ => {
                    let mut page = query.paginate(2).await.map_err(internal)?;
                    page.withQueryString().fragment(Some("results"));
                    page.links()
                }
            }
            .map_err(view_error)?;
            let html = view.render().map_err(view_error)?;
            let mut response = Response::new(StatusCode::OK, html.into());
            response
                .parts_mut()
                .headers
                .insert("content-type", "text/html; charset=utf-8".parse().unwrap());
            Ok(response)
        })
        .await
        .map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?
    })
}

fn manual_length(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let current = request
            .query(Some("entries"))?
            .and_then(|v| v.as_str().and_then(|v| v.parse::<usize>().ok()))
            .unwrap_or(1)
            .max(1);
        let items = (1_i64..=5)
            .skip((current - 1).saturating_mul(2))
            .take(2)
            .collect();
        let mut page = LengthAwarePaginator::new(
            items,
            5,
            2,
            None,
            PaginatorOptions {
                pageName: Some("entries"),
                ..Default::default()
            },
        )
        .map_err(manual_error)?;
        assert_eq!(page.currentPage(), current as u64);
        page.withQueryString()
            .withPath("/api/manual?fixed=1")
            .fragment(Some("results"));
        assert_eq!(page.getOptions().path, Some("/api/manual?fixed=1"));
        assert_eq!(page.getOptions().fragment, Some("results"));
        assert_eq!(page.getPageName(), "entries");
        let urls: Vec<_> = page.getUrlRange(1, 2).collect();
        Response::json(
            StatusCode::OK,
            json!({
                "count": page.count(),
                "first": page.onFirstPage(),
                "last": page.onLastPage(),
                "range": urls,
                "page": page.through(|item| item.to_string()),
            }),
        )
    })
}

fn manual_simple(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut page = Paginator::new(vec![1, 2, 3, 4, 5], 2, Some(1), PaginatorOptions::default())
            .map_err(manual_error)?;
        page.setPageName("after");
        assert_eq!(page.getOptions().pageName, Some("after"));
        Response::json(StatusCode::OK, page.through(|item| item * 10))
    })
}

fn manual_cursor(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = request.query(Some("position"))?;
        let cursor = input
            .as_ref()
            .and_then(|v| v.as_str())
            .and_then(|v| Cursor::fromEncoded(v).ok());
        let anchor = cursor
            .as_ref()
            .map(|cursor| cursor.parameter("id"))
            .transpose()
            .map_err(manual_error)?;
        let ordered: Vec<i64> = match anchor {
            Some(Value::Integer(anchor))
                if cursor.as_ref().is_some_and(Cursor::pointsToPreviousItems) =>
            {
                (1..=5).rev().filter(|i| *i < anchor).take(3).collect()
            }
            Some(Value::Integer(anchor)) => (1..=5).filter(|i| *i > anchor).take(3).collect(),
            _ => vec![1, 2, 3],
        };
        let mut page = CursorPaginator::new(
            ordered,
            2,
            cursor.as_ref(),
            PaginatorOptions {
                cursorName: Some("position"),
                ..Default::default()
            },
            |item| [("id", Value::Integer(*item))],
        )
        .map_err(manual_error)?;
        page.withQueryString();
        assert_eq!(page.getCursorName(), "position");
        assert_eq!(page.getOptions().cursorName, Some("position"));
        page.setCursorName("position");
        Response::json(
            StatusCode::OK,
            json!({
                "count": page.count(),
                "first": page.onFirstPage(),
                "last": page.onLastPage(),
                "page": page.through(|item| item.to_string()),
            }),
        )
    })
}

fn manual_error(source: PaginationError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

fn view_error(source: ViewError) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
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
    serde_json::from_str(&request_body(address, path).await).unwrap()
}

async fn request_body(address: SocketAddr, path: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{path} {response}");
        response.split_once("\r\n\r\n").unwrap().1.to_owned()
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
    let base = tempfile::tempdir().unwrap();
    let lang = base.path().join("lang/zh_TW");
    std::fs::create_dir_all(&lang).unwrap();
    std::fs::write(
        lang.join("pagination.json"),
        r#"{"previous":"上一頁","next":"下一頁 <more>"}"#,
    )
    .unwrap();
    let views = base.path().join("resources/views/vendor/pagination");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(views.join("custom.blade.html"),
        r#"{{ $title }}:{{ $paginator->total }} @foreach ($elements as $element){{ $element->label }}@endforeach"#
    ).unwrap();
    let mut app = Application::configure(base.path().to_path_buf())
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
        "DROP TABLE IF EXISTS pagination_memberships",
        "DROP TABLE IF EXISTS pagination_labels",
        "CREATE TABLE pagination_labels (id BIGINT PRIMARY KEY, name VARCHAR(30))",
        "INSERT INTO pagination_labels VALUES (1, 'first'), (2, 'second'), (3, 'third'), (4, 'other'), (5, 'image')",
        "CREATE TABLE pagination_memberships (id BIGINT PRIMARY KEY, owner_id BIGINT, label_id BIGINT, subject_type VARCHAR(30), label VARCHAR(30), weight INTEGER)",
        "INSERT INTO pagination_memberships VALUES (201, 1, 1, 'article', 'red', 2), (202, 1, 2, 'article', 'blue', 1), (203, 1, 3, 'article', 'green', 3), (204, 2, 4, 'article', 'other', 0), (205, 1, 5, 'image', 'excluded', 0)",
        "DROP TABLE IF EXISTS pagination_reactions",
        "DROP TABLE IF EXISTS pagination_comments",
        "DROP TABLE IF EXISTS pagination_articles",
        "CREATE TABLE pagination_articles (id BIGINT PRIMARY KEY, rank_number INTEGER, category VARCHAR(30))",
        "INSERT INTO pagination_articles VALUES (1, 1, 'a'), (2, 1, 'a'), (3, 2, 'b'), (4, 2, 'b'), (5, 3, 'b')",
        "CREATE TABLE pagination_comments (id BIGINT PRIMARY KEY, article_id BIGINT, subject_type VARCHAR(30), visible INTEGER, deleted_at TIMESTAMP NULL)",
        "INSERT INTO pagination_comments (id, article_id, subject_type, visible) VALUES (11, 1, 'article', 1), (12, 1, 'article', 0), (13, 1, 'article', 1), (14, 2, 'article', 1), (15, 1, 'article', 0), (16, 1, 'image', 0), (17, NULL, 'article', 1)",
        "UPDATE pagination_comments SET deleted_at = '2025-01-01 00:00:00' WHERE id = 16",
        "CREATE TABLE pagination_reactions (id BIGINT PRIMARY KEY, comment_id BIGINT)",
        "INSERT INTO pagination_reactions VALUES (101, 11), (102, 11), (103, 12), (104, 14), (105, 16)",
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

    let parent = Article::queryOn(&connection).find(1).await?.unwrap();
    let comments = HasMany::<Article, Comment>::new("id", "article_id");
    let simple = comments.simplePaginate(&parent, 1).await?;
    assert_eq!(
        simple
            .items()
            .iter()
            .map(|comment| id(comment.getKey()))
            .collect::<Vec<_>>(),
        [11]
    );
    assert!(simple.nextPageUrl().is_some());
    let unsaved = Article::newInstanceOn(&connection, Attributes::new())?;
    let empty = comments.paginate(&unsaved, 2).await?;
    assert_eq!(empty.total(), 0);
    assert!(empty.items().is_empty());

    let simple_labels = labels().simplePaginate(&parent, 1).await?;
    assert_eq!(simple_labels.items().len(), 1);
    assert!(
        simple_labels.items()[0]
            .getRelation::<Pivot>("pivot")
            .is_some()
    );
    assert!(simple_labels.nextPageUrl().is_some());
    assert_eq!(labels().paginate(&unsaved, 2).await?.total(), 0);

    let through = REACTIONS
        .cursorPaginateWith(
            &parent,
            CursorOptions {
                perPage: Some(2),
                columns: &["id"],
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(
        through
            .items()
            .iter()
            .map(|reaction| id(reaction.getKey()))
            .collect::<Vec<_>>(),
        [101, 102]
    );
    let following = REACTIONS
        .cursorPaginateWith(
            &parent,
            CursorOptions {
                perPage: Some(2),
                cursor: through.nextCursor(),
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(
        following
            .items()
            .iter()
            .map(|reaction| id(reaction.getKey()))
            .collect::<Vec<_>>(),
        [103]
    );
    let all = REACTIONS
        .withTrashedParents()
        .simplePaginate(&parent, 10)
        .await?;
    assert_eq!(
        all.items()
            .iter()
            .map(|reaction| id(reaction.getKey()))
            .collect::<Vec<_>>(),
        [101, 102, 103, 105]
    );
    assert_eq!(REACTIONS.paginate(&unsaved, 2).await?.total(), 0);

    let latest = REACTIONS
        .one()
        .latestOfMany(None)
        .paginate(&parent, 2)
        .await?;
    assert_eq!(latest.total(), 1);
    assert_eq!(id(latest.items()[0].getKey()), 103);
    let (address, server) = serve(app).await;
    let manual = request(address, "/api/manual?entries=2&filter=active").await;
    assert_eq!(manual["count"], 2);
    assert_eq!(manual["first"], false);
    assert_eq!(manual["last"], false);
    assert_eq!(manual["page"]["data"], json!(["3", "4"]));
    assert_eq!(manual["page"]["total"], 5);
    assert_eq!(manual["page"]["from"], 3);
    assert_eq!(manual["page"]["to"], 4);
    assert_eq!(
        manual["range"][0][1],
        "/api/manual?fixed=1&filter=active&entries=1#results"
    );

    let manual = request(address, "/api/manual-simple").await;
    assert_eq!(manual["data"], json!([10, 20]));
    assert_eq!(manual["next_page_url"], "/api/manual-simple?after=2");
    let first = request(address, "/api/manual-cursor?state=open").await;
    assert_eq!(first["first"], true);
    assert_eq!(first["page"]["data"], json!(["1", "2"]));
    let second = request(address, first["page"]["next_page_url"].as_str().unwrap()).await;
    assert_eq!(second["count"], 2);
    assert_eq!(second["first"], false);
    assert_eq!(second["page"]["data"], json!(["3", "4"]));
    let last = request(address, second["page"]["next_page_url"].as_str().unwrap()).await;
    assert_eq!(last["last"], true);
    assert_eq!(last["page"]["data"], json!(["5"]));
    let first_again = request(address, second["page"]["prev_page_url"].as_str().unwrap()).await;
    assert_eq!(first_again["first"], true);
    assert_eq!(first_again["page"]["data"], first["page"]["data"]);
    assert!(first_again["page"]["prev_page_url"].is_null());

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
    let related = request(address, "/api/related-length?comments=2&visible=yes").await;
    assert_eq!(related["total"], 3);
    assert_eq!(related["data"], json!([13]));
    let previous = related["prev_page_url"].as_str().unwrap();
    assert!(previous.contains("visible=yes"));
    assert_eq!(request(address, previous).await["data"], json!([11, 12]));
    let first = request(address, "/api/related-cursor?filter=all").await;
    assert_eq!(first["data"], json!([11, 12]));
    let next = first["next_page_url"].as_str().unwrap();
    assert!(next.contains("comments=") && next.contains("filter=all"));
    let second = request(address, next).await;
    assert_eq!(second["data"], json!([13, 15]));
    assert!(second["next_page_url"].is_null());
    let previous = second["prev_page_url"].as_str().unwrap();
    assert_eq!(request(address, previous).await["data"], json!([11, 12]));
    let through = request(address, "/api/through?reactions=2&filter=all").await;
    assert_eq!(through["total"], 3);
    assert_eq!(through["data"], json!([103]));
    let previous = through["prev_page_url"].as_str().unwrap();
    assert!(previous.contains("filter=all"));
    assert_eq!(request(address, previous).await["data"], json!([101, 102]));
    let labels = request(address, "/api/pivot-length?labels=2&filter=all").await;
    assert_eq!(labels["total"], 3);
    assert_eq!(labels["data"], json!([[3, "green"]]));
    let previous = labels["prev_page_url"].as_str().unwrap();
    assert!(previous.contains("filter=all"));
    assert_eq!(
        request(address, previous).await["data"],
        json!([[1, "red"], [2, "blue"]])
    );

    let first = request(address, "/api/pivot-cursor?filter=all").await;
    assert_eq!(first["data"], json!([[2, 202, "blue"], [1, 201, "red"]]));
    let next = first["next_page_url"].as_str().unwrap();
    assert!(next.contains("labels=") && next.contains("filter=all"));
    let second = request(address, next).await;
    assert_eq!(second["data"], json!([[3, 203, "green"]]));
    assert!(second["next_page_url"].is_null());
    let previous = second["prev_page_url"].as_str().unwrap();
    assert_eq!(request(address, previous).await["data"], first["data"]);
    let (chinese, english) = tokio::join!(
        request_body(address, "/api/links?locale=zh-TW&filter=all"),
        request_body(address, "/api/links?filter=all"),
    );
    assert!(chinese.contains("上一頁"));
    assert!(chinese.contains("下一頁 &lt;more&gt;"));
    assert!(english.contains("« Previous"));
    assert!(english.contains("Next »"));
    assert!(english.contains("aria-current=\"page\""));
    assert!(english.contains("aria-disabled=\"true\""));
    assert!(english.contains("filter=all&amp;page=2#results"));
    let last = request_body(address, "/api/links?page=3").await;
    assert!(last.contains("rel=\"prev\""));
    assert!(!last.contains("rel=\"next\""));
    let simple = request_body(address, "/api/links?mode=simple&page=2").await;
    assert!(simple.contains("rel=\"prev\"") && simple.contains("rel=\"next\""));
    assert!(!simple.contains("aria-current"));
    let cursor = request_body(address, "/api/links?mode=cursor").await;
    assert!(cursor.contains("rel=\"next\"") && cursor.contains("cursor="));
    assert!(!cursor.contains("aria-current"));
    let (_, next) = cursor.split_once(r#"rel="next" href=""#).unwrap();
    let (next, _) = next.split_once('"').unwrap();
    let following = request_body(address, &next.replace("&amp;", "&")).await;
    assert!(following.contains(r#"rel="prev""#));

    let json_links = request(address, "/api/links?mode=json&locale=zh-TW").await;
    assert_eq!(json_links["links"][0]["label"], "上一頁");
    assert_eq!(json_links["links"][4]["label"], "下一頁 <more>");
    assert!(
        request_body(address, "/api/links?mode=single")
            .await
            .trim()
            .is_empty()
    );
    let custom = request_body(address, "/api/links?mode=custom").await;
    assert!(custom.contains("&lt;Custom&gt;:5 123"));
    std::fs::write(
        views.join("simple-tailwind.blade.html"),
        "Override {{ $paginator->next }}",
    )
    .unwrap();
    let overridden = request_body(address, "/api/links?mode=simple").await;
    assert!(overridden.contains("Override Next »"));

    for id in 6..=17 {
        connection
            .statement(
                "INSERT INTO pagination_articles VALUES (?, 0, 'window')",
                &[Value::Integer(id)],
            )
            .await?;
    }
    let window = request_body(address, "/api/links?mode=window&page=9").await;
    assert_eq!(window.matches(">...</span>").count(), 2);
    assert!(window.contains(r#"aria-current="page""#) && window.contains(">9</span>"));
    assert!(!window.contains(">8</a>") && !window.contains(">10</a>"));
    server.abort();
    connection
        .statement("DROP TABLE pagination_memberships", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_labels", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_reactions", &[])
        .await?;
    connection
        .statement("DROP TABLE pagination_comments", &[])
        .await?;
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

async fn serve(app: Application) -> (SocketAddr, JoinHandle<()>) {
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
    (address, server)
}

struct StyleProvider(fn() -> ViewResult<()>);

impl ServiceProvider for StyleProvider {
    fn boot(&mut self, _: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async { Ok(self.0()?) })
    }
}

async fn styled_server(
    style: fn() -> ViewResult<()>,
) -> (tempfile::TempDir, SocketAddr, JoinHandle<()>) {
    let base = tempfile::tempdir().unwrap();
    let views = base.path().join("resources/views");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(
        views.join("numbered.blade.html"),
        "numbered {{ $paginator->total }}",
    )
    .unwrap();
    std::fs::write(
        views.join("simple.blade.html"),
        "simple {{ $paginator->next }}",
    )
    .unwrap();
    let mut app = Application::configure(base.path().to_path_buf())
        .withProviders(vec![Box::new(StyleProvider(style))])
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let repository = app.container().get::<dyn Repository>("config").unwrap();
    repository
        .set("database.default", json!("pagination"))
        .unwrap();
    repository
        .set(
            "database.connections.pagination",
            json!({
                "driver": "sqlite", "database": ":memory:"
            }),
        )
        .unwrap();
    let connection = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .statement(
            "CREATE TABLE pagination_articles (id INTEGER PRIMARY KEY)",
            &[],
        )
        .await
        .unwrap();
    connection
        .statement("INSERT INTO pagination_articles VALUES (1), (2), (3)", &[])
        .await
        .unwrap();
    let (address, server) = serve(app).await;
    (base, address, server)
}

#[tokio::test]
async fn pagination_default_styles_are_scoped_to_the_application() {
    let (_root_v3, v3, server_v3) = styled_server(Paginator::useBootstrapThree).await;
    let (_root_v4, v4, server_v4) = styled_server(Paginator::useBootstrap).await;
    let (_root_v5, v5, server_v5) = styled_server(Paginator::useBootstrapFive).await;
    let (_root_tailwind, tailwind, server_tailwind) = styled_server(|| {
        Paginator::useBootstrapFour()?;
        Paginator::useTailwind()
    })
    .await;
    let (_root_custom, custom, server_custom) = styled_server(|| {
        Paginator::defaultView("numbered")?;
        Paginator::defaultSimpleView("simple")
    })
    .await;

    let (three, four, five, tail, own) = tokio::join!(
        request_body(v3, "/api/links"),
        request_body(v4, "/api/links"),
        request_body(v5, "/api/links"),
        request_body(tailwind, "/api/links"),
        request_body(custom, "/api/links"),
    );
    assert!(three.contains(r#"class="pagination""#));
    assert!(three.contains(r#"class="active" aria-current="page""#));
    assert!(!three.contains("page-link"));
    assert!(four.contains(r#"class="page-item active" aria-current="page""#));
    assert!(!four.contains("d-sm-none"));
    assert!(five.contains("d-sm-none") && five.contains("fw-semibold"));
    assert!(five.contains(r#"class="page-item active" aria-current="page""#));
    assert!(tail.contains("rounded-md shadow-sm"));
    assert_eq!(own, "numbered 3");

    for address in [v3, v4, v5] {
        let (simple, cursor) = tokio::join!(
            request_body(address, "/api/links?mode=simple"),
            request_body(address, "/api/links?mode=cursor"),
        );
        assert!(simple.contains(r#"class="pagination""#) && simple.contains("Next »"));
        assert!(cursor.contains(r#"class="pagination""#) && cursor.contains("cursor="));
        assert!(!simple.contains("aria-current") && !cursor.contains("aria-current"));
    }
    assert_eq!(
        request_body(custom, "/api/links?mode=simple").await,
        "simple Next »"
    );
    assert_eq!(
        request_body(custom, "/api/links?mode=cursor").await,
        "simple Next »"
    );

    let semantic = request_body(v5, "/api/links?mode=semantic").await;
    assert!(semantic.contains("ui pagination menu") && semantic.contains("right chevron icon"));
    assert!(semantic.contains(r#"aria-current="page""#));

    for server in [
        server_v3,
        server_v4,
        server_v5,
        server_tailwind,
        server_custom,
    ] {
        server.abort();
    }
}

#[test]
fn rejects_invalid_manual_page_sizes_and_cursor_parameters() {
    assert!(matches!(
        Paginator::new(Vec::<i64>::new(), 0, None, PaginatorOptions::default()),
        Err(PaginationError::InvalidPerPage)
    ));
    assert!(matches!(
        LengthAwarePaginator::new(vec![1], 1, 2, Some(u64::MAX), PaginatorOptions::default()),
        Err(PaginationError::OffsetOverflow)
    ));
    assert!(matches!(
        Cursor::new([("id", Value::Null)], true),
        Err(PaginationError::InvalidCursor)
    ));
    assert!(matches!(
        Cursor::new([("id", Value::Integer(1)), ("id", Value::Integer(2))], true),
        Err(PaginationError::InvalidCursor)
    ));
    let cursor = Cursor::new(
        [
            ("id", Value::Integer(1)),
            ("slug", Value::Text("entry".into())),
        ],
        false,
    )
    .unwrap();
    let restored = Cursor::fromEncoded(&cursor.encode().unwrap()).unwrap();
    assert!(restored.pointsToPreviousItems());
    assert_eq!(
        restored.parameters(&["slug", "id"]).unwrap(),
        [Value::Text("entry".into()), Value::Integer(1)]
    );
}
