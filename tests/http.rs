use std::sync::{Arc, Mutex};

use openrust::{
    contracts::http::{Kernel as _, Middleware},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    routing::MiddlewareEntry,
};

type Trace = Arc<Mutex<Vec<(&'static str, bool)>>>;

struct Record {
    name: &'static str,
    stop: bool,
}

impl Middleware for Record {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let trace = request.parts().extensions.get::<Trace>().unwrap().clone();
            trace.lock().unwrap().push((self.name, true));
            if self.stop {
                return Ok(Response::empty(StatusCode::FORBIDDEN));
            }
            let response = next.run(request).await;
            trace.lock().unwrap().push((self.name, false));
            Ok(response)
        })
    }
}

fn record(name: &'static str) -> Arc<Record> {
    Arc::new(Record { name, stop: false })
}

fn explicit(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn request(method: Method, uri: &str) -> (Request, Trace) {
    let trace = Trace::default();
    let (parts, body) = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .extension(trace.clone())
        .body(Body::empty())
        .unwrap()
        .into_parts();
    (Request::from_parts(parts, body), trace)
}

#[tokio::test]
async fn declared_route_sources_get_their_framework_groups() {
    let application = Application::configure(std::env::current_dir().unwrap())
        .withMiddleware(|middleware| {
            middleware.group("web", [MiddlewareEntry::Instance(record("web"))]);
            middleware.group("api", [MiddlewareEntry::Instance(record("api"))]);
        })
        .withRouting(
            Routing::default()
                .web(|| openrust::support::facades::Route::get("/page", explicit))
                .api(|router| {
                    router.get("/page", explicit)?;
                    Ok(())
                })
                .health("/up"),
        )
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    for (path, group) in [("/page", "web"), ("/api/page", "api")] {
        let (mut request, trace) = request(Method::GET, path);
        assert_eq!(
            kernel.handle(&mut request).await.status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(*trace.lock().unwrap(), [(group, true), (group, false)]);
    }
    let (mut request, trace) = request(Method::GET, "/up");
    assert_eq!(kernel.handle(&mut request).await.status(), StatusCode::OK);
    assert!(trace.lock().unwrap().is_empty());
}

#[tokio::test]
async fn api_prefix_can_be_overridden_without_changing_the_middleware_group() {
    for (prefix, path) in [("/v1/api/", "/v1/api/page"), ("", "/page")] {
        let application = Application::configure(std::env::current_dir().unwrap())
            .withMiddleware(|middleware| {
                middleware.group("api", [MiddlewareEntry::Instance(record("api"))]);
            })
            .withRouting(
                Routing::default()
                    .api(|router| {
                        router.get("/page", explicit)?;
                        Ok(())
                    })
                    .apiPrefix(prefix),
            )
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();

        let (mut api_request, trace) = request(Method::GET, path);
        assert_eq!(
            kernel.handle(&mut api_request).await.status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(*trace.lock().unwrap(), [("api", true), ("api", false)]);

        let (mut default_request, _) = request(Method::GET, "/api/page");
        assert_eq!(
            kernel.handle(&mut default_request).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}
