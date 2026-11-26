use std::{collections::BTreeMap, error::Error, fs, time::Duration};

use openrust::{
    config::{ConfigError, Environment},
    foundation::configuration::Routing,
    http::{Body, Response, StatusCode},
    json_schema::JsonSchema,
    support::facades::Route,
    testing::{TestApp, TestResponse},
};
use openrust_ai::{Agent, Ai, Message, agent};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn config(_: &std::path::Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut gateway = openrust_ai_gateway::default_config(env)?;
    gateway["models"] = json!({"public-gemini":{"provider":"gemini","model":"private.gemini"}});
    gateway["keys"] = json!({"caller":{"key_env":"CALLER_KEY","models":["public-gemini"]}});
    Ok(BTreeMap::from([("ai_gateway".into(), gateway)]))
}

async fn send(
    app: &mut TestApp,
    path: &str,
    key: Option<&str>,
    body: Option<Value>,
) -> Result<TestResponse, Box<dyn Error + Send + Sync>> {
    let mut request = http::Request::builder().uri(path);
    if let Some(key) = key {
        request = request.header("x-goog-api-key", key);
    }
    let request = if let Some(body) = body {
        request
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))?
    } else {
        request.body(Body::empty())?
    };
    app.call(request).await
}

#[tokio::test]
async fn gemini_sdk_and_google_native_routes() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/google/v1beta", listener.local_addr()?);
    let fixture = tokio::spawn(async move {
        let text = json!({
            "modelVersion":"private.gemini",
            "candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"内容"}]}}],
            "usageMetadata":{"promptTokenCount":8, "candidatesTokenCount":2}
        });
        let mut structured = text.clone();
        structured["candidates"][0]["content"]["parts"][0]["text"] =
            json!({"a":"b"}).to_string().into();
        let structured = structured.to_string();
        let content = text.to_string();
        let sse = [
            json!({"modelVersion":"private.gemini","candidates":[{"content":{"parts":[{"text":"内"}]}}]}),
            json!({"modelVersion":"private.gemini","candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"容"}]}}], "usageMetadata":{"promptTokenCount":8,"candidatesTokenCount":2}})
        ].map(|value| format!("data: {value}\n\n")).join("");
        for (path, kind, payload) in [
            ("/google/v1beta/models/private.gemini:generateContent", "application/json", content.clone()),
            ("/google/v1beta/models/private.gemini:streamGenerateContent?alt=sse", "text/event-stream", sse.clone()),
            ("/google/v1beta/models/private.gemini:generateContent", "application/json", structured.clone()),
            ("/google/v1beta/models", "application/json", json!({"models":[{"name":"models/private.gemini","displayName":"Private Gemini"}],"nextPageToken":"two"}).to_string()),
            ("/google/v1beta/models?pageToken=two", "application/json", json!({"models":[{"name":"models/second"}]}).to_string()),
            ("/google/v1beta/models/private.gemini:generateContent", "application/json", content),
            ("/google/v1beta/models/private.gemini:streamGenerateContent?alt=sse", "text/event-stream", sse),
            ("/google/v1beta/models/private.gemini:countTokens", "application/json", json!({"totalTokens":8}).to_string()),
        ] {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            let method = if path.contains(":") { "POST" } else { "GET" };
            assert_eq!(line.trim_end(), format!("{method} {path} HTTP/1.1"));
            let mut headers = BTreeMap::new();
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" { break; }
                let (key, value) = line.split_once(':').unwrap();
                headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
            }
            assert_eq!(headers["x-goog-api-key"], "server-gemini");
            assert!(!headers.contains_key("authorization"));
            if method == "POST" {
                let mut body = vec![0; headers["content-length"].parse().unwrap()];
                socket.read_exact(&mut body).await.unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                let contents = body["contents"].as_array().unwrap();
                assert_eq!(contents.last().unwrap()["parts"][0]["text"], "Hi.");
                if body.get("systemInstruction").is_some() {
                    assert_eq!(contents[0], json!({"role":"user","parts":[{"text":"Earlier"}]}));
                    assert_eq!(contents[1], json!({"role":"model","parts":[{"text":"Earlier reply"}]}));
                }
                if payload == structured {
                    assert_eq!(body["generationConfig"]["responseMimeType"], "application/json");
                    assert_eq!(body["generationConfig"]["responseJsonSchema"]["properties"]["a"]["type"], "string");
                }
                assert!(body.get("model").is_none());
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
            .with_config(config)
            .with_routing(Routing::default().web(|| {
                Route::get("/sdk-gemini", || async {
                    let response = agent("Please reply.")
                        .with_messages([
                            Message::user("Earlier"),
                            Message::assistant("Earlier reply"),
                        ])
                        .prompt("Hi.")
                        .await
                        .unwrap();
                    assert_eq!(response.text, "内容");
                    assert_eq!(response.usage.prompt_tokens, Some(8));
                    Ok::<_, openrust::http::HttpError>(Response::empty(StatusCode::OK))
                })?;
                Route::get("/sdk-gemini-stream", || async {
                    Ok::<_, openrust::http::HttpError>(
                        agent("").stream("Hi.").await.unwrap().to_response(),
                    )
                })?;
                Route::get("/sdk-gemini-structured", || async {
                    let schema = JsonSchema::object([("a", JsonSchema::string().into())]);
                    let response = agent("").with_schema(schema).prompt("Hi.").await.unwrap();
                    Response::json(StatusCode::OK, response)
                })?;
                Route::get("/sdk-gemini-models", || async {
                    let models = Ai::models(Some("gemini")).await.unwrap();
                    assert!(models.discovery_error.is_none());
                    assert_eq!(models.models[0].name.as_deref(), Some("Private Gemini"));
                    assert_eq!(models.models[1].id, "second");
                    assert!(models.models[0].configured && models.models[0].discovered);
                    Ok::<_, openrust::http::HttpError>(Response::empty(StatusCode::OK))
                })?;
                Ok(())
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nAI_PROVIDER=gemini\nGEMINI_URL={url}\nGEMINI_API_KEY=server-gemini\nGEMINI_MODEL=models/private.gemini\nCALLER_KEY=downstream-gemini\n"
        ),
    )?;
    assert_eq!(app.get("/sdk-gemini").await?.status(), StatusCode::OK);
    let sdk = app.get("/sdk-gemini-stream").await?;
    assert!(std::str::from_utf8(sdk.streamed_content())?.contains("\"completion_tokens\":2"));
    assert_eq!(
        app.get("/sdk-gemini-structured").await?.json::<Value>()?,
        json!({"a":"b"})
    );
    assert_eq!(
        app.get("/sdk-gemini-models").await?.status(),
        StatusCode::OK
    );
    let key = Some("downstream-gemini");
    let denied = send(&mut app, "/v1beta/models", Some("server-gemini"), None).await?;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        denied.json::<Value>()?["error"]["status"],
        "UNAUTHENTICATED"
    );
    let models = send(&mut app, "/v1beta/models", key, None)
        .await?
        .json::<Value>()?;
    assert_eq!(models["models"][0]["name"], "models/public-gemini");
    let model = send(&mut app, "/v1beta/models/public-gemini", key, None)
        .await?
        .json::<Value>()?;
    assert_eq!(model["name"], "models/public-gemini");
    let input = json!({"contents":[{"role":"user","parts":[{"text":"Hi."}]}]});
    let response = send(
        &mut app,
        "/v1beta/models/public-gemini:generateContent",
        key,
        Some(input.clone()),
    )
    .await?;
    assert_eq!(response.json::<Value>()?["modelVersion"], "public-gemini");
    let response = send(
        &mut app,
        "/v1beta/models/public-gemini:streamGenerateContent?alt=sse",
        key,
        Some(input.clone()),
    )
    .await?;
    let text = std::str::from_utf8(response.streamed_content())?;
    assert!(text.contains("public-gemini") && !text.contains("private.gemini"));
    let response = send(
        &mut app,
        "/v1beta/models/public-gemini:countTokens",
        key,
        Some(input),
    )
    .await?;
    assert_eq!(response.json::<Value>()?["totalTokens"], 8);
    fixture.await?;
    Ok(())
}
