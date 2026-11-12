use std::{net::SocketAddr, sync::Arc, time::Duration};

use openrust::{
    foundation::{Application, configuration::Routing},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::Notify,
    time::{sleep, timeout},
};

struct BootstrapGate {
    release: Arc<Notify>,
    fail: bool,
}

impl ServiceProvider for BootstrapGate {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.release.notified().await;
            if self.fail {
                return Err(std::io::Error::other("fixture bootstrap failure").into());
            }
            Ok(())
        })
    }
}

async fn request(address: SocketAddr, method: &str, path: &str, accept: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = loop {
            match TcpStream::connect(address).await {
                Ok(stream) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    sleep(Duration::from_millis(15)).await;
                }
                Err(error) => panic!("{error}"),
            }
        };
        let wire = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: {accept}\r\nConnection: close\r\n\r\n");
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    })
    .await
    .unwrap()
}

async fn verify_startup(fail: bool) {
    let directory = tempfile::tempdir().unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    std::fs::write(directory.path().join("port"), address.port().to_string()).unwrap();
    let marker = directory.path().join("storage/framework/down");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(
        &marker,
        json!({
            "template": "<h1>Early maintenance snapshot</h1>",
            "except": ["up"],
            "retry": 60,
            "redirect": "/offline",
            "status": 503
        })
        .to_string(),
    )
    .unwrap();

    let release = Arc::new(Notify::new());
    let app = Application::configure(directory.path().to_path_buf())
        .withConfig(|base, _| {
            let port: u16 = std::fs::read_to_string(base.join("port"))
                .unwrap()
                .parse()
                .unwrap();
            Ok([("app".to_owned(), json!({"host":"127.0.0.1","port":port}))].into())
        })
        .withProviders(vec![Box::new(BootstrapGate {
            release: release.clone(),
            fail,
        })])
        .withMiddleware(|middleware| {
            middleware.preventRequestsDuringMaintenance(["up"]);
        })
        .withRouting(Routing::default().health("/up"))
        .create()
        .unwrap();

    let service = tokio::task::spawn_local(app.start());
    let html = request(address, "GET", "/offline", "text/html").await;
    assert!(html.starts_with("HTTP/1.1 503 "), "{html}");
    assert!(html.contains("retry-after: 60"));
    assert!(html.contains("Early maintenance snapshot"));
    let head = request(address, "HEAD", "/offline", "text/html").await;
    assert!(head.split_once("\r\n\r\n").unwrap().1.is_empty());
    let redirect = request(address, "GET", "/", "text/html").await;
    assert!(redirect.starts_with("HTTP/1.1 302 "), "{redirect}");
    assert!(redirect.contains("location: /offline"));
    let waiting =
        tokio::spawn(async move { request(address, "GET", "/up", "application/json").await });
    sleep(Duration::from_millis(50)).await;
    assert!(
        !waiting.is_finished(),
        "an excluded request must wait for the normal kernel"
    );

    release.notify_one();
    let passed = waiting.await.unwrap();
    if fail {
        assert!(passed.starts_with("HTTP/1.1 500 "), "{passed}");
        let html = request(address, "GET", "/offline", "text/html").await;
        assert!(html.starts_with("HTTP/1.1 503 "), "{html}");
        std::fs::remove_file(marker).unwrap();
        assert!(
            timeout(Duration::from_secs(3), service)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    } else {
        assert!(passed.starts_with("HTTP/1.1 200 "), "{passed}");
        std::fs::remove_file(marker).unwrap();
        let restored = request(address, "GET", "/offline", "text/html").await;
        assert!(restored.starts_with("HTTP/1.1 404 "), "{restored}");
        service.abort();
        let _ = service.await;
    }
}

#[tokio::test]
async fn prerender_serves_while_bootstrap_waits_and_hands_off_to_the_kernel() {
    tokio::task::LocalSet::new()
        .run_until(verify_startup(false))
        .await;
}

#[tokio::test]
async fn prerender_stays_available_after_a_bootstrap_failure_until_up() {
    tokio::task::LocalSet::new()
        .run_until(verify_startup(true))
        .await;
}
