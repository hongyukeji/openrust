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
async fn chat_clients_use_messages_with_tools_and_streaming_usage()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let address = upstream.local_addr()?;
    let server = tokio::spawn(async move {
        for stage in ["buffered", "history", "stream", "truncated"] {
            let (socket, _) = timeout(Duration::from_secs(5), upstream.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /v1/messages HTTP/1.1");
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
                if key.eq_ignore_ascii_case("x-api-key") {
                    authorized = value.trim() == "claude-key";
                }
            }
            assert!(authorized);
            let mut bytes = vec![0; size];
            socket.read_exact(&mut bytes).await.unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["model"], "claude-model");
            assert_eq!(request["system"], "Concise");
            assert_eq!(request["max_tokens"], 80);
            assert_eq!(request["stop_sequences"], json!(["done"]));
            assert_eq!(request["tools"][0]["input_schema"]["type"], "object");
            assert_eq!(
                request["tool_choice"],
                json!({"type":"tool","name":"lookup","disable_parallel_tool_use":true})
            );
            let tool =
                json!({"type":"tool_use","id":"toolu_fixture","name":"lookup","input":{"sku":1}});
            let usage = json!({"input_tokens":12,"output_tokens":5,"cache_read_input_tokens":2});
            let (kind, data) = match stage {
                "buffered" => (
                    "application/json",
                    json!({"stop_reason":"tool_use","content":[tool],"usage":usage}).to_string(),
                ),
                "history" => {
                    assert_eq!(request["messages"][1]["content"][0], tool);
                    assert_eq!(
                        request["messages"][2]["content"][0],
                        json!({"type":"tool_result","tool_use_id":"toolu_fixture","content":"found"})
                    );
                    ("application/json",json!({"stop_reason":"end_turn","content":[{"type":"text","text":"confirmed"}],"usage":usage}).to_string())
                }
                _ => {
                    let mut frames = vec![
                        json!({"type":"message_start","message":{"usage":{"input_tokens":12,"cache_read_input_tokens":2}}}),
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Checking"}}),
                        json!({"type":"content_block_stop","index":0}),
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_fixture","name":"lookup","input":{}}}),
                        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"sku\":"}}),
                    ];
                    if stage == "stream" {
                        frames.extend([
                            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"1}"}}),
                            json!({"type":"content_block_stop","index":1}),
                            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}),
                            json!({"type":"message_stop"}),
                        ]);
                    }
                    (
                        "text/event-stream",
                        frames
                            .into_iter()
                            .map(|frame| format!("data: {frame}\n\n"))
                            .collect(),
                    )
                }
            };
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                data.len()
            );
            socket
                .get_mut()
                .write_all(headers.as_bytes())
                .await
                .unwrap();
            socket.get_mut().write_all(data.as_bytes()).await.unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder.with_providers(providers).with_config(|_, env| {
            let mut gateway = openrust_ai_gateway::default_config(env)?;
            gateway["models"] = json!({"public":{
                "provider":"anthropic","model":"claude-model","endpoints":["chat/completions"],
            }});
            gateway["keys"] = json!({"caller":{"key_env":"CALLER_KEY","models":["public"]}});
            Ok([("ai_gateway".into(), gateway)].into())
        })
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nANTHROPIC_URL=http://{address}/v1\nANTHROPIC_API_KEY=claude-key\nCALLER_KEY=downstream-key\n"
        ),
    )?;
    let mut body = json!({
        "model":"public","messages":[{"role":"developer","content":"Concise"},{"role":"user","content":[{"type":"text","text":"Look up the item"}]}],
        "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}],
        "tool_choice":{"type":"function","function":{"name":"lookup"}},"parallel_tool_calls":false,
        "max_completion_tokens":80,"stop":"done",
    });
    let request = |value: &Value| {
        http::Request::post("/v1/chat/completions")
            .header("authorization", "Bearer downstream-key")
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
    };

    let response = app.call(request(&body)?).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let result = response.json::<Value>()?;
    assert_eq!(result["model"], "public");
    assert_eq!(result["object"], "chat.completion");
    assert_eq!(result["usage"]["prompt_tokens"], 14);
    assert_eq!(result["usage"]["prompt_tokens_details"]["cached_tokens"], 2);
    assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
    let assistant = result["choices"][0]["message"].clone();
    assert_eq!(assistant["tool_calls"][0]["id"], "toolu_fixture");
    assert_eq!(
        assistant["tool_calls"][0]["function"]["arguments"],
        "{\"sku\":1}"
    );

    let mut continuation = body.clone();
    let messages = continuation["messages"].as_array_mut().unwrap();
    messages.push(assistant);
    messages.push(json!({"role":"tool","tool_call_id":"toolu_fixture","content":"found"}));
    let response = app.call(request(&continuation)?).await?;
    assert_eq!(
        response.json::<Value>()?["choices"][0]["message"]["content"],
        "confirmed"
    );

    body["stream"] = true.into();
    body["stream_options"] = json!({"include_usage":true});
    let response = app.call(request(&body)?).await?;
    let sse = std::str::from_utf8(response.streamed_content())?;
    assert!(sse.contains("\"content\":\"Checking\""));
    assert!(sse.contains("\"id\":\"toolu_fixture\""));
    assert!(
        sse.contains("\"finish_reason\":\"tool_calls\"") && sse.contains("\"total_tokens\":19")
    );
    assert!(sse.ends_with("data: [DONE]\n\n"));
    let response = app.call(request(&body)?).await?;
    let truncated = std::str::from_utf8(response.streamed_content())?;
    assert!(truncated.contains("event: error") && !truncated.contains("[DONE]"));
    server.await?;
    Ok(())
}
