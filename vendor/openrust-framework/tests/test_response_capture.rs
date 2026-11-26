use openrust::{
    foundation::configuration::Routing,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Route,
    testing::{ResponseCaptureOptions, TestApp},
};
use std::{
    convert::Infallible,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

fn sized(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let size: usize = request.route("size").unwrap().parse().unwrap();
        Ok(Response::new(StatusCode::OK, Body::from(vec![b'x'; size])))
    })
}
static DROPPED: AtomicBool = AtomicBool::new(false);
struct DropProbe;
impl Drop for DropProbe {
    fn drop(&mut self) {
        DROPPED.store(true, Ordering::SeqCst);
    }
}
fn pending(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let guard = DropProbe;
        let body = Body::from_stream(futures_util::stream::once(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
            Ok::<_, Infallible>(axum::body::Bytes::new())
        }));
        Ok(Response::new(StatusCode::OK, body))
    })
}
fn delayed(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Ok(Response::new(
            StatusCode::OK,
            Body::from_stream(futures_util::stream::once(async {
                tokio::time::sleep(Duration::from_millis(5200)).await;
                Ok::<_, Infallible>(axum::body::Bytes::from_static(b"done"))
            })),
        ))
    })
}
fn app() -> TestApp {
    TestApp::create(|builder| {
        builder.with_routing(Routing::default().web(|| {
            Route::get("/size/{size}", sized)?;
            Route::get("/pending", pending)?;
            Route::get("/delayed", delayed)
        }))
    })
    .unwrap()
}

#[tokio::test]
async fn capture_limit_is_configurable_at_runtime_and_isolated_per_app() {
    let mut first = app();
    let mut second = app();
    first.get("/size/1048576").await.unwrap().assert_ok();
    assert!(first.get("/size/1048577").await.is_err());
    first.with_response_capture(ResponseCaptureOptions {
        max_bytes: 2 * 1024 * 1024,
        ..Default::default()
    });
    let response = first.get("/size/1048577").await.unwrap();
    assert_eq!(response.body().len(), 1048577);
    assert!(second.get("/size/1048577").await.is_err());
    first.with_response_capture(ResponseCaptureOptions {
        max_bytes: 0,
        ..Default::default()
    });
    first.get("/size/0").await.unwrap().assert_content("");
    assert!(first.get("/size/1").await.is_err());
}

#[tokio::test]
async fn capture_timeout_cancels_infinite_stream_and_does_not_poison_later_requests() {
    DROPPED.store(false, Ordering::SeqCst);
    let mut app = app();
    app.with_response_capture(ResponseCaptureOptions {
        timeout: Duration::from_millis(20),
        ..Default::default()
    });
    assert!(app.get("/pending").await.is_err());
    assert!(DROPPED.load(Ordering::SeqCst));
    app.get("/size/1").await.unwrap().assert_content("x");
}

#[tokio::test]
async fn long_finite_stream_can_exceed_default_timeout_when_explicitly_configured() {
    let mut app = app();
    assert!(app.get("/delayed").await.is_err());
    app.with_response_capture(ResponseCaptureOptions {
        timeout: Duration::from_secs(7),
        ..Default::default()
    });
    app.get("/delayed").await.unwrap().assert_content("done");
}
