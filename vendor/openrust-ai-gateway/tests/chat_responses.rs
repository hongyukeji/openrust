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

#[tokio::test]
async fn responses_relay_to_chat_only_connections_in_json_and_sse()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let address = upstream.local_addr()?;
    let schema_format = json!({
        "type":"json_schema","name":"result","description":"Completion status","strict":true,
        "schema":{"type":"object","properties":{"done":{"type":"boolean"}},
        "required":["done"],"additionalProperties":false},
    });
    let upstream_schema = schema_format.clone();
    let relay = tokio::spawn(async move {
        for stage in [
            "json",
            "schema",
            "inline_media",
            "stream",
            "cut",
            "json_tool",
            "history",
            "tool_stream",
            "bad_tool",
        ] {
            let (socket, _) = timeout(Duration::from_secs(5), upstream.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /v1/chat/completions HTTP/1.1");
            let mut size = 0;
            let mut authorized = false;
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
                if key.eq_ignore_ascii_case("authorization") {
                    authorized = value.trim() == "Bearer upstream-key";
                }
            }
            assert!(authorized);
            let mut body = vec![0; size];
            socket.read_exact(&mut body).await.unwrap();
            let payload: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(payload["model"], "upstream-model");
            assert_eq!(payload["max_tokens"], 64);
            assert!(payload.get("store").is_none());
            assert_eq!(
                &payload["messages"].as_array().unwrap()
                    [..if stage == "inline_media" { 3 } else { 4 }],
                json!([
                    {"role":"system","content":"Be concise"},
                    {"role":"user","content":"Hello world"},
                    {"role":"assistant","content":"Hello"},
                    {"role":"user","content":"Next?"},
                ])
                .as_array()
                .map(|messages| &messages[..if stage == "inline_media" { 3 } else { 4 }])
                .unwrap()
            );
            if matches!(stage, "json_tool" | "history" | "tool_stream" | "bad_tool") {
                assert_eq!(payload["tools"][0]["function"]["name"], "lookup");
                assert_eq!(
                    payload["tools"][0]["function"]["parameters"]["type"],
                    "object"
                );
                assert_eq!(payload["tool_choice"]["function"]["name"], "lookup");
            }
            let (kind, body) = if stage == "json" {
                assert!(payload["stream"].is_null());
                assert_eq!(payload["response_format"]["type"], "json_object");
                ("application/json",json!({"choices":[{"message":{"content":"{\"done\":true}"},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":10,"completion_tokens":4,"prompt_cache_hit_tokens":3,
                    "completion_tokens_details":{"reasoning_tokens":1}}}).to_string())
            } else if stage == "schema" {
                let mut definition = upstream_schema.as_object().unwrap().clone();
                definition.remove("type");
                assert_eq!(
                    payload["response_format"],
                    json!({"type":"json_schema","json_schema":definition})
                );
                (
                    "application/json",
                    json!({"choices":[{"message":{"content":"{\"done\":true}"},"finish_reason":"stop"}],
                        "usage":{"prompt_tokens":10,"completion_tokens":4,
                        "prompt_tokens_details":{"cached_tokens":3,"cache_write_tokens":2}}}).to_string(),
                )
            } else if stage == "inline_media" {
                assert_eq!(
                    payload["messages"][3]["content"],
                    json!([
                        {"type":"text","text":"Next?"},
                        {"type":"image_url","image_url":{"url":"data:image/png;base64,AA==","detail":"auto"}},
                        {"type":"file","file":{"filename":"fixture.pdf","file_data":"JVBERi0="}},
                    ])
                );
                (
                    "application/json",
                    json!({"choices":[{"message":{"content":"media"},"finish_reason":"stop"}]})
                        .to_string(),
                )
            } else if stage == "json_tool" {
                ("application/json",json!({"choices":[{"message":{"content":null,"tool_calls":[
                    {"id":"call_one","type":"function","function":{"name":"lookup","arguments":"{\"sku\":1}"}}
                ]},"finish_reason":"tool_calls"}]}).to_string())
            } else if stage == "history" {
                assert_eq!(payload["messages"][4]["tool_calls"][0]["id"], "call_one");
                assert_eq!(
                    payload["messages"][5],
                    json!({"role":"tool","tool_call_id":"call_one","content":"found"})
                );
                (
                    "application/json",
                    json!({"choices":[{"message":{"content":"confirmed"},"finish_reason":"stop"}]})
                        .to_string(),
                )
            } else if matches!(stage, "tool_stream" | "bad_tool") {
                let mut frames = vec![
                    json!({"choices":[{"delta":{"content":"Checking"},"finish_reason":null}]}),
                    json!({"choices":[{"delta":{"tool_calls":[
                        {"index":0,"id":"call_one","type":"function","function":{"name":"lookup","arguments":"{\"sku\":"}},
                        {"index":1,"id":"call_two","type":"function","function":{"name":"lookup","arguments":"{\"sku\":"}}
                    ]},"finish_reason":null}]}),
                ];
                if stage == "tool_stream" {
                    frames.push(json!({"choices":[{"delta":{"tool_calls":[
                        {"index":1,"function":{"arguments":"2}"}},
                        {"index":0,"function":{"arguments":"1}"}}
                    ]},"finish_reason":"tool_calls"}]}));
                } else {
                    frames.push(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}));
                }
                let sse = frames
                    .into_iter()
                    .map(|frame| format!("data: {frame}\n\n"))
                    .collect::<String>()
                    + "data: [DONE]\n\n";
                ("text/event-stream", sse)
            } else {
                assert_eq!(payload["stream"], true);
                assert_eq!(payload["stream_options"]["include_usage"], true);
                let mut data="data: {\"choices\":[{\"delta\":{\"content\":\"early\"},\"finish_reason\":null}]}\n\n".to_owned();
                if stage == "stream" {
                    data.push_str("data: {\"choices\":[{\"delta\":{\"content\":\" done\"},\"finish_reason\":\"stop\"}]}\n\n");
                    data.push_str("data: {\"choices\":[],\"usage\":{\"prompt_tokens\":13,\"completion_tokens\":3}}\n\n");
                }
                data.push_str("data: [DONE]\n\n");
                ("text/event-stream", data)
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.get_mut().write_all(head.as_bytes()).await.unwrap();
            socket.get_mut().write_all(body.as_bytes()).await.unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder.with_providers(providers).with_config(|_,env| {
            let mut gateway=openrust_ai_gateway::default_config(env)?;
            gateway["models"]=json!({"public":{
                "provider":"deepseek","model":"upstream-model","endpoints":["responses","chat/completions"],
            }});
            gateway["keys"]=json!({"caller":{"key_env":"CALLER_KEY","models":["public"]}});
            Ok([("ai_gateway".into(),gateway)].into())
        })
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nDEEPSEEK_URL=http://{address}/v1\nDEEPSEEK_API_KEY=upstream-key\nCALLER_KEY=downstream-key\n"
        ),
    )?;
    let mut payload = json!({
        "model":"public","instructions":"Be concise","max_output_tokens":64,
        "input":[
            {"role":"user","content":[{"type":"input_text","text":"Hello "},{"type":"input_text","text":"world"}]},
            {"role":"assistant","content":[{"type":"output_text","text":"Hello"}],"status":"completed"},
            {"role":"user","content":"Next?"},
        ],
        "text":{"format":{"type":"json_object"}},
    });
    let request = |value: &Value| {
        http::Request::post("/v1/responses")
            .header("authorization", "Bearer downstream-key")
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
    };
    let response = app.call(request(&payload)?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response.json::<Value>()?;
    assert_eq!(value["model"], "public");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["text"]["format"]["type"], "json_object");
    assert_eq!(value["output"][0]["content"][0]["text"], "{\"done\":true}");
    assert_eq!(value["usage"]["input_tokens"], 10);
    assert_eq!(value["usage"]["input_tokens_details"]["cached_tokens"], 3);
    assert_eq!(
        value["usage"]["output_tokens_details"]["reasoning_tokens"],
        1
    );
    assert_eq!(value["usage"]["total_tokens"], 14);

    let mut structured = payload.clone();
    structured["text"]["format"] = schema_format.clone();
    let response = app.call(request(&structured)?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value = response.json::<Value>()?;
    assert_eq!(value["text"]["format"], schema_format);
    assert_eq!(value["output"][0]["content"][0]["text"], "{\"done\":true}");
    assert_eq!(value["usage"]["input_tokens"], 10);
    assert_eq!(value["usage"]["input_tokens_details"]["cached_tokens"], 3);
    assert_eq!(value["usage"]["cache_creation_input_tokens"], 2);

    let mut media = payload.clone();
    media["input"][2]["content"] = json!([
        {"type":"input_text","text":"Next?"},
        {"type":"input_image","image_url":"https://example.invalid/input.png","detail":"auto"},
        {"type":"input_file","filename":"fixture.pdf","file_data":"JVBERi0="},
    ]);
    assert_eq!(
        app.call(request(&media)?).await?.status(),
        StatusCode::BAD_REQUEST
    );
    media["input"][2]["content"][1]["image_url"] = "data:image/png;base64,AA==".into();
    let response = app.call(request(&media)?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>()?["output"][0]["content"][0]["text"],
        "media"
    );

    let mut unsupported = payload.clone();
    unsupported["tools"] = json!([{"type":"web_search"}]);
    let error = app.call(request(&unsupported)?).await?;
    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error.json::<Value>()?["error"]["code"], "unsupported_state");

    payload["stream"] = true.into();
    payload.as_object_mut().unwrap().remove("text");
    let response = app.call(request(&payload)?).await?;
    let body = std::str::from_utf8(response.streamed_content())?;
    assert!(body.contains("response.output_text.delta") && body.contains("early done"));
    assert!(body.contains("response.completed") && body.contains("\"total_tokens\":16"));
    let response = app.call(request(&payload)?).await?;
    let body = std::str::from_utf8(response.streamed_content())?;
    assert!(body.contains("event: error") && !body.contains("response.completed"));
    payload.as_object_mut().unwrap().remove("stream");
    payload["tools"] = json!([{"type":"function","name":"lookup","parameters":{"type":"object"}}]);
    payload["tool_choice"] = json!({"type":"function","name":"lookup"});
    let response = app.call(request(&payload)?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = response.json::<Value>()?;
    assert_eq!(response["output"][0]["type"], "function_call");
    assert_eq!(response["output"][0]["call_id"], "call_one");
    assert_eq!(response["output"][0]["arguments"], "{\"sku\":1}");
    assert_eq!(response["tool_choice"]["name"], "lookup");
    let mut followup = payload.clone();
    followup["input"]
        .as_array_mut()
        .unwrap()
        .push(response["output"][0].clone());
    let rejected = app.call(request(&followup)?).await?;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    followup["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"function_call_output","call_id":"call_one","output":"found"}));
    let result = app.call(request(&followup)?).await?;
    assert_eq!(
        result.json::<Value>()?["output"][0]["content"][0]["text"],
        "confirmed"
    );
    payload["stream"] = true.into();
    let response = app.call(request(&payload)?).await?;
    let body = std::str::from_utf8(response.streamed_content())?;
    let completed: Value = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find(|event| event["type"] == "response.completed")
        .unwrap();
    assert!(body.contains("response.function_call_arguments.delta"));
    assert_eq!(completed["response"]["output"][1]["call_id"], "call_one");
    assert_eq!(
        completed["response"]["output"][1]["arguments"],
        "{\"sku\":1}"
    );
    assert_eq!(completed["response"]["output"][2]["call_id"], "call_two");
    assert_eq!(
        completed["response"]["output"][2]["arguments"],
        "{\"sku\":2}"
    );
    let response = app.call(request(&payload)?).await?;
    let bad = std::str::from_utf8(response.streamed_content())?;
    assert!(bad.contains("event: error"));
    assert!(
        !bad.contains("response.function_call_arguments.done")
            && !bad.contains("response.completed")
    );
    relay.await?;
    Ok(())
}
