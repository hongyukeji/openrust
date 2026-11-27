use std::{error::Error, fs, time::Duration};

use futures_util::{SinkExt, StreamExt};
use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode, client::WebSocketMessage},
    support::facades::{Config, Route},
    testing::TestApp,
};
use openrust_ai::{Ai, AiError, LiveProtocol};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const OFFER: &str = "v=0\r\ns=offer\r\n";
const ANSWER: &str = "v=0\r\ns=answer\r\n";
const ID: &str = "live_fixture";

#[tokio::test]
async fn native_live_connections_use_their_own_create_and_sideband_wire_contracts()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let address = upstream.local_addr()?;
    let fixture = tokio::spawn(async move {
        let session = json!({"model":"voice-upstream","instructions":"Fixture"});
        for mode in [LiveProtocol::Codex, LiveProtocol::OpenAi] {
            let (mut socket, line, headers, body) = request(&upstream).await;
            assert_eq!(headers["authorization"], "Bearer fixture-key");
            let (response, extra_headers) = match mode {
                LiveProtocol::Codex => {
                    assert_eq!(
                        line,
                        "POST /edge/codex/realtime/calls?intent=quicksilver&architecture=avas HTTP/1.1"
                    );
                    assert_eq!(headers["x-oai-attestation"], "fixture-attestation");
                    assert_eq!(headers["openai-alpha"], "quicksilver=v2");
                    assert_eq!(headers["session-id"].len(), 36);
                    assert_eq!(headers["thread-id"].len(), 36);
                    assert_eq!(headers["accept"], "application/sdp");
                    assert_eq!(body, json!({"sdp":OFFER,"session":session}));
                    (
                        ANSWER.to_owned(),
                        "Content-Type: application/sdp\r\nLocation: https://untrusted.invalid/live_fixture\r\n",
                    )
                }
                LiveProtocol::OpenAi => {
                    assert_eq!(line, "POST /api/v1/live/sessions HTTP/1.1");
                    assert!(!headers.contains_key("x-oai-attestation"));
                    assert_eq!(
                        body,
                        json!({"transport":{"type":"webrtc","sdp":OFFER},"session":session})
                    );
                    (
                        json!({"session":{"id":ID},"transport":{"type":"webrtc","sdp":ANSWER}})
                            .to_string(),
                        "Content-Type: application/json\r\n",
                    )
                }
            };
            let response = format!(
                "HTTP/1.1 201 Created\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            drop(socket);
            confirm_sideband(&upstream, mode).await;
        }
        let (mut socket, line, headers, _) = request(&upstream).await;
        assert_eq!(
            line,
            "POST /api/v1/live/sessions/live_fixture/hangup HTTP/1.1"
        );
        assert_eq!(headers["authorization"], "Bearer fixture-key");
        socket
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });

    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_config(|_, env| {
                Ok([(
                    "live_fixture_address".into(),
                    json!(env.get_utf8("LIVE_FIXTURE_ADDRESS")?),
                )]
                .into())
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/live-test", || async {
                    let address = Config::get("live_fixture_address").unwrap().unwrap();
                    let address = address.as_str().unwrap();
                    let codex = json!({
                        "name":"fixture-codex", "driver":"openai",
                        "url":format!("http://{address}/edge/codex"),"key_env":"LIVE_FIXTURE_KEY",
                        "live":"codex", "headers":{"x-oai-attestation":"fixture-attestation"},
                    });
                    let public = json!({
                        "name":"fixture-public", "driver":"openai",
                        "url":format!("http://{address}/api/v1"),"key_env":"LIVE_FIXTURE_KEY",
                        "live":"openai",
                    });
                    let mut disabled = public.clone();
                    disabled["name"] = "fixture-disabled".into();
                    disabled.as_object_mut().unwrap().remove("live");
                    assert!(Ai::build(disabled).unwrap().live_gateway().is_none());
                    let mut missing = codex.clone();
                    missing["name"] = "fixture-missing".into();
                    missing.as_object_mut().unwrap().remove("headers");
                    let unavailable = Ai::build(missing).unwrap();
                    let session = json!({"model":"voice-upstream","instructions":"Fixture"});
                    assert!(matches!(
                        unavailable
                            .live_gateway()
                            .unwrap()
                            .create_live_call(OFFER, &session)
                            .await,
                        Err(AiError::InvalidConfiguration(_))
                    ));
                    let a = Ai::build(codex).unwrap();
                    let b = Ai::build(public).unwrap();
                    let a = a.live_gateway().unwrap();
                    let b = b.live_gateway().unwrap();
                    assert_eq!(a.protocol(), LiveProtocol::Codex);
                    assert_eq!(b.protocol(), LiveProtocol::OpenAi);
                    assert_ne!(a.live_scope().unwrap(), b.live_scope().unwrap());
                    for provider in [a, b] {
                        assert!(matches!(
                            provider.connect_live_sideband("../escape").await,
                            Err(AiError::InvalidPrompt)
                        ));
                        let call = provider.create_live_call(OFFER, &session).await.unwrap();
                        assert_eq!(call.id, ID);
                        assert_eq!(call.sdp, ANSWER);
                        let mut ws = provider.connect_live_sideband(&call.id).await.unwrap();
                        ws.send(WebSocketMessage::Text("control".into()))
                            .await
                            .unwrap();
                        let event = ws.next().await.unwrap().unwrap();
                        assert_eq!(event, WebSocketMessage::Text("event".into()));
                        ws.close(None).await.unwrap();
                    }
                    assert!(matches!(
                        a.hangup_live_call(ID).await,
                        Err(AiError::EndpointNotSupported)
                    ));
                    b.hangup_live_call(ID).await.unwrap();
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Ok(())
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!("LIVE_FIXTURE_KEY=fixture-key\nLIVE_FIXTURE_ADDRESS={address}\n"),
    )?;
    assert_eq!(
        timeout(Duration::from_secs(12), app.get("/live-test"))
            .await??
            .status(),
        StatusCode::OK
    );
    timeout(Duration::from_secs(5), fixture).await??;
    Ok(())
}

async fn request(upstream: &TcpListener) -> (TcpStream, String, http::HeaderMap, Value) {
    let (socket, _) = upstream.accept().await.unwrap();
    let mut socket = BufReader::new(socket);
    let mut request = String::new();
    socket.read_line(&mut request).await.unwrap();
    let mut headers = http::HeaderMap::new();
    loop {
        let mut line = String::new();
        assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.split_once(':').unwrap();
        headers.append(
            http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            http::HeaderValue::from_str(value.trim()).unwrap(),
        );
    }
    let length = headers
        .get("content-length")
        .map_or(0, |value| value.to_str().unwrap().parse::<usize>().unwrap());
    let mut bytes = vec![0; length];
    socket.read_exact(&mut bytes).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (
        socket.into_inner(),
        request.trim_end().to_owned(),
        headers,
        body,
    )
}

async fn confirm_sideband(upstream: &TcpListener, mode: LiveProtocol) {
    let (socket, _) = upstream.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_hdr_async(
        socket,
        move |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
            assert_eq!(request.headers()["authorization"], "Bearer fixture-key");
            let expected = match mode {
                LiveProtocol::Codex => {
                    assert_eq!(
                        request.headers()["x-oai-attestation"],
                        "fixture-attestation"
                    );
                    assert_eq!(request.headers()["openai-alpha"], "quicksilver=v2");
                    assert_eq!(request.headers()["session-id"].len(), 36);
                    assert_eq!(request.headers()["thread-id"].len(), 36);
                    "/edge/codex/live_fixture"
                }
                LiveProtocol::OpenAi => "/api/v1/live/sessions/live_fixture/attach",
            };
            assert_eq!(request.uri().path(), expected);
            Ok(response)
        },
    )
    .await
    .unwrap();
    assert_eq!(
        ws.next().await.unwrap().unwrap(),
        WebSocketMessage::Text("control".into())
    );
    ws.send(WebSocketMessage::Text("event".into()))
        .await
        .unwrap();
    assert!(matches!(
        ws.next().await.unwrap().unwrap(),
        WebSocketMessage::Close(_)
    ));
    ws.flush().await.unwrap();
}
