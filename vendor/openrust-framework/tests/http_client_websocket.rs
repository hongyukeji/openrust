use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, StatusCode, header};
use openrust::http::client::{Error, Factory, Response, WebSocketConfig, WebSocketMessage};
use tokio::{io::AsyncWriteExt, net::TcpListener, time::timeout};

#[tokio::test]
async fn websocket_upgrade_uses_http_client_headers_and_exchanges_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut peer = tokio_tungstenite::accept_hdr_async(
            socket,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(request.uri().to_string(), "/api/live?call_id=rtc_fixture");
                assert_eq!(request.headers()[header::AUTHORIZATION], "Bearer example");
                assert_eq!(request.headers()["x-fixture"], "middleware");
                assert!(
                    !request
                        .headers()
                        .contains_key(header::SEC_WEBSOCKET_PROTOCOL)
                );
                assert!(
                    !request
                        .headers()
                        .contains_key(header::SEC_WEBSOCKET_EXTENSIONS)
                );
                Ok(response)
            },
        )
        .await
        .unwrap();
        for expected in [
            WebSocketMessage::Text("hello".into()),
            WebSocketMessage::Binary(vec![1, 2, 3].into()),
        ] {
            let frame = peer.next().await.unwrap().unwrap();
            assert_eq!(frame, expected);
            peer.send(frame).await.unwrap();
        }
        assert!(matches!(
            peer.next().await.unwrap().unwrap(),
            WebSocketMessage::Close(_)
        ));
        peer.flush().await.unwrap();
    });
    let factory = Factory::new().unwrap();
    let mut socket = factory
        .pending_request()
        .base_url(format!("ws://{address}/api"))
        .with_token("example")
        .with_query_parameters([("call_id".into(), "rtc_fixture".into())])
        .with_header(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static("unsupported"),
        )
        .before_sending(|request| {
            request
                .headers_mut()
                .insert("x-fixture", HeaderValue::from_static("middleware"));
            Ok(())
        })
        .timeout(Duration::from_secs(2))
        .connect_websocket("live", WebSocketConfig::default())
        .await
        .unwrap();

    for frame in [
        WebSocketMessage::Text("hello".into()),
        WebSocketMessage::Binary(vec![1, 2, 3].into()),
    ] {
        socket.send(frame.clone()).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(2), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            frame
        );
    }
    socket.close(None).await.unwrap();
    timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn websocket_upgrade_rejects_unverified_handshakes_and_does_not_follow_redirects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for response in [
            &b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: wrong\r\n\r\n"[..],
            &b"HTTP/1.1 302 Found\r\nLocation: /do-not-follow\r\nContent-Length: 0\r\n\r\n"[..],
        ] {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            while socket.read_line(&mut line).await.unwrap() != 0 && line != "\r\n" {
                line.clear();
            }
            socket.get_mut().write_all(response).await.unwrap();
        }
        assert!(
            timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let factory = Factory::new().unwrap();
    let url = format!("http://{address}/api");
    assert!(matches!(
        factory
            .pending_request()
            .connect_websocket(&url, WebSocketConfig::default())
            .await,
        Err(Error::InvalidWebSocketUpgrade)
    ));
    assert!(matches!(
        factory.pending_request().connect_websocket(&url, WebSocketConfig::default()).await,
        Err(Error::RequestFailed(response)) if response.status() == StatusCode::FOUND
    ));
    timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();

    let fake = Factory::new().unwrap();
    fake.prevent_stray_requests(true);
    assert!(matches!(
        fake.pending_request()
            .connect_websocket(&url, WebSocketConfig::default())
            .await,
        Err(Error::StrayRequest)
    ));
    fake.stub_url(
        url.as_str(),
        Response::new(
            StatusCode::SWITCHING_PROTOCOLS,
            http::HeaderMap::new(),
            Vec::new(),
        ),
    );
    assert!(matches!(
        fake.pending_request()
            .connect_websocket(&url, WebSocketConfig::default())
            .await,
        Err(Error::InvalidWebSocketUpgrade)
    ));
}
