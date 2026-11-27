use std::{error::Error, fs, time::Duration};

use openrust::{
    http::{Body, StatusCode},
    testing::TestApp,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

type TestError = Box<dyn Error + Send + Sync>;

fn app<const ALIASES: bool>() -> Result<TestApp, TestError> {
    TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder.with_providers(providers).with_config(|base,env| {
            let mut gateway=openrust_ai_gateway::default_config(env)?;
            gateway["unversioned_paths"]=ALIASES.into();
            gateway["models"]=json!({
                "openai-public":{"provider":"openai","model":"openai-private"},
                "claude-public":{"provider":"anthropic","model":"claude-private"},
                "bedrock-public":{"provider":"bedrock","model":"aws-private","endpoints":["messages/count_tokens","responses/input_tokens"]},
                "hidden":{"provider":"deepseek","model":"hidden"},
            });
            gateway["keys"]=json!({
                "caller":{"key_env":"CALLER_KEY","models":["openai-public","claude-public"],"requests_per_minute":3},
                "bedrock":{"key_env":"BEDROCK_CALLER_KEY","models":["bedrock-public"]},
            });
            Ok([
                ("ai_gateway".into(),gateway),
                ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}})),
                ("logging".into(),json!({"default":"usage","channels":{"usage":{"driver":"single","path":base.join("usage.log"),"formatter":"json"}}})),
            ].into())
        })
    })
}

#[tokio::test]
async fn bedrock_count_requires_the_original_upstream_value() -> Result<(), TestError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = tokio::spawn(async move {
        let router = axum::Router::new().route(
            "/model/aws-private/count-tokens",
            axum::routing::post(
                |headers: http::HeaderMap, bytes: axum::body::Bytes| async move {
                    assert_eq!(headers["authorization"], "Bearer bedrock-fixture");
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    let messages = &body["input"]["converse"]["messages"];
                    assert_eq!(messages[0]["role"], "user");
                    let response = match messages[0]["content"][0]["text"].as_str().unwrap() {
                        "zero" => r#"{"inputTokens":0}"#,
                        "missing" => "{}",
                        "negative" => r#"{"inputTokens":-1}"#,
                        _ => panic!("unexpected count input"),
                    };
                    ([(http::header::CONTENT_TYPE, "application/json")], response)
                },
            ),
        );
        axum::serve(listener, router).await
    });
    let mut app = app::<false>()?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nBEDROCK_URL=http://{address}\nAWS_BEARER_TOKEN_BEDROCK=bedrock-fixture\nBEDROCK_CALLER_KEY=bedrock-caller\nCALLER_KEY=caller-fixture\n"
        ),
    )?;
    for (prompt, status) in [
        ("zero", StatusCode::OK),
        ("missing", StatusCode::BAD_GATEWAY),
        ("negative", StatusCode::BAD_GATEWAY),
    ] {
        let request = http::Request::post("/v1/responses/input_tokens")
            .header("authorization", "Bearer bedrock-caller")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"model":"bedrock-public","input":prompt}).to_string(),
            ))?;
        let response = app.call(request).await?;
        assert_eq!(response.status(), status);
        if status == StatusCode::OK {
            assert_eq!(
                response.json::<Value>()?,
                json!({"object":"response.input_tokens","input_tokens":0})
            );
        }
    }
    upstream.abort();
    Ok(())
}

#[tokio::test]
async fn native_and_bridged_counts_share_keys_and_unversioned_limits() -> Result<(), TestError> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let address = upstream.local_addr()?;
    let provider = tokio::spawn(async move {
        for (stage, count) in [("openai", 41), ("claude", 37), ("anthropic-client", 42)] {
            let (socket, _) = timeout(Duration::from_secs(5), upstream.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            if stage == "claude" {
                assert_eq!(
                    line.trim_end(),
                    "POST /claude/v1/messages/count_tokens HTTP/1.1"
                );
            } else {
                assert_eq!(
                    line.trim_end(),
                    "POST /openai/v1/responses/input_tokens HTTP/1.1"
                );
            }
            let mut size = 0;
            let mut allowed = false;
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
                if (stage == "claude"
                    && key.eq_ignore_ascii_case("x-api-key")
                    && value.trim() == "claude-fixture")
                    || (stage != "claude"
                        && key.eq_ignore_ascii_case("authorization")
                        && value.trim() == "Bearer openai-fixture")
                {
                    allowed = true;
                }
            }
            assert!(allowed);
            let mut bytes = vec![0; size];
            socket.read_exact(&mut bytes).await.unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            for field in ["stream", "store", "max_tokens", "max_output_tokens"] {
                assert!(request.get(field).is_none(), "{field}");
            }
            let output = if stage == "claude" {
                assert_eq!(request["model"], "claude-private");
                assert_eq!(request["messages"][0]["content"], "Estimate this");
                json!({"input_tokens":count})
            } else {
                assert_eq!(request["model"], "openai-private");
                if stage == "anthropic-client" {
                    assert_eq!(request["input"][0]["content"], "Estimate this");
                } else {
                    assert_eq!(request["input"], "Estimate this");
                }
                json!({"object":"response.input_tokens","input_tokens":count})
            }
            .to_string();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                output.len()
            );
            socket
                .get_mut()
                .write_all(headers.as_bytes())
                .await
                .unwrap();
            socket.get_mut().write_all(output.as_bytes()).await.unwrap();
        }
    });
    let fixture_env = format!(
        "AI_GATEWAY_ENABLED=true\nOPENAI_URL=http://{address}/openai/v1\nOPENAI_API_KEY=openai-fixture\nANTHROPIC_URL=http://{address}/claude/v1\nANTHROPIC_API_KEY=claude-fixture\nCALLER_KEY=caller-fixture\nBEDROCK_CALLER_KEY=bedrock-caller\n"
    );
    let mut app = app::<true>()?;
    fs::write(app.root().join(".env.testing"), fixture_env)?;
    assert_eq!(app.get("/models").await?.status(), StatusCode::UNAUTHORIZED);
    let headers = app
        .call(
            http::Request::get("/models")
                .header("authorization", "Bearer caller-fixture")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        headers.json::<Value>()?["data"].as_array().unwrap().len(),
        2
    );
    let request = |path: &str, header: &str, key: &str, data: Value| {
        http::Request::post(path)
            .header(header, key)
            .header("content-type", "application/json")
            .body(Body::from(data.to_string()))
    };

    for (data, code) in [
        (
            json!({"conversation":"another-client"}),
            "unsupported_state",
        ),
        (
            json!({"input":[{"role":"user","content":[{"type":"input_file","file_id":"external-resource"}]}]}),
            "unsupported_count_input",
        ),
        (json!({"stream":true}), "invalid_stream"),
    ] {
        let mut data = data;
        data["model"] = "openai-public".into();
        let result = app
            .call(request(
                "/v1/responses/input_tokens",
                "authorization",
                "Bearer caller-fixture",
                data,
            )?)
            .await?;
        assert_eq!(result.status(), StatusCode::BAD_REQUEST);
        assert_eq!(result.json::<Value>()?["error"]["code"], code);
    }

    for (path, header, key, model, count) in [
        (
            "/v1/responses/input_tokens",
            "authorization",
            "Bearer caller-fixture",
            "openai-public",
            41,
        ),
        (
            "/responses/input_tokens",
            "authorization",
            "Bearer caller-fixture",
            "claude-public",
            37,
        ),
        (
            "/messages/count_tokens",
            "x-api-key",
            "caller-fixture",
            "openai-public",
            42,
        ),
    ] {
        let data = if header == "x-api-key" {
            json!({"model":model,"messages":[{"role":"user","content":"Estimate this"}]})
        } else {
            json!({"model":model,"input":"Estimate this"})
        };
        let result = app.call(request(path, header, key, data)?).await?;
        assert_eq!(result.status(), StatusCode::OK);
        let value = result.json::<Value>()?;
        assert_eq!(value["input_tokens"], count);
        if header == "authorization" {
            assert_eq!(value["object"], "response.input_tokens");
        } else {
            assert_eq!(value, json!({"input_tokens":count}));
        }
    }
    let blocked = app
        .call(request(
            "/responses/input_tokens",
            "authorization",
            "Bearer caller-fixture",
            json!({"model":"openai-public","input":"limited"}),
        )?)
        .await?;
    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    let logged = fs::read_to_string(app.root().join("usage.log"))?;
    let rows = logged
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|row| row["context"]["input_tokens"].is_null()
                && row["context"]["output_tokens"].is_null())
    );
    provider.await?;
    Ok(())
}

#[tokio::test]
async fn root_aliases_are_opt_in() -> Result<(), TestError> {
    let mut app = app::<false>()?;
    fs::write(
        app.root().join(".env.testing"),
        "AI_GATEWAY_ENABLED=true\nCALLER_KEY=caller-fixture\nBEDROCK_CALLER_KEY=bedrock-caller\n",
    )?;
    assert_eq!(app.get("/models").await?.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        app.get("/v1/models").await?.status(),
        StatusCode::UNAUTHORIZED
    );
    Ok(())
}
