use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        events::{Event, Listener},
        http::Kernel as _,
    },
    events::{Delivery, EventContext, EventError, EventFuture, ListenerResponse},
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        events::DiagnosingHealth,
        http::Kernel,
    },
    support::{ProviderResult, ServiceProvider, facades::Event as Events},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Probe(bool);

impl Listener for Probe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, Result<Option<ListenerResponse>, EventError>> {
        Box::pin(async move {
            assert!(event.downcast_ref::<DiagnosingHealth>().is_some());
            if self.0 {
                return Err(EventError::Listener {
                    source: Box::new(io::Error::other("private diagnostic failure")),
                });
            }
            Ok(None)
        })
    }
}

impl ServiceProvider for Probe {
    fn boot<'a>(
        &'a mut self,
        _: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            Events::listen(
                DiagnosingHealth.name(),
                Arc::new(Self(self.0)),
                Delivery::Sync,
            )?;
            Ok(())
        })
    }
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn send(address: SocketAddr, method: &str, accept: &str) -> (String, String) {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!(
                    "{method} /up HTTP/1.1\r\nHost: {address}\r\nAccept: {accept}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        (headers.to_owned(), body.to_owned())
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn health_endpoint_negotiates_html_and_json_and_reports_failed_diagnostics() {
    for (failing, debug) in [(false, false), (true, false), (true, true)] {
        let root = tempfile::tempdir().unwrap();
        let reports = Arc::new(AtomicUsize::new(0));
        let application = Application::configure(root.path().to_owned())
            .withConfig(if debug {
                |_, _| Ok([("app".into(), json!({"debug": true}))].into())
            } else {
                |_, _| Ok([("app".into(), json!({"name": "<Health & \"Check\">"}))].into())
            })
            .withProviders(vec![Box::new(Probe(failing))])
            .withRouting(Routing::default().health("/up"))
            .withExceptions({
                let reports = reports.clone();
                move |exceptions| {
                    exceptions.report::<EventError>(move |_| {
                        reports.fetch_add(1, Ordering::Relaxed);
                        ReportDecision::Stop
                    });
                }
            })
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });

        let status = if failing { 500 } else { 200 };
        let (headers, body) = send(address, "GET", "application/json").await;
        assert!(
            headers.starts_with(&format!("HTTP/1.1 {status} ")),
            "{headers}"
        );
        assert!(
            headers.contains("content-type: application/json"),
            "{headers}"
        );
        let json: Value = serde_json::from_str(&body).unwrap();
        if debug {
            assert!(json.get("status").is_none(), "{json}");
            assert!(body.contains("private diagnostic failure"), "{body}");
        } else {
            assert_eq!(json, json!({"status": if failing { "down" } else { "up" }}));
            assert!(
                headers.contains("cache-control: no-cache, private"),
                "{headers}"
            );
            let (headers, body) = send(address, "GET", "text/html").await;
            assert!(
                headers.starts_with(&format!("HTTP/1.1 {status} ")),
                "{headers}"
            );
            assert!(
                headers.contains("content-type: text/html; charset=utf-8"),
                "{headers}"
            );
            assert!(
                body.contains("<title>&lt;Health &amp; &quot;Check&quot;&gt;</title>"),
                "{body}"
            );
            let heading = if failing {
                "experiencing problems"
            } else {
                "up"
            };
            assert!(
                body.contains(&format!("<h2>Application {heading}</h2>")),
                "{body}"
            );
            assert!(
                body.contains("HTTP request received. Response rendered in "),
                "{body}"
            );
            assert!(body.contains("ms.</p>"), "{body}");
            assert_eq!(body.contains("class=\"status status-down\""), failing);
            assert!(!body.contains("private diagnostic failure"), "{body}");

            let (headers, body) = send(address, "HEAD", "text/html").await;
            assert!(
                headers.starts_with(&format!("HTTP/1.1 {status} ")),
                "{headers}"
            );
            assert!(
                headers.contains("content-type: text/html; charset=utf-8"),
                "{headers}"
            );
            assert!(body.is_empty(), "{body}");
        }
        assert_eq!(
            reports.load(Ordering::Relaxed),
            if debug {
                1
            } else if failing {
                3
            } else {
                0
            }
        );
        server.abort();
        let _ = server.await;
    }
}
