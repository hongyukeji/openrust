use std::{
    error::Error,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Response, StatusCode},
    json_schema::{JsonSchemaTypeFactory, types::ObjectType},
    support::facades::Route,
    testing::TestApp,
    validation::RuleSet,
};
use openrust_ai::{Agent, AiResult, PromptOptions, Tool, ToolRequest, agent};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

struct Lookup(AtomicUsize);

impl Tool for Lookup {
    fn name(&self) -> &str {
        "lookup"
    }

    fn description(&self) -> &str {
        "Look up a product in the local catalog"
    }

    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType {
        schema.object([("sku", schema.string().required(true).into())])
    }

    fn handle<'a>(&'a self, request: ToolRequest<'a>) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            #[derive(Deserialize)]
            struct Arguments {
                sku: String,
            }
            request
                .validate(RuleSet::parse([("sku", "required|string")]).unwrap())
                .await
                .map_err(|_| openrust_ai::AiError::InvalidToolArguments)?;
            let args = request.decode::<Arguments>()?;
            assert_eq!(args.sku, "demo");
            assert_eq!(request.tool_call_id(), "call_fixture");
            assert_eq!(request.tool_invocation_id().len(), 26);
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(json!({"sku":args.sku,"quantity":4}).to_string())
        })
    }
}

async fn fixture(path: &'static str, replies: Vec<Value>) -> (String, JoinHandle<Vec<Value>>) {
    native_fixture("v1", path, "authorization", "Bearer fixture-key", replies).await
}

async fn native_fixture(
    version: &'static str,
    path: &'static str,
    auth_header: &'static str,
    auth_value: &'static str,
    replies: Vec<Value>,
) -> (String, JoinHandle<Vec<Value>>) {
    let replies = replies
        .into_iter()
        .map(|value| ("application/json", value.to_string()))
        .collect();
    http_fixture(version, path, auth_header, auth_value, replies).await
}

async fn http_fixture(
    version: &'static str,
    path: &'static str,
    auth_header: &'static str,
    auth_value: &'static str,
    replies: Vec<(&'static str, String)>,
) -> (String, JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/{}", listener.local_addr().unwrap(), version);
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (content_type, payload) in replies {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), format!("POST /{version}/{path} HTTP/1.1"));
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
                if key.eq_ignore_ascii_case(auth_header) {
                    authorized = value.trim() == auth_value;
                }
            }
            assert!(authorized);
            let mut body = vec![0; size];
            socket.read_exact(&mut body).await.unwrap();
            requests.push(serde_json::from_slice(&body).unwrap());
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            socket.get_mut().write_all(header.as_bytes()).await.unwrap();
            socket
                .get_mut()
                .write_all(payload.as_bytes())
                .await
                .unwrap();
        }
        requests
    });
    (url, server)
}

#[tokio::test]
async fn sdk_tools_roundtrip_through_chat_and_responses() -> TestResult {
    let arguments = json!({"sku":"demo"}).to_string();
    let call = json!({
        "model":"chat-fixture","choices":[{"message":{"content":null,"tool_calls":[
            {"id":"call_fixture","type":"function","function":{"name":"lookup","arguments":arguments}}
        ]}}],"usage":{"prompt_tokens":5,"completion_tokens":1}
    });
    let (chat, chat_calls) = fixture("chat/completions", vec![
        call.clone(), json!({"choices":[{"message":{"content":"four remaining"}}],"usage":{"prompt_tokens":6,"completion_tokens":2}}),
        call,
    ]).await;
    let (responses, response_calls) = fixture("responses", vec![
        json!({"status":"completed","output":[{"type":"function_call","call_id":"call_fixture","name":"lookup","arguments":arguments}],"usage":{"input_tokens":5,"output_tokens":1}}),
        json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"four remaining"}]}],"usage":{"input_tokens":6,"output_tokens":2}}),
    ]).await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/chat", || async {
                    check_tools("deepseek").await;
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Route::get("/responses", || async {
                    check_tools("openai").await;
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Route::get("/step-limit", || async {
                    let tool = Arc::new(Lookup(AtomicUsize::new(0)));
                    let response = agent("")
                        .with_tools(vec![tool.clone()])
                        .with_max_steps(1)
                        .prompt("Check demo.")
                        .await
                        .unwrap();
                    assert_eq!(tool.0.load(Ordering::Relaxed), 0);
                    assert!(response.tool_results[0].failed);
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=deepseek\nDEEPSEEK_URL={chat}\nDEEPSEEK_MODEL=chat-fixture\nDEEPSEEK_API_KEY=fixture-key\nOPENAI_URL={responses}\nOPENAI_API_KEY=fixture-key\nOPENAI_MODEL=response-fixture\n"
        ),
    )?;
    assert_eq!(app.get("/chat").await?.status(), StatusCode::OK);
    assert_eq!(app.get("/responses").await?.status(), StatusCode::OK);
    assert_eq!(app.get("/step-limit").await?.status(), StatusCode::OK);

    let chat = chat_calls.await?;
    assert_eq!(
        chat[0]["tools"][0]["function"]["parameters"]["required"],
        json!(["sku"])
    );
    assert_eq!(
        chat[1]["messages"][1]["tool_calls"][0]["id"],
        "call_fixture"
    );
    let result = &chat[1]["messages"][2];
    assert_eq!(result["tool_call_id"], "call_fixture");
    assert_eq!(
        serde_json::from_str::<Value>(result["content"].as_str().unwrap())?["quantity"],
        4
    );
    let responses = response_calls.await?;
    assert_eq!(responses[0]["tools"][0]["name"], "lookup");
    assert_eq!(responses[1]["input"][1]["call_id"], "call_fixture");
    assert_eq!(responses[1]["input"][2]["type"], "function_call_output");
    assert_eq!(responses[1]["input"][2]["call_id"], "call_fixture");
    Ok(())
}

async fn check_tools(provider: &str) {
    let tool = Arc::new(Lookup(AtomicUsize::new(0)));
    let response = agent("")
        .with_tools(vec![tool.clone()])
        .prompt_with(
            "Check demo.",
            PromptOptions {
                provider: Some(provider),
                ..PromptOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(tool.0.load(Ordering::Relaxed), 1);
    assert_eq!(response.text, "four remaining");
    assert_eq!(response.tool_calls[0].id, response.tool_results[0].id);
    assert!(!response.tool_results[0].failed);
    assert_eq!(response.usage.prompt_tokens, Some(11));
    assert_eq!(response.usage.completion_tokens, Some(3));
}

#[tokio::test]
async fn sdk_tools_roundtrip_through_anthropic_and_gemini() -> TestResult {
    let thinking =
        json!({"type":"thinking","thinking":"fixture","signature":"opaque-claude-signature"});
    let gemini_call = json!({
        "functionCall":{"id":"call_fixture","name":"lookup","args":{"sku":"demo"}},
        "thoughtSignature":"opaque-gemini-signature",
    });
    let gemini_replies = vec![
        json!({"candidates":[{"content":{"role":"model","parts":[gemini_call]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":1}}),
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":"four remaining"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":6,"candidatesTokenCount":2}}),
    ];
    let (anthropic, anthropic_calls) = native_fixture("v1", "messages", "x-api-key", "fixture-key", vec![
        json!({"stop_reason":"tool_use","content":[thinking,{"type":"tool_use","id":"call_fixture","name":"lookup","input":{"sku":"demo"}}],"usage":{"input_tokens":5,"output_tokens":1}}),
        json!({"stop_reason":"end_turn","content":[{"type":"text","text":"four remaining"}],"usage":{"input_tokens":6,"output_tokens":2}}),
    ]).await;
    let (gemini, gemini_calls) = native_fixture(
        "v1beta",
        "models/gemini-fixture:generateContent",
        "x-goog-api-key",
        "fixture-key",
        gemini_replies,
    )
    .await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/claude", || async {
                    check_tools("anthropic").await;
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Route::get("/gemini", || async {
                    check_tools("gemini").await;
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "ANTHROPIC_URL={anthropic}\nANTHROPIC_MODEL=claude-fixture\nANTHROPIC_API_KEY=fixture-key\nGEMINI_URL={gemini}\nGEMINI_API_KEY=fixture-key\nGEMINI_MODEL=gemini-fixture\n"
        ),
    )?;
    assert_eq!(app.get("/claude").await?.status(), StatusCode::OK);
    assert_eq!(app.get("/gemini").await?.status(), StatusCode::OK);
    let claude = anthropic_calls.await?;
    assert_eq!(
        claude[0]["tools"][0]["input_schema"]["required"],
        json!(["sku"])
    );
    assert_eq!(claude[1]["messages"][1]["content"][0], thinking);
    let result = &claude[1]["messages"][2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "call_fixture");
    assert_eq!(result["is_error"], false);
    assert_eq!(
        serde_json::from_str::<Value>(result["content"].as_str().unwrap())?["quantity"],
        4
    );

    let gemini = gemini_calls.await?;
    assert_eq!(
        gemini[0]["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["required"],
        json!(["sku"])
    );
    assert_eq!(gemini[1]["contents"][1]["parts"][0], gemini_call);
    let result = &gemini[1]["contents"][2]["parts"][0]["functionResponse"];
    assert_eq!(result["name"], "lookup");
    assert_eq!(result["id"], "call_fixture");
    assert_eq!(
        serde_json::from_str::<Value>(result["response"]["output"].as_str().unwrap())?["quantity"],
        4
    );
    Ok(())
}

#[tokio::test]
async fn agent_middleware_controls_each_step_without_replaying_disabled_tools() -> TestResult {
    use openrust_ai::{AgentMiddleware, NextStep, PendingStep, StepResponse};
    use std::sync::Mutex;

    #[derive(Default)]
    struct RestrictLookup(Mutex<Vec<(usize, String)>>);

    impl AgentMiddleware for RestrictLookup {
        fn handle<'a, 'step: 'a>(
            &'a self,
            step: PendingStep<'step>,
            next: NextStep<'a, 'step>,
        ) -> HttpFuture<'a, AiResult<StepResponse>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .unwrap()
                    .push((step.number, step.invocation_id.into()));
                let step = match step.number {
                    0 => {
                        assert!(step.is_first_step());
                        assert!(step.steps.is_empty());
                        assert_eq!(step.tools()[0].name, "lookup");
                        step.with_model("first-step")
                            .with_instructions("Inspect the catalog")
                    }
                    1 => {
                        assert_eq!(step.usage.prompt_tokens, Some(2));
                        assert!(!step.steps[0].results[0].failed);
                        step.without_tools(&["lookup"])
                    }
                    2 => {
                        assert!(step.is_final_step);
                        assert_eq!(step.usage.prompt_tokens, Some(4));
                        assert!(step.steps[1].results[0].failed);
                        step.only_tools(&[])
                    }
                    _ => panic!("unexpected step"),
                };
                let mut response = next.run(step).await?;
                if let Some(text) = &mut response.text {
                    text.push_str(" (checked)");
                }
                Ok(response)
            })
        }
    }

    let tool_response = |id| {
        json!({
            "choices":[{"message":{"content":null,"tool_calls":[{
                "id":id,"type":"function","function":{"name":"lookup","arguments":"{\"sku\":\"demo\"}"}
            }]}}],
            "usage":{"prompt_tokens":2,"completion_tokens":1}
        })
    };
    let (url, calls) = fixture("chat/completions", vec![
        tool_response("call_fixture"),
        tool_response("should_not_run"),
        json!({"choices":[{"message":{"content":"available"}}],"usage":{"prompt_tokens":3,"completion_tokens":1}}),
    ]).await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/middleware", || async {
                    let tool = Arc::new(Lookup(AtomicUsize::new(0)));
                    let middleware = Arc::new(RestrictLookup::default());
                    let agent = agent("")
                        .with_tools(vec![tool.clone()])
                        .with_middleware(vec![middleware.clone()])
                        .with_max_steps(3);
                    let response = agent.prompt("Check demo.").await.unwrap();
                    assert_eq!(response.text, "available (checked)");
                    assert_eq!(response.tool_results[1].id, "should_not_run");
                    assert!(response.tool_results[1].failed);
                    assert_eq!(tool.0.load(Ordering::Relaxed), 1);
                    assert!(matches!(
                        agent.stream("Check demo.").await,
                        Err(openrust_ai::AiError::StreamingMiddlewareNotSupported)
                    ));
                    let steps = middleware.0.lock().unwrap();
                    assert_eq!(
                        steps.iter().map(|(step, _)| *step).collect::<Vec<_>>(),
                        [0, 1, 2]
                    );
                    assert!(steps.iter().all(|(_, id)| *id == steps[0].1));
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=deepseek\nDEEPSEEK_API_KEY=fixture-key\nDEEPSEEK_MODEL=default-model\nDEEPSEEK_URL={url}\n"
        ),
    )?;
    assert_eq!(app.get("/middleware").await?.status(), StatusCode::OK);
    let requests = calls.await?;
    assert_eq!(requests[0]["model"], "first-step");
    assert_eq!(requests[0]["messages"][0]["content"], "Inspect the catalog");
    assert_eq!(requests[1]["model"], "default-model");
    assert_eq!(requests[1]["tools"], json!([]));
    assert_eq!(
        requests[2]["messages"].as_array().unwrap().last().unwrap()["tool_call_id"],
        "should_not_run"
    );
    Ok(())
}

#[tokio::test]
async fn sdk_openai_streams_execute_tools_and_continue_with_same_call() -> TestResult {
    let events = |values: Vec<Value>, chat: bool| {
        let mut text: String = values
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect();
        if chat {
            text.push_str("data: [DONE]\n\n");
        }
        ("text/event-stream", text)
    };
    let chat = vec![
        events(
            vec![
                json!({"choices":[{"delta":{"content":"Checking "},"finish_reason":null}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_fixture","type":"function","function":{"name":"lookup","arguments":"{\"sku\":"}}]},"finish_reason":null}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"demo\"}"}}]},"finish_reason":"tool_calls"}]}),
                json!({"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":1}}),
            ],
            true,
        ),
        events(
            vec![
                json!({"choices":[{"delta":{"content":"four remaining"},"finish_reason":"stop"}]}),
                json!({"choices":[],"usage":{"prompt_tokens":6,"completion_tokens":2}}),
            ],
            true,
        ),
        // 具备完整参数但缺少终止帧，应在执行工具前终止。
        events(
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_fixture","type":"function","function":{"name":"lookup","arguments":"{\"sku\":\"demo\"}"}}]},"finish_reason":"tool_calls"}]}),
            ],
            false,
        ),
    ];
    let responses = vec![
        events(
            vec![
                json!({"type":"response.output_text.delta","delta":"Checking "}),
                json!({"type":"response.completed","response":{"status":"completed","output":[
                {"type":"message","content":[{"type":"output_text","text":"Checking "}]},
                {"type":"function_call","call_id":"call_fixture","name":"lookup","arguments":"{\"sku\":\"demo\"}"}
            ],"usage":{"input_tokens":5,"output_tokens":1}}}),
            ],
            false,
        ),
        events(
            vec![
                json!({"type":"response.output_text.delta","delta":"four remaining"}),
                json!({"type":"response.completed","response":{"status":"completed","output":[
                {"type":"message","content":[{"type":"output_text","text":"four remaining"}]}
            ],"usage":{"input_tokens":6,"output_tokens":2}}}),
            ],
            false,
        ),
    ];
    let (chat, chat_calls) = http_fixture(
        "v1",
        "chat/completions",
        "authorization",
        "Bearer fixture-key",
        chat,
    )
    .await;
    let (responses, response_calls) = http_fixture(
        "v1",
        "responses",
        "authorization",
        "Bearer fixture-key",
        responses,
    )
    .await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/chat-stream-tools", || async {
                    let tool = Arc::new(Lookup(AtomicUsize::new(0)));
                    let response = agent("")
                        .with_tools(vec![tool])
                        .stream("Check demo.")
                        .await
                        .unwrap();
                    Ok::<_, HttpError>(response.using_vercel_data_protocol(None).to_response())
                })?;
                Route::get("/responses-stream-tools", || async {
                    let tool = Arc::new(Lookup(AtomicUsize::new(0)));
                    let response = agent("")
                        .with_tools(vec![tool])
                        .stream_with(
                            "Check demo.",
                            PromptOptions {
                                provider: Some("openai"),
                                ..PromptOptions::default()
                            },
                        )
                        .await
                        .unwrap();
                    Ok::<_, HttpError>(
                        response
                            .using_agent_user_interaction_protocol(None, None)
                            .to_response(),
                    )
                })?;
                Route::get("/interrupted-tool-stream", || async {
                    use futures_util::StreamExt;
                    let tool = Arc::new(Lookup(AtomicUsize::new(0)));
                    let mut response = agent("")
                        .with_tools(vec![tool.clone()])
                        .stream("Check demo.")
                        .await
                        .unwrap()
                        .into_stream();
                    assert!(response.next().await.unwrap().is_ok());
                    assert!(matches!(
                        response.next().await,
                        Some(Err(openrust_ai::AiError::InvalidResponse))
                    ));
                    assert_eq!(tool.0.load(Ordering::Relaxed), 0);
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=deepseek\nDEEPSEEK_URL={chat}\nDEEPSEEK_MODEL=chat\nDEEPSEEK_API_KEY=fixture-key\nOPENAI_URL={responses}\nOPENAI_MODEL=response\nOPENAI_API_KEY=fixture-key\n"
        ),
    )?;
    let chat = app.get("/chat-stream-tools").await?;
    assert_eq!(chat.status(), StatusCode::OK);
    let chat_body = std::str::from_utf8(chat.streamed_content())?;
    assert!(chat_body.contains("\"type\":\"tool-input-available\""));
    assert!(chat_body.contains("\"type\":\"tool-output-available\""));
    assert!(chat_body.contains("four remaining") && chat_body.contains("\"totalTokens\":14"));
    let responses = app.get("/responses-stream-tools").await?;
    assert_eq!(responses.status(), StatusCode::OK);
    let response_body = std::str::from_utf8(responses.streamed_content())?;
    assert!(
        response_body.contains("TOOL_CALL_START") && response_body.contains("TOOL_CALL_RESULT")
    );
    assert!(
        response_body.contains("four remaining") && response_body.contains("\"totalTokens\":14")
    );
    assert_eq!(
        app.get("/interrupted-tool-stream").await?.status(),
        StatusCode::OK
    );
    let chat = chat_calls.await?;
    assert_eq!(chat[0]["stream_options"]["include_usage"], true);
    assert_eq!(chat[1]["messages"][2]["tool_call_id"], "call_fixture");
    assert_eq!(
        serde_json::from_str::<Value>(chat[1]["messages"][2]["content"].as_str().unwrap())?["quantity"],
        4
    );
    let responses = response_calls.await?;
    assert_eq!(responses[1]["input"][2]["call_id"], "call_fixture");
    assert_eq!(responses[1]["input"][3]["call_id"], "call_fixture");
    assert_eq!(responses[1]["input"][3]["type"], "function_call_output");
    Ok(())
}
