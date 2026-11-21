use openrust::{
    container::ParameterValue,
    contracts::{
        container::Container as _,
        events::{Dispatcher as _, Event, Listener},
        foundation::Application as _,
        http::Kernel as _,
    },
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{
        Application,
        configuration::Routing,
        http::{Kernel, events::RequestHandled},
    },
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{Router, RoutingError},
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HANDLED: &str = r"OpenRust\Foundation\Http\Events\RequestHandled";
fn ok(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request
            .attributes()
            .insert("trace", ParameterValue::new("correlation-1".to_owned()));
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok(Response::from("hello"))
    })
}
fn error(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(openrust::http::abort(
            StatusCode::UNPROCESSABLE_ENTITY,
            "bad input",
            Default::default(),
        ))
    })
}
fn stream(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Ok(Response::new(
            StatusCode::OK,
            Body::from_stream(futures_util::stream::pending::<
                Result<&'static str, std::io::Error>,
            >()),
        ))
    })
}
fn slow(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok(Response::from("late"))
    })
}
fn routes(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.get("/ok", ok)?.name("observed");
    router.get("/error", error)?;
    router.get("/stream", stream)?;
    router.get("/slow", slow)?;
    Ok(router)
}
async fn kernel() -> (Kernel, Arc<Dispatcher>) {
    let app = Application::configure(std::env::temp_dir())
        .withRouting(Routing::new(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let events = kernel
        .get_application()
        .container()
        .make::<Dispatcher>()
        .unwrap();
    (kernel, events)
}
fn request(method: &str, path: &str) -> Request {
    let (parts, _) = http::Request::builder()
        .method(method)
        .uri(path)
        .body(())
        .unwrap()
        .into_parts();
    Request::from_parts(parts, Body::empty())
}
#[tokio::test]
async fn handled_observes_rendered_errors_context_and_headers_without_consuming_bodies() {
    let (kernel, dispatcher) = kernel().await;
    let fake = dispatcher.fake(&[HANDLED]);
    let mut incoming = request("GET", "/ok");
    let response = kernel.handle(&mut incoming).await;
    assert_eq!(response.status(), StatusCode::OK);
    let captured = fake.dispatched(HANDLED);
    assert_eq!(captured.len(), 1);
    let event = captured[0].downcast_ref::<RequestHandled>().unwrap();
    assert_eq!(event.routeName.as_deref(), Some("observed"));
    assert_eq!(
        event.attributes["trace"]
            .downcast::<String>()
            .unwrap()
            .as_str(),
        "correlation-1"
    );
    assert!(event.duration >= Duration::from_millis(5));
    incoming.attributes().remove("trace");
    assert!(event.attributes.contains_key("trace"));
    let (_, body) = response.into_parts();
    assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), "hello");
    for (method, path, status) in [
        ("GET", "/error", 422),
        ("GET", "/missing", 404),
        ("HEAD", "/ok", 200),
        ("GET", "/stream", 200),
        ("GET", "/%FF", 400),
    ] {
        let mut incoming = request(method, path);
        let response = tokio::time::timeout(Duration::from_secs(1), kernel.handle(&mut incoming))
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        let captured = fake.dispatched(HANDLED);
        let event = captured
            .last()
            .unwrap()
            .downcast_ref::<RequestHandled>()
            .unwrap();
        assert_eq!(event.response.status.as_u16(), status);
        assert_eq!(event.request.method.as_str(), method);
        if path != "/ok" {
            assert!(event.routeName.is_none());
        }
        if method == "HEAD" {
            assert_eq!(event.response.headers["content-length"], "5");
            assert!(
                axum::body::to_bytes(response.into_parts().1, 1024)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }
    assert_eq!(fake.dispatched(HANDLED).len(), 6);
    let mut incoming = request("GET", "/slow");
    assert!(
        tokio::time::timeout(Duration::from_millis(10), kernel.handle(&mut incoming))
            .await
            .is_err()
    );
    assert_eq!(fake.dispatched(HANDLED).len(), 6);
}

struct Observe(Arc<Mutex<Vec<String>>>);
impl Listener for Observe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let handled = event.downcast_ref::<RequestHandled>().unwrap();
            // Dispatch is inside this application's facade lifecycle.
            openrust::http::client::Http::globalMiddleware(|request, next| {
                Box::pin(async move { next.run(request).await })
            })
            .unwrap();
            openrust::http::client::Http::preventStrayRequests(true).unwrap();
            self.0
                .lock()
                .unwrap()
                .push(handled.request.uri.path().to_owned());
            Ok(None)
        })
    }
}
#[tokio::test]
async fn handled_listeners_run_in_the_owning_application_and_remain_isolated() {
    let (first, a) = kernel().await;
    let (second, b) = kernel().await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    a.listen(HANDLED, Arc::new(Observe(observed.clone())), Delivery::Sync)
        .unwrap();
    let fake = b.fake(&[HANDLED]);
    let mut one = request("GET", "/ok");
    let mut two = request("GET", "/ok");
    let (r1, r2) = tokio::join!(first.handle(&mut one), second.handle(&mut two));
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
    assert_eq!(observed.lock().unwrap().as_slice(), ["/ok"]);
    assert_eq!(fake.dispatched(HANDLED).len(), 1);
    assert!(
        first
            .get_application()
            .container()
            .make::<openrust::http::client::Factory>()
            .unwrap()
            .preventingStrayRequests()
    );
    assert!(
        !second
            .get_application()
            .container()
            .make::<openrust::http::client::Factory>()
            .unwrap()
            .preventingStrayRequests()
    );
}

struct Reject;
impl Listener for Reject {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async { Err(openrust::events::EventError::InvalidPayload) })
    }
}
#[tokio::test]
async fn handled_listener_errors_are_rendered_once_without_recursive_dispatch() {
    let (kernel, dispatcher) = kernel().await;
    dispatcher
        .listen(HANDLED, Arc::new(Reject), Delivery::Sync)
        .unwrap();
    let mut incoming = request("GET", "/ok");
    assert_eq!(
        kernel.handle(&mut incoming).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn handled_is_emitted_for_real_http_success_and_rendered_failure() {
    use axum::{extract::State, handler::Handler as _};
    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handleTransport(request).await
    }
    let (kernel, dispatcher) = kernel().await;
    let fake = dispatcher.fake(&[HANDLED]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let first = client.get(format!("{base}/ok")).send().await.unwrap();
    assert_eq!(first.text().await.unwrap(), "hello");
    let second = client.get(format!("{base}/error")).send().await.unwrap();
    assert_eq!(second.status().as_u16(), 422);
    assert_eq!(fake.dispatched(HANDLED).len(), 2);
    server.abort();
}
