//! Resource routing and response contracts through axum's one-shot service.

use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::Path;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use openrust_core::config::FrameworkConfig;
use openrust_core::{AppBuilder, finalize};
use openrust_http::{
    Action, AppBuilderHttpExt, Created, NoContent, PageParams, Paginated, Resource,
    ResourceActions, Routes, build_router,
};
use serde_json::{Value, json};
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn index() -> &'static str {
    "index"
}

async fn show(Path(id): Path<String>) -> String {
    format!("show:{id}")
}

async fn store() -> &'static str {
    "store"
}

async fn update(Path(id): Path<String>) -> String {
    format!("update:{id}")
}

async fn destroy(Path(id): Path<String>) -> String {
    format!("destroy:{id}")
}

struct Posts;

impl Resource for Posts {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions
            .destroy(destroy)
            .show(show)
            .store(store)
            .update(update)
            .index(index)
    }
}

struct IndexOnly;

impl Resource for IndexOnly {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions.index(index)
    }
}

struct Empty;

impl Resource for Empty {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions
    }
}

struct FilteredBeforeRegistration;

impl Resource for FilteredBeforeRegistration {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions
            .only(&[Action::Index, Action::Show])
            .except(&[Action::Show])
            .show(show)
            .index(index)
    }
}

async fn router(
    f: impl Fn(Routes) -> Routes + Send + Sync + 'static,
) -> openrust_core::Result<axum::Router> {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder.routes(f);
    let app = finalize(builder).await?;
    Ok(build_router(&app))
}

async fn send(router: axum::Router, method: Method, path: &str) -> TestResultResponse {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())?;
    Ok(router.oneshot(request).await?)
}

type TestResultResponse = Result<Response, Box<dyn std::error::Error>>;

async fn body(response: Response) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(to_bytes(response.into_body(), usize::MAX).await?.to_vec())
}

async fn json_body(response: Response) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&body(response).await?)?)
}

#[tokio::test]
async fn all_resource_actions_map_to_the_expected_methods_and_paths() -> TestResult {
    let router = router(|routes| routes.resource::<Posts>("/posts")).await?;
    for (method, path, expected) in [
        (Method::GET, "/posts", "index"),
        (Method::POST, "/posts", "store"),
        (Method::GET, "/posts/42", "show:42"),
        (Method::PUT, "/posts/42", "update:42"),
        (Method::PATCH, "/posts/42", "update:42"),
        (Method::DELETE, "/posts/42", "destroy:42"),
    ] {
        let response = send(router.clone(), method, path).await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(body(response).await?, expected.as_bytes());
    }
    Ok(())
}

#[tokio::test]
async fn trailing_slash_resource_path_uses_one_collection_and_member_base() -> TestResult {
    let router = router(|routes| routes.resource::<Posts>("/posts/")).await?;
    for (path, expected) in [("/posts", "index"), ("/posts/42", "show:42")] {
        let response = send(router.clone(), Method::GET, path).await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(body(response).await?, expected.as_bytes());
    }
    Ok(())
}

#[tokio::test]
async fn empty_and_slash_resource_paths_work_inside_groups() -> TestResult {
    for path in ["", "/"] {
        let router =
            router(move |routes| routes.group("/posts", |group| group.resource::<Posts>(path)))
                .await?;
        for (request_path, expected) in [("/posts", "index"), ("/posts/42", "show:42")] {
            let response = send(router.clone(), Method::GET, request_path).await?;
            assert_eq!(response.status(), StatusCode::OK, "{path}: {request_path}");
            assert_eq!(body(response).await?, expected.as_bytes());
        }
    }
    Ok(())
}

#[tokio::test]
async fn absent_actions_do_not_create_paths_and_registered_paths_reject_other_methods() -> TestResult
{
    let router = router(|routes| {
        routes
            .resource::<IndexOnly>("/posts")
            .resource::<Empty>("/empty")
    })
    .await?;
    for (method, path, status) in [
        (Method::GET, "/posts/42", StatusCode::NOT_FOUND),
        (Method::PUT, "/posts/42", StatusCode::NOT_FOUND),
        (Method::POST, "/posts", StatusCode::METHOD_NOT_ALLOWED),
        (Method::GET, "/empty", StatusCode::NOT_FOUND),
        (Method::GET, "/empty/42", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(send(router.clone(), method, path).await?.status(), status);
    }
    Ok(())
}

#[tokio::test]
async fn only_and_except_filter_registered_actions_in_nested_groups() -> TestResult {
    let router = router(|routes| {
        routes.group("/api", |api| {
            api.resource_with::<Posts>("/only", |a| a.only(&[Action::Show, Action::Update]))
                .resource_with::<Posts>("/except", |a| a.except(&[Action::Store, Action::Destroy]))
                .resource_with::<Posts>("/none", |a| a.only(&[]))
        })
    })
    .await?;
    for (method, path, status) in [
        (Method::GET, "/api/only", StatusCode::NOT_FOUND),
        (Method::POST, "/api/only", StatusCode::NOT_FOUND),
        (Method::GET, "/api/only/1", StatusCode::OK),
        (Method::PATCH, "/api/only/1", StatusCode::OK),
        (
            Method::DELETE,
            "/api/only/1",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (Method::GET, "/api/except", StatusCode::OK),
        (Method::POST, "/api/except", StatusCode::METHOD_NOT_ALLOWED),
        (Method::GET, "/api/except/1", StatusCode::OK),
        (Method::PUT, "/api/except/1", StatusCode::OK),
        (
            Method::DELETE,
            "/api/except/1",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (Method::GET, "/api/none", StatusCode::NOT_FOUND),
        (Method::GET, "/api/none/1", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            send(router.clone(), method, path).await?.status(),
            status,
            "{path}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn filters_apply_even_when_declared_before_handlers() -> TestResult {
    let router = router(|routes| routes.resource::<FilteredBeforeRegistration>("/posts")).await?;
    assert_eq!(
        send(router.clone(), Method::GET, "/posts").await?.status(),
        StatusCode::OK
    );
    assert_eq!(
        send(router, Method::GET, "/posts/1").await?.status(),
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test]
async fn repeated_only_calls_intersect_registered_actions() -> TestResult {
    let router = router(|routes| {
        routes.resource_with::<Posts>("/posts", |actions| {
            actions
                .only(&[Action::Index, Action::Show])
                .only(&[Action::Show, Action::Store])
        })
    })
    .await?;
    assert_eq!(
        send(router.clone(), Method::GET, "/posts/1")
            .await?
            .status(),
        StatusCode::OK
    );
    for (method, path) in [(Method::GET, "/posts"), (Method::POST, "/posts")] {
        assert_eq!(
            send(router.clone(), method, path).await?.status(),
            StatusCode::NOT_FOUND
        );
    }
    Ok(())
}

async fn created() -> Created<Json<Value>> {
    Created(Json(json!({"id": 42})))
}

async fn no_content() -> NoContent {
    NoContent
}

async fn paginated(params: PageParams) -> Paginated<&'static str> {
    Paginated::new(vec!["a", "b"], params, 57)
}

async fn pagination_params(params: PageParams) -> Json<Value> {
    Json(
        json!({"page": params.page, "per_page": params.per_page, "offset": params.offset(), "limit": params.limit()}),
    )
}

#[tokio::test]
async fn created_and_no_content_have_their_status_and_body_contract() -> TestResult {
    let router = router(|routes| {
        routes
            .post("/created", created)
            .delete("/no-content", no_content)
    })
    .await?;
    let response = send(router.clone(), Method::POST, "/created").await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(json_body(response).await?, json!({"id": 42}));

    let response = send(router, Method::DELETE, "/no-content").await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(body(response).await?, Vec::<u8>::new());
    Ok(())
}

#[tokio::test]
async fn pagination_defaults_and_json_shape() -> TestResult {
    let router = router(|routes| {
        routes
            .get("/page", paginated)
            .get("/params", pagination_params)
    })
    .await?;
    let response = send(router.clone(), Method::GET, "/page").await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        json_body(response).await?,
        json!({"data": ["a", "b"], "meta": {"page": 1, "per_page": 20, "total": 57, "total_pages": 3}})
    );
    assert_eq!(
        json_body(send(router, Method::GET, "/params?page=3&per_page=10").await?).await?,
        json!({"page": 3, "per_page": 10, "offset": 20, "limit": 10})
    );
    Ok(())
}

#[tokio::test]
async fn pagination_per_page_only_keeps_default_page() -> TestResult {
    let router = router(|routes| routes.get("/params", pagination_params)).await?;
    assert_eq!(
        json_body(send(router, Method::GET, "/params?per_page=50").await?).await?,
        json!({"page": 1, "per_page": 50, "offset": 0, "limit": 50})
    );
    Ok(())
}

#[tokio::test]
async fn pagination_accepts_largest_database_offset() -> TestResult {
    let router = router(|routes| routes.get("/params", pagination_params)).await?;
    assert_eq!(
        json_body(
            send(
                router,
                Method::GET,
                "/params?page=9223372036854775808&per_page=1"
            )
            .await?
        )
        .await?,
        json!({"page": 9_223_372_036_854_775_808_u64, "per_page": 1, "offset": i64::MAX, "limit": 1})
    );
    Ok(())
}

#[tokio::test]
async fn pagination_rejects_database_offset_overflow_as_page_error() -> TestResult {
    let router = router(|routes| routes.get("/page", paginated)).await?;
    for query in [
        "page=9223372036854775809&per_page=1",
        "page=18446744073709551615&per_page=100",
    ] {
        let response = send(router.clone(), Method::GET, &format!("/page?{query}")).await?;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
        let problem = json_body(response).await?;
        assert_eq!(problem["errors"]["page"], json!(["is too large"]));
    }
    Ok(())
}

#[tokio::test]
async fn pagination_rejects_out_of_range_and_invalid_fields_as_problems() -> TestResult {
    let router = router(|routes| routes.get("/page", paginated)).await?;
    for (query, field) in [
        ("page=0", "page"),
        ("page=abc", "page"),
        ("page=-1", "page"),
        ("page=18446744073709551616", "page"),
        ("per_page=0", "per_page"),
        ("per_page=101", "per_page"),
        ("per_page=abc", "per_page"),
    ] {
        let response = send(router.clone(), Method::GET, &format!("/page?{query}")).await?;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{query}"
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
        let problem = json_body(response).await?;
        assert_eq!(problem["code"], "validation_failed");
        assert!(problem["errors"][field].is_array(), "{query}: {problem}");
    }
    let response = send(router, Method::GET, "/page?page=0&per_page=101").await?;
    let problem = json_body(response).await?;
    assert!(problem["errors"]["page"].is_array());
    assert!(problem["errors"]["per_page"].is_array());
    Ok(())
}

#[tokio::test]
async fn total_pages_cover_zero_exact_and_partial_pages() -> TestResult {
    for (total, expected) in [(0, 0), (20, 1), (40, 2), (41, 3)] {
        let response = Paginated::<u8>::new(
            vec![],
            PageParams {
                page: 1,
                per_page: 20,
            },
            total,
        );
        let json = json_body(response.into_response()).await?;
        assert_eq!(json["meta"]["total_pages"], expected);
        assert_eq!(json["meta"]["total"], total);
    }
    Ok(())
}
