use openrust::{
    contracts::http::{Kernel as _, Middleware},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{MiddlewareEntry, RouteGroup, RoutingError},
    support::facades::Route,
};
use std::sync::Arc;

struct Stamp;
impl Middleware for Stamp {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            assert!(request.has_session(), "追加中间件不得取代原生web会话装配");
            let mut response = next.run(request).await;
            response.parts_mut().headers.append(
                http::HeaderName::from_static("x-group-stamp"),
                http::HeaderValue::from_static("ran"),
            );
            Ok(response)
        })
    }
}
fn endpoint(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}
fn routes() -> Result<(), RoutingError> {
    let stamp = Arc::new(Stamp);
    Route::push_middleware_to_group("web", MiddlewareEntry::Instance(stamp.clone()))?;
    Route::push_middleware_to_group("web", MiddlewareEntry::Instance(stamp))?;
    Route::get("/included", endpoint)?;
    Route::group(RouteGroup::new().without_middleware("web"), |router| {
        router.get("/excluded", endpoint)?;
        Ok(())
    })
}
#[tokio::test]
async fn public_group_append_keeps_native_sessions_deduplicates_and_honors_group_exclusion() {
    let app = Application::configure(std::env::temp_dir())
        .with_config(|_, _| {
            Ok([
                ("app".into(), serde_json::json!({"key":"k".repeat(32)})),
                (
                    "session".into(),
                    serde_json::json!({"driver":"array","lottery":[0,100]}),
                ),
            ]
            .into())
        })
        .with_routing(Routing::default().web(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    for (path, count) in [("/included", 1), ("/excluded", 0)] {
        let mut request: Request = http::Request::builder()
            .uri(format!("http://localhost{path}"))
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap()
            .into();
        let response = kernel.handle(&mut request).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            response.headers().get_all("x-group-stamp").iter().count(),
            count
        );
    }
}
