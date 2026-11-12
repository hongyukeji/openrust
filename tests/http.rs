use std::sync::{Arc, Mutex};

use openrust::{
    contracts::http::{Kernel as _, Middleware},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    routing::{MiddlewareEntry, Router},
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

#[tokio::test]
async fn exceptions_negotiate_json_without_changing_status_or_allow() {
    let app = Application::configure(std::env::current_dir().unwrap())
        .withRouting(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/api/page", explicit)?;
            Ok(router)
        }))
        .withExceptions(|exceptions| {
            exceptions
                .shouldRenderJsonWhen(|request, _| request.is("api/*") || request.expectsJson());
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();

    for (path, accept, json) in [
        ("/api/missing", "text/html", true),
        ("/ap%69/missing", "text/html", true),
        ("/api", "text/html", false),
        ("/missing", "text/html", false),
        ("/missing", "application/problem+json", true),
        ("/missing", "application/json;q=0.4, text/html;q=0.9", false),
        (
            "/missing",
            "text/html;profile=\"one,two;q=0\";q=0.4, application/json;q=0.9",
            true,
        ),
    ] {
        let (mut request, _) = request(Method::GET, path);
        request
            .parts_mut()
            .headers
            .insert("accept", accept.parse().unwrap());
        let mut response = kernel.handle(&mut request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.take_body(), 1024)
            .await
            .unwrap();
        if json {
            assert_eq!(response.headers()["content-type"], "application/json");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                serde_json::json!({"message":"Not Found"})
            );
        } else {
            assert_eq!(
                response.headers()["content-type"],
                "text/html; charset=utf-8"
            );
            assert!(String::from_utf8_lossy(&body).contains("404 Not Found"));
        }
    }

    let (mut wrong_method, _) = request(Method::POST, "/api/page");
    let response = kernel.handle(&mut wrong_method).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["allow"], "GET, HEAD");
}

#[tokio::test]
async fn exception_callbacks_match_sources_and_finalize_custom_and_default_responses() {
    use openrust::foundation::configuration::ReportDecision;

    fn fail(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            Err(HttpError::Internal {
                source: Box::new(std::io::Error::other("private exception detail")),
            })
        })
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = events.clone();
    let app = Application::configure(std::env::current_dir().unwrap())
        .withRouting(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/api/custom", fail)?;
            router.get("/api/fallback", fail)?;
            Ok(router)
        }))
        .withExceptions(move |exceptions| {
            let report_events = observed.clone();
            let render_events = observed.clone();
            exceptions
                .report::<std::io::Error>(move |_| {
                    report_events.lock().unwrap().push("report");
                    ReportDecision::Stop
                })
                .report::<HttpError>(|_| panic!("reporting should stop"))
                .render::<std::io::Error>(|_, _| None)
                .render::<std::io::Error>(move |_, request| {
                    render_events.lock().unwrap().push("render");
                    request.is("api/custom").then(|| {
                        Response::new(StatusCode::IM_A_TEAPOT, Body::from("custom response"))
                    })
                })
                .respond(move |mut response, _, _| {
                    observed.lock().unwrap().push("respond");
                    response
                        .parts_mut()
                        .headers
                        .insert("x-exception", "finalized".parse().unwrap());
                    response
                })
                .shouldRenderJsonWhen(|request, _| request.is("api/*") || request.expectsJson());
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();

    for (method, path, status, body) in [
        (
            Method::GET,
            "/api/custom",
            StatusCode::IM_A_TEAPOT,
            "custom response",
        ),
        (Method::HEAD, "/api/custom", StatusCode::IM_A_TEAPOT, ""),
        (
            Method::GET,
            "/api/fallback",
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"message":"Server Error"}"#,
        ),
    ] {
        let (mut request, _) = request(method, path);
        let (parts, response_body) = kernel.handle(&mut request).await.into_parts();
        assert_eq!(parts.status, status);
        assert_eq!(parts.headers["x-exception"], "finalized");
        assert_eq!(
            axum::body::to_bytes(response_body, 1024).await.unwrap(),
            body
        );
    }
    let (mut request, _) = request(Method::POST, "/api/custom");
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()["allow"], "GET, HEAD");
    assert_eq!(response.headers()["x-exception"], "finalized");
    assert_eq!(
        *events.lock().unwrap(),
        [
            "report", "render", "respond", "report", "render", "respond", "report", "render",
            "respond", "respond"
        ]
    );
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
