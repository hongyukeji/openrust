use super::*;
use crate::config::FrameworkConfig;
use crate::foundation::{ProviderBuilder, RegisterContext};
use crate::http::Inject;
use crate::support::{BoxFuture, ServiceProvider};
use figment::{Figment, providers::Serialized};
use http_body_util::{BodyExt, StreamBody};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tower::ServiceExt;

struct Routes;

fn unfinished_stream() -> Body {
    use futures_util::StreamExt;
    Body::from_stream(
        futures_util::stream::iter([Ok::<_, Infallible>(Bytes::from_static(b"partial"))])
            .chain(futures_util::stream::pending()),
    )
}

impl ServiceProvider for Routes {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.scoped::<AtomicBool, _>(|_| {
                Box::pin(async { Ok(Arc::new(AtomicBool::new(false))) })
            })
            .await
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/complete", |Inject(flag): Inject<AtomicBool>| async move {
                        flag.store(true, Ordering::Relaxed);
                        "payload"
                    })
                    .get("/empty", || async { StatusCode::NO_CONTENT })
                    .get("/trailers", || async {
                        let mut trailers = http::HeaderMap::new();
                        trailers.insert("x-probe", http::HeaderValue::from_static("ok"));
                        let frames = futures_util::stream::iter([
                            Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"payload"))),
                            Ok(Frame::trailers(trailers)),
                        ]);
                        ([("trailer", "x-probe")], Body::new(StreamBody::new(frames)))
                    })
                    .get("/stream", || async { unfinished_stream() })
                    .get("/stream-length", || async {
                        ([("content-length", "7")], unfinished_stream())
                    })
                    .get("/stream-error", || async {
                        Body::from_stream(futures_util::stream::iter([
                            Ok(Bytes::from_static(b"partial")),
                            Err(std::io::Error::other("正文失败")),
                        ]))
                    })
            })?;
            Ok(())
        })
    }
}

async fn app() -> Result<(Application, mpsc::UnboundedReceiver<bool>)> {
    let mut config = FrameworkConfig::default();
    config.api.http_status = crate::config::HttpStatusMode::Semantic;
    let mut builder = ProviderBuilder::new();
    builder.provider(HttpServiceProvider).provider(Routes);
    let (sender, receiver) = mpsc::unbounded_channel();
    builder.terminating(move |state| {
        let sender = sender.clone();
        Box::pin(async move {
            assert!(http_entry_scope().is_some());
            let flag = state.resolve::<AtomicBool>().await?.load(Ordering::Relaxed);
            sender.send(flag).map_err(Error::internal)?;
            Ok(())
        })
    });
    let app = Application::from_provider_runtime(
        builder
            .start(Figment::from(Serialized::defaults(config)))
            .await?,
    )?;
    Ok((app, receiver))
}

async fn read_response(address: std::net::SocketAddr, method: &str, path: &str) -> Result<String> {
    let mut stream = TcpStream::connect(address).await.map_err(Error::internal)?;
    stream.write_all(
        format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nTE: trailers\r\n\r\n").as_bytes(),
    ).await.map_err(Error::internal)?;
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_string(&mut response))
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    Ok(response)
}

#[tokio::test]
async fn terminating_http_handles_fixed_head_empty_trailers_and_not_found() -> Result<()> {
    let (app, mut events) = app().await?;
    let router = build_router(&app)?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(Error::internal)?;
    let address = listener.local_addr().map_err(Error::internal)?;
    let (shutdown, stop) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stop.await;
            })
            .await
    });
    for (method, path, status, expected) in [
        ("GET", "/complete", "200", true),
        ("HEAD", "/complete", "200", true),
        ("GET", "/empty", "204", false),
        ("GET", "/trailers", "200", false),
        ("GET", "/missing", "404", false),
    ] {
        let response = read_response(address, method, path).await?;
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{response}"
        );
        if method == "HEAD" || status == "204" {
            assert!(response.ends_with("\r\n\r\n"), "{response}");
        } else if path == "/complete" {
            assert!(response.ends_with("payload"), "{response}");
        } else if path == "/trailers" {
            assert!(response.contains("x-probe: ok\r\n"), "{response}");
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .map_err(Error::internal)?,
            Some(expected),
        );
    }
    shutdown
        .send(())
        .map_err(|()| Error::custom("test_listener_stopped", "测试监听已关闭"))?;
    server
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    assert!(events.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn terminating_http_releases_failed_or_abandoned_body_without_running_callbacks() -> Result<()>
{
    let (app, mut events) = app().await?;
    let router = build_router(&app)?;
    for (path, version) in [
        ("/stream", Version::HTTP_11),
        ("/stream-error", Version::HTTP_11),
        ("/stream-length", Version::HTTP_2),
    ] {
        let request = Request::builder()
            .uri(path)
            .version(version)
            .body(Body::empty())
            .map_err(Error::internal)?;
        let response = router
            .clone()
            .oneshot(request)
            .await
            .map_err(Error::internal)?;
        let mut body = response.into_body();
        let frame = body.frame().await.transpose().map_err(Error::internal)?;
        assert!(
            matches!(frame, Some(frame) if frame.data_ref() == Some(&Bytes::from_static(b"partial")))
        );
        if path == "/stream-error" {
            assert!(matches!(body.frame().await, Some(Err(_))));
        }
        drop(body);
    }
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await?;
    assert!(events.try_recv().is_err());
    Ok(())
}
