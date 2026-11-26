use std::{error::Error, fs, time::Duration};

use openrust::{
    foundation::configuration::Routing,
    support::facades::Route,
    testing::{TestApp, TestResponse},
};
use openrust_ai::{Agent, agent};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn frames(response: &TestResponse) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
    Ok(std::str::from_utf8(response.streamed_content())?
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

#[tokio::test]
async fn vercel_and_agent_ui_stream_text_tokens_and_mask_failures() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/chat/v1", listener.local_addr()?);
    let fixture = tokio::spawn(async move {
        let start =
            "data: {\"type\":\"response.created\",\"response\":{\"model\":\"fixture\"}}\n\n";
        let delta = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n";
        let done = "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\n";
        let error = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"upstream secret\"}}}\n\n";
        for end in [done, done, error, error] {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /chat/v1/responses HTTP/1.1");
            let mut size = 0;
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let (key, value) = line.split_once(':').unwrap();
                if key.eq_ignore_ascii_case("content-length") {
                    size = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; size];
            socket.read_exact(&mut body).await.unwrap();
            let data: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(data["stream"], true);
            let payload = format!("{start}{delta}{end}");
            let wire = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            socket.get_mut().write_all(wire.as_bytes()).await.unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/vercel", || async {
                    agent("")
                        .stream("test")
                        .await
                        .unwrap()
                        .using_vercel_data_protocol(Some("continued-message".into()))
                        .to_response()
                })?;
                Route::get("/ag-ui", || async {
                    agent("")
                        .stream("test")
                        .await
                        .unwrap()
                        .using_agent_user_interaction_protocol(
                            Some("thread".into()),
                            Some("run".into()),
                        )
                        .to_response()
                })?;
                Ok(())
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=openai\nOPENAI_MODEL=fixture\nOPENAI_URL={url}\nOPENAI_API_KEY=fixture-only\n"
        ),
    )?;

    let ui = app.get("/vercel").await?;
    ui.assert_ok();
    assert_eq!(
        ui.headers().get("x-vercel-ai-ui-message-stream").unwrap(),
        "v1"
    );
    assert_eq!(
        ui.headers().get("cache-control").unwrap(),
        "no-cache, no-transform"
    );
    assert!(ui.streamed_content().ends_with(b"data: [DONE]\n\n"));
    let events = frames(&ui)?;
    assert_eq!(events[0]["messageId"], "continued-message");
    assert_eq!(events[1]["type"], "start-step");
    assert_eq!(events[3]["delta"], "hello");
    assert_eq!(events[2]["id"], events[4]["id"]);
    assert_eq!(events[5]["type"], "finish-step");
    assert_eq!(events[6]["finishReason"], "stop");
    assert_eq!(events[6]["messageMetadata"]["usage"]["totalTokens"], 5);

    let ag = app.get("/ag-ui").await?;
    ag.assert_ok();
    assert_eq!(
        ag.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    assert!(!ag.streamed_content().ends_with(b"data: [DONE]\n\n"));
    let events = frames(&ag)?;
    assert_eq!(events[0]["type"], "RUN_STARTED");
    assert_eq!(events[0]["threadId"], "thread");
    assert_eq!(events[0]["runId"], "run");
    assert_eq!(events[1]["stepName"], "1");
    assert_eq!(events[2]["role"], "assistant");
    assert_eq!(events[3]["delta"], "hello");
    assert_eq!(events[5]["type"], "STEP_FINISHED");
    assert_eq!(events[6]["type"], "RUN_FINISHED");
    assert_eq!(events[6]["usage"][0]["inputTokens"], 3);
    assert_eq!(events[6]["usage"][0]["provider"], "openai");

    for (path, terminal) in [("/vercel", "error"), ("/ag-ui", "RUN_ERROR")] {
        let response = app.get(path).await?;
        let events = frames(&response)?;
        assert_eq!(events.last().unwrap()["type"], terminal);
        assert!(!std::str::from_utf8(response.streamed_content())?.contains("upstream secret"));
    }
    fixture.await?;
    Ok(())
}
