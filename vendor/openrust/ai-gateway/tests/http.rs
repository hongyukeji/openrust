use std::{collections::BTreeMap, error::Error, fs, time::Duration};

use openrust::{
    config::{ConfigError, Environment},
    http::{Body, StatusCode},
    json_schema::JsonSchema,
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

#[tokio::test]
async fn anthropic_native_routes_and_sdk_share_transport() -> TestResult {
    use openrust::{foundation::configuration::Routing, http::Response, support::facades::Route};
    use openrust_ai::{Agent, Ai, Message, agent};

    fn load(
        base: &std::path::Path,
        env: &Environment,
    ) -> Result<BTreeMap<String, Value>, ConfigError> {
        let mut configuration = config(base, env)?;
        let gateway = configuration.get_mut("ai_gateway").unwrap();
        gateway["models"] =
            json!({"claude-public":{"provider":"anthropic","model":"claude-private"}});
        gateway["keys"] = json!({"caller":{"key_env":"CALLER_GENERAL","models":["claude-public"]}});
        Ok(configuration)
    }

    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/anthropic/v1", upstream.local_addr()?);
    let task = tokio::spawn(async move {
        let message = json!({
            "id":"msg_local", "type":"message", "model":"claude-private",
            "content":[{"type":"text","text":"こんにちは"}], "stop_reason":"end_turn",
            "usage":{"input_tokens":6, "output_tokens":4, "cache_read_input_tokens":2, "cache_creation_input_tokens":1}
        });
        let mut structured = message.clone();
        structured["content"][0]["text"] = json!({"a":"b"}).to_string().into();
        let structured = structured.to_string();
        let events = [
            json!({"type":"message_start","message":{"model":"claude-private","usage":{"input_tokens":6,"cache_read_input_tokens":2,"cache_creation_input_tokens":1}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"こんにちは"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4}}),
            json!({"type":"message_stop"}),
        ].into_iter().map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().unwrap())).collect::<String>();
        for (path, expected_stream, kind, payload) in [
            ("/anthropic/v1/messages", false, "application/json", message.to_string()),
            ("/anthropic/v1/messages", true, "text/event-stream", events.clone()),
            ("/anthropic/v1/messages", false, "application/json", structured.clone()),
            ("/anthropic/v1/models", false, "application/json", json!({"data":[{"id":"First","display_name":"First model"}], "has_more":true, "last_id":"First"}).to_string()),
            ("/anthropic/v1/models?after_id=First", false, "application/json", json!({"data":[{"id":"Second"}], "has_more":false, "last_id":"Second"}).to_string()),
            ("/anthropic/v1/messages", true, "text/event-stream", events.clone()),
            ("/anthropic/v1/messages/count_tokens", false, "application/json", json!({"input_tokens":6}).to_string()),
            ("/anthropic/v1/messages", false, "application/json", message.to_string()),
            ("/anthropic/v1/messages", true, "text/event-stream", events),
        ] {
            let (socket, _) = timeout(Duration::from_secs(5), upstream.accept()).await.unwrap().unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            let method = if path.contains("/models") { "GET" } else { "POST" };
            assert_eq!(line.trim_end(), format!("{method} {path} HTTP/1.1"));
            let mut headers = BTreeMap::new();
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" { break; }
                let (key, value) = line.split_once(':').unwrap();
                headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
            }
            assert_eq!(headers["x-api-key"], "anthropic-upstream");
            assert_eq!(headers["anthropic-version"], "2023-06-01");
            assert!(!headers.contains_key("authorization"));
            if method == "POST" {
                let mut body = vec![0; headers["content-length"].parse().unwrap()];
                socket.read_exact(&mut body).await.unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["model"], "claude-private");
                if body["max_tokens"] == 120 {
                    assert_eq!(body["system"], "Keep it brief");
                    assert!(body.get("store").is_none());
                    assert_eq!(body["messages"][0]["content"][0]["text"], "  Hi!  ");
                } else {
                    let messages = body["messages"].as_array().unwrap();
                    assert_eq!(messages.last().unwrap()["content"], "Hi!");
                    if body["system"] == "日本語で答えて" {
                        assert_eq!(messages[0], json!({"role":"user","content":"Earlier"}));
                        assert_eq!(messages[1], json!({"role":"assistant","content":"Earlier reply"}));
                    }
                }
                if payload == structured {
                    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
                    assert_eq!(body["output_config"]["format"]["schema"]["properties"]["a"]["type"], "string");
                }
                assert_eq!(body["stream"] == true, expected_stream);
            }
            let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
            socket.get_mut().write_all(headers.as_bytes()).await.unwrap();
            socket.get_mut().write_all(payload.as_bytes()).await.unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder
            .with_providers(providers)
            .with_config(load)
            .with_routing(Routing::default().web(|| {
                Route::get("/sdk-native", || async {
                    let response = agent("日本語で答えて")
                        .with_messages([
                            Message::user("Earlier"),
                            Message::assistant("Earlier reply"),
                        ])
                        .prompt("Hi!")
                        .await
                        .unwrap();
                    assert_eq!(response.text, "こんにちは");
                    assert_eq!(response.usage.prompt_tokens, Some(6));
                    Ok::<_, openrust::http::HttpError>(Response::empty(StatusCode::OK))
                })?;
                Route::get("/sdk-native-stream", || async {
                    Ok::<_, openrust::http::HttpError>(
                        agent("").stream("Hi!").await.unwrap().to_response(),
                    )
                })?;
                Route::get("/sdk-native-structured", || async {
                    let schema = JsonSchema::object([("a", JsonSchema::string().into())]);
                    let response = agent("").with_schema(schema).prompt("Hi!").await.unwrap();
                    Response::json(StatusCode::OK, response)
                })?;
                Route::get("/sdk-native-models", || async {
                    let catalog = Ai::models(Some("anthropic")).await.unwrap();
                    assert!(catalog.discovery_error.is_none());
                    assert_eq!(catalog.models[0].name.as_deref(), Some("First model"));
                    assert_eq!(catalog.models[1].id, "Second");
                    assert!(
                        catalog
                            .models
                            .iter()
                            .any(|model| model.configured && model.id == "claude-private")
                    );
                    Ok::<_, openrust::http::HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nAI_PROVIDER=anthropic\nANTHROPIC_URL={url}\nANTHROPIC_MODEL=claude-private\nANTHROPIC_API_KEY=anthropic-upstream\nCALLER_GENERAL=anthropic-caller\n"
        ),
    )?;
    assert_eq!(app.get("/sdk-native").await?.status(), StatusCode::OK);
    let sdk = app.get("/sdk-native-stream").await?;
    let sdk_events = std::str::from_utf8(sdk.streamed_content())?;
    assert!(sdk_events.contains("こんにちは"));
    assert!(sdk_events.contains("\"prompt_tokens\":6"));
    assert!(sdk_events.contains("\"completion_tokens\":4"));
    assert_eq!(
        app.get("/sdk-native-structured").await?.json::<Value>()?,
        json!({"a":"b"})
    );
    assert_eq!(
        app.get("/sdk-native-models").await?.status(),
        StatusCode::OK
    );

    let directory = app
        .call(
            http::Request::get("/v1/models")
                .header("x-api-key", "anthropic-caller")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(directory.json::<Value>()?["data"][0]["type"], "model");
    let model = app
        .call(
            http::Request::get("/v1/models/claude-public")
                .header("x-api-key", "anthropic-caller")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(model.json::<Value>()?["id"], "claude-public");
    let model = send(
        &mut app,
        "/v1/models/claude-public",
        Some("Bearer anthropic-caller"),
        None,
    )
    .await?;
    assert_eq!(model.json::<Value>()?["object"], "model");

    let messages = |path: &str, key: &str, stream: bool| {
        http::Request::post(path).header("x-api-key", key)
            .header("content-type", "application/json")
            .body(Body::from(json!({"model":"claude-public","max_tokens":100,"stream":stream,"messages":[{"role":"user","content":"Hi!"}]}).to_string()))
            .unwrap()
    };
    let denied = app
        .call(messages("/v1/messages", "anthropic-upstream", true))
        .await?;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        denied.json::<Value>()?["error"]["type"],
        "authentication_error"
    );
    let streamed = app
        .call(messages("/v1/messages", "anthropic-caller", true))
        .await?;
    assert_eq!(streamed.status(), StatusCode::OK);
    let frames = std::str::from_utf8(streamed.streamed_content())?;
    assert!(frames.contains("event: message_stop"));
    assert!(frames.contains("claude-public"));
    assert!(!frames.contains("claude-private"));
    let count = app
        .call(messages(
            "/v1/messages/count_tokens",
            "anthropic-caller",
            false,
        ))
        .await?;
    assert_eq!(count.json::<Value>()?["input_tokens"], 6);
    let input = json!({
        "model":"claude-public", "max_output_tokens":120, "instructions":"Keep it brief",
        "input":[{"role":"user","content":[{"type":"input_text","text":"  Hi!  "}]}],
    });
    let bearer = Some("Bearer anthropic-caller");
    let unsupported = send(
        &mut app,
        "/v1/responses",
        bearer,
        Some(json!({
            "model":"claude-public","input":"Hello","text":{"format":{"type":"json_schema"}}
        })),
    )
    .await?;
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        unsupported.json::<Value>()?["error"]["code"],
        "unsupported_conversion"
    );
    let response = send(&mut app, "/v1/responses", bearer, Some(input.clone())).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = response.json::<Value>()?;
    assert_eq!(response["model"], "claude-public");
    assert_eq!(response["output"][0]["content"][0]["text"], "こんにちは");
    assert_eq!(response["usage"]["total_tokens"], 13);
    assert_eq!(
        response["usage"]["input_tokens_details"]["cached_tokens"],
        2
    );
    assert_eq!(response["usage"]["cache_creation_input_tokens"], 1);
    let mut input = input;
    input["stream"] = true.into();
    let response = send(&mut app, "/v1/responses", bearer, Some(input)).await?;
    let text = std::str::from_utf8(response.streamed_content())?;
    let frames = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    for (sequence, frame) in frames.iter().enumerate() {
        assert_eq!(frame["sequence_number"], sequence);
    }
    assert_eq!(frames[0]["type"], "response.created");
    assert!(frames.iter().any(
        |frame| frame["type"] == "response.output_text.delta" && frame["delta"] == "こんにちは"
    ));
    let complete = frames.last().unwrap();
    assert_eq!(complete["type"], "response.completed");
    assert_eq!(complete["response"]["usage"]["output_tokens"], 4);
    assert_eq!(complete["response"]["usage"]["input_tokens"], 9);
    assert_eq!(
        complete["response"]["output"][0]["id"],
        frames[2]["item"]["id"]
    );
    assert_eq!(complete["response"]["model"], "claude-public");
    assert!(!text.contains("claude-private"));
    task.await?;
    let log = fs::read_to_string(app.root().join("gateway.log"))?;
    let rows: Vec<Value> = log
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(rows[2]["context"]["endpoint"], "responses");
    assert_eq!(rows[2]["context"]["upstream_endpoint"], "messages");
    assert_eq!(rows[3]["context"]["input_tokens"], 6);
    assert_eq!(rows[3]["context"]["output_tokens"], 4);
    Ok(())
}

#[tokio::test]
async fn anthropic_clients_bridge_to_responses_with_text_and_usage() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/private/v1", listener.local_addr()?);
    let upstream_task = tokio::spawn(async move {
        let response = json!({
            "id":"resp_upstream", "model":"private-responses", "status":"completed",
            "output":[{"type":"message","content":[{"type":"output_text","text":"ready"}]}],
            "usage":{"input_tokens":8,"output_tokens":3,"input_tokens_details":{"cached_tokens":2},"cache_creation_input_tokens":1}
        });
        let mut incomplete = response.clone();
        incomplete["status"] = "incomplete".into();
        incomplete["incomplete_details"]["reason"] = "max_output_tokens".into();
        let events = [
            json!({"type":"response.created","response":{"id":"resp_upstream"}}),
            json!({"type":"response.output_item.added","item":{"type":"message"}}),
            json!({"type":"response.content_part.added","part":{"type":"output_text","text":""},"output_index":0,"content_index":0}),
            json!({"type":"response.output_text.delta","delta":"ready","output_index":0,"content_index":0}),
            json!({"type":"response.content_part.done","part":{"type":"output_text","text":"ready"},"output_index":0,"content_index":0}),
            json!({"type":"response.incomplete","response":incomplete}),
        ].map(|event| format!("event: {}\ndata: {event}\n\n",event["type"].as_str().unwrap())).join("");
        for (stream, kind, payload) in [
            (false, "application/json", response.to_string()),
            (true, "text/event-stream", events),
        ] {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /private/v1/responses HTTP/1.1");
            let mut headers = BTreeMap::new();
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let (key, value) = line.split_once(':').unwrap();
                headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
            }
            assert_eq!(headers["authorization"], "Bearer openai-upstream");
            assert!(!headers.contains_key("x-api-key"));
            let mut body = vec![0; headers["content-length"].parse().unwrap()];
            socket.read_exact(&mut body).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["model"], "private-responses");
            assert_eq!(body["store"], false);
            assert_eq!(body["stream"], stream);
            assert_eq!(body["max_output_tokens"], 128);
            assert_eq!(body["instructions"], "Reply briefly.");
            assert_eq!(body["input"][0]["content"], "  text  ");
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            socket.get_mut().write_all(header.as_bytes()).await.unwrap();
            socket
                .get_mut()
                .write_all(payload.as_bytes())
                .await
                .unwrap();
        }
    });
    let mut app = fixture()?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nOPENAI_URL={url}\nOPENAI_API_KEY=openai-upstream\nCALLER_GENERAL=caller-all\nCALLER_LIMITED=caller-one\n"
        ),
    )?;
    let request = |body: Value| {
        http::Request::post("/v1/messages")
            .header("x-api-key", "caller-all")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
    };
    let mut query = json!({
        "model":"public-responses","system":[{"type":"text","text":"Reply briefly."}],
        "messages":[{"role":"user","content":[{"type":"text","text":"  text  "}]}],
        "max_tokens":128,"stream":false,
    });
    let mut unsupported = query.clone();
    unsupported["tools"] = json!([{"name":"unknown"}]);
    let invalid = app.call(request(unsupported)?).await?;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        invalid.json::<Value>()?["error"]["type"],
        "invalid_request_error"
    );

    let response = app.call(request(query.clone())?).await?.json::<Value>()?;
    assert_eq!(response["model"], "public-responses");
    assert_eq!(response["stop_reason"], "end_turn");
    assert_eq!(response["content"][0]["text"], "ready");
    assert_eq!(response["usage"]["input_tokens"], 5);
    assert_eq!(response["usage"]["cache_read_input_tokens"], 2);
    assert_eq!(response["usage"]["cache_creation_input_tokens"], 1);
    assert_ne!(response["id"], "resp_upstream");

    query["stream"] = true.into();
    let response = app.call(request(query)?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = std::str::from_utf8(response.streamed_content())?;
    let frames = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(frames[0]["message"]["model"], "public-responses");
    assert_eq!(frames[2]["delta"]["text"], "ready");
    assert_eq!(frames[4]["delta"]["stop_reason"], "max_tokens");
    assert_eq!(frames[4]["usage"]["output_tokens"], 3);
    assert_eq!(frames[5]["type"], "message_stop");
    assert!(!body.contains("private-responses") && !body.contains("resp_upstream"));
    upstream_task.await?;
    let log = fs::read_to_string(app.root().join("gateway.log"))?;
    let rows: Vec<Value> = log
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert!(
        rows.iter()
            .all(|row| row["context"]["endpoint"] == "messages"
                && row["context"]["upstream_endpoint"] == "responses"
                && row["context"]["input_tokens"] == 8)
    );
    Ok(())
}
