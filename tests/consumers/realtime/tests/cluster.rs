use std::{error::Error as StdError, net::TcpListener, process::Stdio, time::Duration};

use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::{
    process::{Child, Command},
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header},
    },
};

type TestResult<T = ()> = Result<T, Box<dyn StdError>>;

#[tokio::test]
async fn two_real_api_processes_route_websocket_and_sse_through_redis() -> TestResult {
    let redis_url = std::env::var("OPENRUST_REALTIME_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let port_a = free_port()?;
    let port_b = free_port()?;
    let mut node_a = start_node(port_a, &redis_url)?;
    let mut node_b = start_node(port_b, &redis_url)?;
    let client = Client::builder().no_proxy().build()?;
    wait_for_node(&client, port_a).await?;
    wait_for_node(&client, port_b).await?;

    let bad_origin = websocket_request(port_a, "user-73", "http://evil.example")?;
    assert!(connect_async(bad_origin).await.is_err());

    let mut websocket = connect_async(websocket_request(port_a, "user-73", "http://localhost")?)
        .await?
        .0;
    let sse_response = client
        .get(format!("http://127.0.0.1:{port_a}/realtime/sse"))
        .bearer_auth("user-73")
        .header(header::ORIGIN, "http://localhost")
        .send()
        .await?;
    assert!(sse_response.status().is_success());
    let mut sse = sse_response.bytes_stream();

    sleep(Duration::from_millis(300)).await;
    let response = client
        .post(format!("http://127.0.0.1:{port_b}/realtime/send/73"))
        .bearer_auth("user-900")
        .json(&json!({"message": "cross-node"}))
        .send()
        .await?;
    assert!(
        response.status().is_success(),
        "node B rejected publish: {}",
        response.status()
    );

    let websocket_payload = next_websocket_event(&mut websocket).await?;
    assert!(websocket_payload.contains("cross-node"));
    let sse_payload = next_sse_event(&mut sse).await?;
    assert!(sse_payload.contains("cross-node"));

    let revoke_response = client
        .post(format!("http://127.0.0.1:{port_b}/realtime/revoke/73"))
        .bearer_auth("user-900")
        .send()
        .await?;
    assert!(revoke_response.status().is_success());
    timeout(Duration::from_secs(5), wait_websocket_close(&mut websocket)).await??;

    let mut shutdown_socket =
        connect_async(websocket_request(port_a, "user-73", "http://localhost")?)
            .await?
            .0;
    let shutdown_sse_response = client
        .get(format!("http://127.0.0.1:{port_a}/realtime/sse"))
        .bearer_auth("user-73")
        .header(header::ORIGIN, "http://localhost")
        .send()
        .await?;
    let mut shutdown_sse = shutdown_sse_response.bytes_stream();
    sleep(Duration::from_millis(100)).await;
    signal_sigterm(&node_a).await?;
    timeout(
        Duration::from_secs(5),
        wait_websocket_close(&mut shutdown_socket),
    )
    .await??;
    timeout(Duration::from_secs(5), wait_sse_close(&mut shutdown_sse)).await??;
    timeout(Duration::from_secs(5), node_a.wait()).await??;
    stop_node(&mut node_b).await?;

    // Redis 不可达时发布必须明确失败；不能把跨节点消息伪装成本地投递成功。
    let failed_port = free_port()?;
    let mut failed_node = start_node(failed_port, "redis://127.0.0.1:1/")?;
    wait_for_node(&client, failed_port).await?;
    let failed_publish = client
        .post(format!("http://127.0.0.1:{failed_port}/realtime/send/73"))
        .bearer_auth("user-900")
        .json(&json!({"message": "must-fail"}))
        .send()
        .await?;
    let failure: serde_json::Value = failed_publish.json().await?;
    assert_eq!(failure["code"], "realtime_transport_unavailable");
    stop_node(&mut failed_node).await?;
    Ok(())
}

async fn next_websocket_event(
    socket: &mut (
             impl futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
             + Unpin
         ),
) -> TestResult<String> {
    timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("WebSocket closed before cross-node message arrived")??;
            if let Message::Text(payload) = message
                && payload.contains("cross-node")
            {
                return Ok::<_, Box<dyn StdError>>(payload.to_string());
            }
        }
    })
    .await?
}

async fn next_sse_event(
    stream: &mut (impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin),
) -> TestResult<String> {
    timeout(Duration::from_secs(5), async {
        loop {
            let chunk = stream.next().await.ok_or("SSE stream ended")??;
            let text = String::from_utf8_lossy(&chunk).into_owned();
            if text.contains("cross-node") {
                return Ok::<_, Box<dyn StdError>>(text);
            }
        }
    })
    .await?
}

async fn wait_websocket_close(
    socket: &mut (
             impl futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
             + Unpin
         ),
) -> TestResult {
    loop {
        match socket.next().await {
            Some(Ok(Message::Close(_))) => return Ok(()),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(Ok(_)) => return Err("WebSocket received a data frame before closing".into()),
            Some(Err(error)) => return Err(error.into()),
            None => return Err("WebSocket ended without a Close frame".into()),
        }
    }
}

async fn wait_sse_close(
    stream: &mut (impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin),
) -> TestResult {
    loop {
        match stream.next().await {
            None | Some(Err(_)) => return Ok(()),
            Some(Ok(_)) => {}
        }
    }
}

fn websocket_request(
    port: u16,
    token: &str,
    origin: &str,
) -> TestResult<tokio_tungstenite::tungstenite::http::Request<()>> {
    let mut request = format!("ws://127.0.0.1:{port}/realtime/ws").into_client_request()?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    request
        .headers_mut()
        .insert(header::ORIGIN, HeaderValue::from_str(origin)?);
    Ok(request)
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn start_node(port: u16, redis_url: &str) -> std::io::Result<Child> {
    Command::new(env!("CARGO_BIN_EXE_realtime-node"))
        .env("APP_SERVER_HOST", "127.0.0.1")
        .env("APP_SERVER_PORT", port.to_string())
        .env("APP_APP_NAME", "realtime-e2e")
        .env("OPENRUST_REALTIME_REDIS_URL", redis_url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
}

async fn wait_for_node(client: &Client, port: u16) -> TestResult {
    timeout(Duration::from_secs(20), async {
        loop {
            if client
                .get(format!("http://127.0.0.1:{port}/health"))
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await?;
    Ok(())
}

async fn stop_node(child: &mut Child) -> std::io::Result<()> {
    if child.try_wait()?.is_none() {
        child.kill().await?;
    }
    Ok(())
}

#[cfg(unix)]
async fn signal_sigterm(child: &Child) -> std::io::Result<()> {
    let pid = child
        .id()
        .ok_or(std::io::Error::other("node process has no PID"))?;
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "could not send SIGTERM to realtime node",
        ))
    }
}

#[cfg(not(unix))]
async fn signal_sigterm(_child: &Child) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "real-time shutdown acceptance currently requires a Unix host",
    ))
}
