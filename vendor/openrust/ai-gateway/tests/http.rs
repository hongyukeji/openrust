use std::{collections::BTreeMap, error::Error, fs, time::Duration};

use openrust::{
    config::{ConfigError, Environment},
    http::{Body, StatusCode},
    testing::{TestApp, TestResponse},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn config(_: &std::path::Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut gateway = openrust_ai_gateway::default_config(env)?;
    gateway["models"] = json!({
        "public-chat": {"provider": "deepseek", "model": "private-chat"},
        "public-responses": {"provider": "openai", "model": "private-responses"}
    });
    gateway["keys"] = json!({
        "general": {"key_env":"CALLER_GENERAL", "models":["public-chat", "public-responses"]},
        "limited": {"key_env":"CALLER_LIMITED", "models":["public-chat"], "requests_per_minute":2}
    });
    gateway["usage_channel"] = "ai-usage".into();
    Ok(BTreeMap::from([
        ("ai_gateway".to_owned(), gateway),
        (
            "cache".to_owned(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
        (
            "logging".to_owned(),
            json!({
                "default": "ai-usage",
                "channels": {"ai-usage":{"driver":"single", "path":"gateway.log", "formatter":"json"}}
            }),
        ),
    ]))
}

fn fixture() -> Result<TestApp, Box<dyn Error + Send + Sync>> {
    TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder.with_providers(providers).with_config(config)
    })
}

async fn send(
    app: &mut TestApp,
    path: &str,
    token: Option<&str>,
    json: Option<Value>,
) -> Result<TestResponse, Box<dyn Error + Send + Sync>> {
    let mut request = http::Request::builder().uri(path);
    if let Some(token) = token {
        request = request.header("authorization", token);
    }
    app.call(if let Some(json) = json {
        request
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))?
    } else {
        request.body(Body::empty())?
    })
    .await
}

#[tokio::test]
async fn disabled_routes_are_unavailable() -> TestResult {
    let mut app = fixture()?;
    let response = app.get("/v1/models").await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn native_routes_authorize_models_relay_and_record_usage() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let upstream = listener.local_addr()?;
    let task = tokio::spawn(async move {
        for (path, status, kind, payload) in [
            (
                "chat/completions", "200 OK", "application/json",
                json!({"model":"private-chat","choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}).to_string(),
            ),
            (
                "responses", "200 OK", "text/event-stream; charset=utf-8",
                concat!(
                    "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"model\":\"private-responses\"}}\n\n",
                    "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"一\"}\n\n",
                    "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"model\":\"private-responses\",\"usage\":{\"input_tokens\":7,\"output_tokens\":4}}}\n\n"
                ).into(),
            ),
            ("chat/completions", "429 Too Many Requests", "application/json", r#"{"error":{"message":"private credential"}}"#.into()),
        ] {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), format!("POST /private/v1/{path} HTTP/1.1"));
            let mut authorized = false;
            let mut length = 0;
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" { break; }
                let (key, value) = line.split_once(':').unwrap();
                if key.eq_ignore_ascii_case("authorization") { authorized = value.trim() == "Bearer server-only-key"; }
                if key.eq_ignore_ascii_case("content-length") { length = value.trim().parse().unwrap(); }
            }
            assert!(authorized);
            let mut body = vec![0; length];
            socket.read_exact(&mut body).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            if path == "responses" {
                assert_eq!(body["model"], "private-responses");
                assert_eq!(body["store"], false);
            } else {
                assert_eq!(body["model"], "private-chat");
                assert_eq!(body["messages"][0]["content"], "  hello  ");
            }
            let headers = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nRetry-After: 3\r\nSet-Cookie: secret=1\r\nConnection: close\r\n\r\n", payload.len());
            socket.get_mut().write_all(headers.as_bytes()).await.unwrap();
            socket.get_mut().write_all(payload.as_bytes()).await.unwrap();
        }
    });

    let mut app = fixture()?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nDEEPSEEK_URL=http://{upstream}/private/v1\nDEEPSEEK_API_KEY=server-only-key\nOPENAI_URL=http://{upstream}/private/v1\nOPENAI_API_KEY=server-only-key\nCALLER_GENERAL=caller-all\nCALLER_LIMITED=caller-one\n"
        ),
    )?;
    for token in [
        None,
        Some("Bearer server-only-key"),
        Some("Basic Bearer caller-all"),
    ] {
        let response = send(&mut app, "/v1/models", token, None).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.json::<Value>()?["error"]["code"],
            "invalid_api_key"
        );
    }
    let limited = Some("Bearer caller-one");
    let general = Some("Bearer caller-all");
    for (token, count) in [(limited, 1), (general, 2)] {
        let response = send(&mut app, "/v1/models", token, None)
            .await?
            .json::<Value>()?;
        assert_eq!(response["data"].as_array().unwrap().len(), count);
        assert_eq!(response["data"][0]["id"], "public-chat");
        assert!(!response.to_string().contains("private-chat"));
    }
    let chat = json!({"model":"public-chat","messages":[{"role":"user","content":"  hello  "}]});
    for name in ["public-responses", "private-chat", "missing"] {
        let response = send(
            &mut app,
            "/v1/chat/completions",
            limited,
            Some(json!({"model":name})),
        )
        .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response.json::<Value>()?["error"]["code"],
            "model_not_found"
        );
    }
    let invalid_json = http::Request::post("/v1/chat/completions")
        .header("authorization", "Bearer caller-one")
        .header("content-type", "application/json")
        .body(Body::from("{"))?;
    assert_eq!(
        app.call(invalid_json).await?.status(),
        StatusCode::BAD_REQUEST
    );
    for state in [
        json!({"store":true}),
        json!({"previous_response_id":"resp_other"}),
        json!({"input":[{"type":"item_reference","id":"item_other"}]}),
        json!({"tools":[{"type":"file_search","vector_store_ids":["vs_other"]}]}),
    ] {
        let mut state = state;
        state["model"] = "public-responses".into();
        let response = send(&mut app, "/v1/responses", general, Some(state)).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json::<Value>()?["error"]["code"],
            "unsupported_state"
        );
    }
    let response = send(&mut app, "/v1/responses", general, Some(chat.clone())).await?;
    assert_eq!(
        response.json::<Value>()?["error"]["code"],
        "unsupported_endpoint"
    );
    let response = send(
        &mut app,
        "/v1/chat/completions",
        limited,
        Some(chat.clone()),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>()?["model"], "public-chat");
    let response = send(
        &mut app,
        "/v1/responses",
        general,
        Some(json!({"model":"public-responses","stream":true,"input":"hello"})),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let frames = std::str::from_utf8(response.streamed_content())?;
    assert!(frames.contains("event: response.completed"));
    assert!(frames.contains("\"model\":\"public-responses\""));
    assert!(!frames.contains("private-responses"));
    let response = send(
        &mut app,
        "/v1/chat/completions",
        limited,
        Some(chat.clone()),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.json::<Value>()?["error"]["code"], "upstream_error");
    assert_eq!(response.headers().get("retry-after").unwrap(), "3");
    assert!(!response.headers().contains_key("set-cookie"));
    let limited_response = send(&mut app, "/v1/chat/completions", limited, Some(chat)).await?;
    assert_eq!(limited_response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        limited_response.json::<Value>()?["error"]["code"],
        "rate_limit_exceeded"
    );
    assert!(limited_response.headers().contains_key("retry-after"));
    task.await?;

    let log = fs::read_to_string(app.root().join("gateway.log"))?;
    let rows: Vec<Value> = log
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["context"]["key"], "limited");
    assert_eq!(rows[0]["context"]["input_tokens"], 5);
    assert_eq!(rows[1]["context"]["outcome"], "completed");
    assert_eq!(rows[1]["context"]["output_tokens"], 4);
    assert_eq!(rows[2]["context"]["status"], 429);
    assert!(rows[2]["context"]["input_tokens"].is_null());
    assert!(!log.contains("server-only-key") && !log.contains("caller-one"));
    Ok(())
}
