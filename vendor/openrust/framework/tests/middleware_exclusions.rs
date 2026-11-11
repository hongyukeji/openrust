use std::sync::Arc;

use openrust::{
    contracts::http::{Kernel as _, Middleware},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Next, Request, Response, StatusCode},
    routing::{MiddlewareEntry, RouteGroup, RoutingError},
    support::facades::Route,
};

struct Stamp;

impl Middleware for Stamp {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let mut response = next.run(request).await;
            response.parts_mut().headers.append(
                ::http::HeaderName::from_static("x-stamp"),
                ::http::HeaderValue::from_static("ran"),
            );
            Ok(response)
        })
    }
}

fn route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn register() -> Result<(), RoutingError> {
    Route::group(RouteGroup::new().middleware_name("outer"), |router| {
        router.get("/included", route)?;
        router.get("/excluded", route)?.withoutMiddleware("stamp");
        router.group(RouteGroup::new().withoutMiddleware("outer"), |router| {
            router.get("/group-excluded", route)?;
            Ok(())
        })
    })
}

#[tokio::test]
async fn exclusions_resolve_nested_aliases_without_removing_global_middleware() {
    let app = Application::configure(std::env::temp_dir())
        .withMiddleware(|middleware| {
            let stamp = Arc::new(Stamp);
            middleware
                .append(stamp.clone())
                .alias("stamp", stamp)
                .group("inner", [MiddlewareEntry::named("stamp")])
                .group("outer", [MiddlewareEntry::named("inner")]);
        })
        .withRouting(Routing::default().web(register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    for (path, count) in [("/included", 2), ("/excluded", 1), ("/group-excluded", 1)] {
        let (parts, body) = ::http::Request::builder()
            .uri(path)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        assert_eq!(response.headers().get_all("x-stamp").iter().count(), count);
    }
}
