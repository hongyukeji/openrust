use std::{path::PathBuf, process::Stdio};

use tokio::{
    fs,
    io::{AsyncBufReadExt as _, BufReader},
    process::Command,
    time::sleep,
};

use super::{
    Application, HttpError, HttpFuture, Middleware, Next, Request, RequestParts, Response,
    ResponseParts, Routing, StatusCode, TIMEOUT, TerminableMiddleware, routes, send,
};
use serde_json::json;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt as _, net::TcpStream, time::timeout};

const ROOT: &str = "OPENRUST_TEST_HTTP_SHUTDOWN_ROOT";
const POLL: Duration = Duration::from_millis(20);

struct WaitForRelease(PathBuf);

impl Middleware for WaitForRelease {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move { Ok(next.run(request).await) })
    }
}

impl TerminableMiddleware for WaitForRelease {
    fn terminate<'a>(
        &'a self,
        request: &'a RequestParts,
        response: &'a ResponseParts,
    ) -> HttpFuture<'a, ()> {
        Box::pin(async move {
            assert_eq!(request.uri.path(), "/api/data");
            assert_eq!(response.status, StatusCode::OK);
            let mut log = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.0.join("callbacks"))
                .await
                .unwrap();
            log.write_all(b"entered\n").await.unwrap();
            log.flush().await.unwrap();
            while !self.0.join("release").exists() {
                sleep(POLL).await;
            }
            log.write_all(b"finished\n").await.unwrap();
            log.flush().await.unwrap();
        })
    }
}

#[tokio::test]
async fn graceful_shutdown_waits_for_termination_after_closing_the_listener() {
    if let Some(root) = std::env::var_os(ROOT).map(PathBuf::from) {
        Application::configure(root.clone())
            .with_config(
                |_, _| Ok([("app".into(), json!({"host": "127.0.0.1", "port": 0}))].into()),
            )
            .with_routing(Routing::default().api(routes))
            .with_middleware(move |middleware| {
                middleware.appendTerminable(Arc::new(WaitForRelease(root)));
            })
            .create()
            .unwrap()
            .start()
            .await
            .unwrap();
        return;
    }

    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "shutdown::graceful_shutdown_waits_for_termination_after_closing_the_listener",
            "--nocapture",
        ])
        .env(ROOT, root.path())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let address: SocketAddr = timeout(TIMEOUT, async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(address) = line.strip_prefix("OpenRust listening on http://") {
                return address.parse().unwrap();
            }
        }
        panic!("child exited before announcing the listener");
    })
    .await
    .unwrap();
    let response = send(
        address,
        "GET /api/data HTTP/1.1\r\nHost: app.test\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200 "));
    assert!(response.ends_with("data"));

    let callbacks = root.path().join("callbacks");
    timeout(TIMEOUT, async {
        while fs::read(&callbacks).await.as_deref().ok() != Some(b"entered\n") {
            sleep(POLL).await;
        }
    })
    .await
    .expect("terminable callback did not start");
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().unwrap().to_string()])
            .status()
            .await
            .unwrap()
            .success()
    );
    timeout(TIMEOUT, async {
        while TcpStream::connect(address).await.is_ok() {
            sleep(POLL).await;
        }
    })
    .await
    .expect("HTTP listener remained open after SIGINT");
    assert!(
        timeout(Duration::from_millis(100), child.wait())
            .await
            .is_err(),
        "process exited before the callback was released"
    );
    fs::write(root.path().join("release"), []).await.unwrap();
    assert!(
        timeout(TIMEOUT, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read(callbacks).await.unwrap(), b"entered\nfinished\n");
}
