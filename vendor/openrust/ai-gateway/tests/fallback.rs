use std::{collections::BTreeMap, error::Error, fs, path::Path, time::Duration};

use openrust::{
    config::{ConfigError, Environment},
    http::{Body, StatusCode},
    testing::{TestApp, TestResponse},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn config(_: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut gateway = openrust_ai_gateway::default_config(env)?;
    gateway["models"] = json!({"public":{
        "provider":"deepseek","model":"primary","endpoints":["chat/completions"],
        "fallbacks":[{"provider":"openai","model":"backup"}],
    }});
    gateway["keys"] =
        json!({"caller":{"key_env":"CALLER_KEY","models":["public"],"requests_per_minute":5}});
    Ok(BTreeMap::from([
        ("ai_gateway".into(), gateway),
        (
            "cache".into(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
        (
            "logging".into(),
            json!({"default":"calls","channels":{"calls":{"driver":"single","path":"calls.log","formatter":"json"}}}),
        ),
    ]))
}

async fn fixture(
    token: &'static str,
    model: &'static str,
    replies: Vec<(&'static str, &'static str, &'static str)>,
) -> (String, JoinHandle<()>) {
    fixture_with_headers(token, model, replies, "").await
}

async fn fixture_with_headers(
    token: &'static str,
    model: &'static str,
    replies: Vec<(&'static str, &'static str, &'static str)>,
    additional_headers: &'static str,
) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/test/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for (status, kind, payload) in replies {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /test/v1/chat/completions HTTP/1.1");
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
            assert_eq!(headers["authorization"], format!("Bearer {token}"));
            let mut body = vec![0; headers["content-length"].parse().unwrap()];
            socket.read_exact(&mut body).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["model"], model);
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n{additional_headers}\r\n",
                payload.len()
            );
            socket.get_mut().write_all(head.as_bytes()).await.unwrap();
            socket
                .get_mut()
                .write_all(payload.as_bytes())
                .await
                .unwrap();
        }
    });
    (url, server)
}

async fn send(
    app: &mut TestApp,
    payload: Value,
) -> Result<TestResponse, Box<dyn Error + Send + Sync>> {
    app.call(
        http::Request::post("/v1/chat/completions")
            .header("authorization", "Bearer caller-fixture")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))?,
    )
    .await
}

#[tokio::test]
async fn fallback_is_ordered_and_stops_before_replaying_tools_or_a_started_stream() -> TestResult {
    let (primary, first) = fixture("primary-key", "primary", vec![
        ("429 Too Many Requests", "application/json", r#"{"error":{"message":"first credential"}}"#),
        ("400 Bad Request", "application/json", "{}"),
        ("200 OK", "text/event-stream", "data: {\"model\":\"primary\",\"choices\":[{\"delta\":{\"content\":\"starts\"}}]}\n\n"),
        ("429 Too Many Requests", "application/json", "{}"),
        ("503 Service Unavailable", "application/json", "{}"),
    ]).await;
    let (backup, second) = fixture("backup-key", "backup", vec![
        ("200 OK", "application/json", r#"{"model":"backup","choices":[{"message":{"content":"backup"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#),
        ("200 OK", "text/event-stream", "data: {\"model\":\"backup\",\"choices\":[{\"delta\":{\"content\":\"fallback stream\"}}]}\n\ndata: [DONE]\n\n"),
    ]).await;
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder.with_providers(providers).with_config(config)
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nDEEPSEEK_URL={primary}\nDEEPSEEK_API_KEY=primary-key\nOPENAI_URL={backup}\nOPENAI_API_KEY=backup-key\nCALLER_KEY=caller-fixture\n"
        ),
    )?;
    let mut payload = json!({"model":"public","messages":[{"role":"user","content":"hello"}]});
    let response = send(&mut app, payload.clone()).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>()?["model"], "public");
    assert_eq!(response.json::<Value>()?["usage"]["prompt_tokens"], 5);

    let response = send(&mut app, payload.clone()).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    payload["stream"] = true.into();
    let response = send(&mut app, payload.clone()).await?;
    let body = std::str::from_utf8(response.streamed_content())?;
    assert!(body.contains("starts") && body.contains("event: error"));

    let mut tool_request = payload.clone();
    tool_request["tools"] = json!([{"type":"function","function":{"name":"example"}}]);
    let response = send(&mut app, tool_request).await?;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.json::<Value>()?["error"]["code"], "upstream_error");

    let response = send(&mut app, payload.clone()).await?;
    assert!(std::str::from_utf8(response.streamed_content())?.contains("fallback stream"));
    let response = send(&mut app, payload).await?;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.json::<Value>()?["error"]["code"],
        "rate_limit_exceeded"
    );

    first.await?;
    second.await?;
    let logs = fs::read_to_string(app.root().join("calls.log"))?;
    let rows: Vec<Value> = logs
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(rows.len(), 7);
    assert_eq!(
        rows[0]["context"]["request_id"],
        rows[1]["context"]["request_id"]
    );
    assert_eq!(rows[0]["context"]["provider"], "deepseek");
    assert_eq!(rows[1]["context"]["provider"], "openai");
    assert_eq!(rows[1]["context"]["input_tokens"], 5);
    assert_eq!(rows[3]["context"]["outcome"], "incomplete_stream");
    assert_ne!(
        rows[4]["context"]["request_id"],
        rows[5]["context"]["request_id"]
    );
    assert_eq!(
        rows[5]["context"]["request_id"],
        rows[6]["context"]["request_id"]
    );
    assert!(
        !logs.contains("caller-fixture")
            && !logs.contains("primary-key")
            && !logs.contains("backup-key")
    );
    Ok(())
}

fn cooldown_config(path: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut config = config(path, env)?;
    let gateway = config.get_mut("ai_gateway").unwrap();
    gateway["models"]["second-alias"] = json!({"provider":"deepseek","model":"different"});
    gateway["keys"]["caller"]["models"] = json!(["public", "second-alias"]);
    gateway["accounts"]["deepseek"] = json!({"cooldown_seconds":5});
    Ok(config)
}

#[tokio::test]
async fn account_cooldown_is_shared_across_aliases_and_obeys_retry_after() -> TestResult {
    let (primary, first) = fixture_with_headers(
        "primary-key",
        "primary",
        vec![(
            "429 Too Many Requests",
            "application/json",
            r#"{"error":{"message":"private provider detail"}}"#,
        )],
        "Retry-After: 20\r\n",
    )
    .await;
    let reply = (
        "200 OK",
        "application/json",
        r#"{"model":"backup","choices":[{"message":{"content":"backup"}}]}"#,
    );
    let (backup, second) = fixture("backup-key", "backup", vec![reply, reply]).await;
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder
            .with_providers(providers)
            .with_config(cooldown_config)
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nDEEPSEEK_URL={primary}\nDEEPSEEK_API_KEY=primary-key\nOPENAI_URL={backup}\nOPENAI_API_KEY=backup-key\nCALLER_KEY=caller-fixture\n"
        ),
    )?;
    let mut body = json!({"model":"public","messages":[{"role":"user","content":"hello"}]});
    let first_call = send(&mut app, body.clone()).await?;
    assert_eq!(first_call.status(), StatusCode::OK);
    let fallback = send(&mut app, body.clone()).await?;
    assert_eq!(fallback.status(), StatusCode::OK);
    assert_eq!(
        fallback.json::<Value>()?["choices"][0]["message"]["content"],
        "backup"
    );
    body["model"] = "second-alias".into();
    let response = send(&mut app, body).await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.json::<Value>()?["error"]["code"],
        "upstream_unavailable"
    );
    let seconds = response.headers()["retry-after"].to_str()?.parse::<u64>()?;
    assert!((6..=20).contains(&seconds), "{seconds}");
    first.await?;
    second.await?;
    let logs = fs::read_to_string(app.root().join("calls.log"))?;
    assert_eq!(logs.lines().count(), 3);
    Ok(())
}

fn group_config(path: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut config = config(path, env)?;
    let gateway = config.get_mut("ai_gateway").unwrap();
    gateway["models"]["hidden"] = json!({"provider":"openai","model":"backup"});
    gateway["groups"] = json!({
        "primary-team":{"models":["public"],"providers":["deepseek"]},
        "backup-team":{"models":["public","hidden"],"providers":["openai"]},
    });
    gateway["keys"] = json!({
        "caller":{"key_env":"CALLER_KEY","group":"primary-team"},
        "backup":{"key_env":"BACKUP_CALLER_KEY","group":"backup-team","models":["public"]},
    });
    Ok(config)
}

#[tokio::test]
async fn gateway_group_filters_catalogue_targets_and_fallbacks() -> TestResult {
    let (primary, first) = fixture(
        "primary-key",
        "primary",
        vec![("429 Too Many Requests", "application/json", "{}")],
    )
    .await;
    let (backup, second) = fixture(
        "backup-key",
        "backup",
        vec![(
            "200 OK",
            "application/json",
            r#"{"model":"backup","choices":[{"message":{"content":"backup only"}}]}"#,
        )],
    )
    .await;
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder.with_providers(providers).with_config(group_config)
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nDEEPSEEK_URL={primary}\nDEEPSEEK_API_KEY=primary-key\nOPENAI_URL={backup}\nOPENAI_API_KEY=backup-key\nCALLER_KEY=caller-fixture\nBACKUP_CALLER_KEY=caller-backup\n"
        ),
    )?;
    let first_body = json!({"model":"public","messages":[{"role":"user","content":"hello"}]});
    let blocked = send(&mut app, first_body.clone()).await?;
    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(blocked.json::<Value>()?["error"]["code"], "upstream_error");

    for token in ["caller-fixture", "caller-backup"] {
        let listing = app
            .call(
                http::Request::get("/v1/models")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())?,
            )
            .await?;
        let list = listing.json::<Value>()?;
        assert_eq!(list["data"].as_array().unwrap().len(), 1);
        assert_eq!(list["data"][0]["id"], "public");
        let hidden = app
            .call(
                http::Request::get("/v1/models/hidden")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    }
    let allowed = app
        .call(
            http::Request::post("/v1/chat/completions")
                .header("authorization", "Bearer caller-backup")
                .header("content-type", "application/json")
                .body(Body::from(first_body.to_string()))?,
        )
        .await?;
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(
        allowed.json::<Value>()?["choices"][0]["message"]["content"],
        "backup only"
    );
    first.await?;
    second.await?;
    let logs = fs::read_to_string(app.root().join("calls.log"))?;
    let calls: Vec<Value> = logs
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["context"]["group"], "primary-team");
    assert_eq!(calls[1]["context"]["group"], "backup-team");
    Ok(())
}
