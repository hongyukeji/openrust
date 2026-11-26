use std::{error::Error, fs, sync::Arc, time::Duration};

use futures_util::StreamExt;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::configuration::Routing,
    http::{Response, StatusCode},
    support::facades::Route,
    testing::TestApp,
};
use openrust_ai::{
    Agent, Ai, AiError, AiManager, Message, PromptOptions, StreamEventData, agent,
    openrust_providers,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

async fn upstream(
    method: &'static str,
    path: &'static str,
    requests: Vec<(u16, Value)>,
) -> (String, JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/private/v1/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for (status, response) in requests {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let (mut socket, body) = read_request(socket, method, path).await;
            bodies.extend(body);
            let response = response.to_string();
            let wire = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len(),
            );
            socket.get_mut().write_all(wire.as_bytes()).await.unwrap();
        }
        bodies
    });
    (url, server)
}

async fn read_request(
    socket: TcpStream,
    method: &str,
    path: &str,
) -> (BufReader<TcpStream>, Option<Value>) {
    let mut socket = BufReader::new(socket);
    let mut line = String::new();
    socket.read_line(&mut line).await.unwrap();
    assert_eq!(line.trim_end(), format!("{method} {path} HTTP/1.1"));
    let mut length = None;
    let mut authorized = false;
    loop {
        line.clear();
        assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        let (key, value) = line.split_once(':').unwrap();
        if key.eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse().unwrap());
        }
        if key.eq_ignore_ascii_case("authorization") {
            authorized = value.trim() == "Bearer local-fixture-key";
        }
    }
    assert!(authorized);
    if let Some(length) = length.filter(|length| *length > 0) {
        let mut body = vec![0; length];
        socket.read_exact(&mut body).await.unwrap();
        return (socket, Some(serde_json::from_slice(&body).unwrap()));
    }
    (socket, None)
}

#[tokio::test]
async fn application_routes_call_both_openai_protocols() -> TestResult {
    let (chat_url, chat_server) = upstream(
        "POST",
        "/private/v1/chat/completions",
        vec![
            (
                200,
                json!({
                    "model": "resolved-chat",
                    "choices": [{"message": {"content": "你好"}}],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 5}
                }),
            ),
            (429, json!({"error":{"message":"secret service response"}})),
        ],
    )
    .await;
    let (response_url, response_server) = upstream(
        "POST",
        "/private/v1/responses",
        vec![(
            200,
            json!({
                "status": "completed",
                "model": "resolved-responses",
                "output": [
                    {"type": "reasoning",
                        "summary":[{"type":"summary_text","text":"Public summary"}],
                        "content":[{"type":"reasoning_text","text":"Private trace"}]},
                    {"type": "message", "content": [{"type": "output_text", "text": "OK",
                        "annotations":[
                            {"type":"url_citation","url":"https://example.test/source",
                                "title":"Source","start_index":0,"end_index":2},
                            {"type":"file_citation","file_id":"ignored"}
                        ]}]}
                ],
                "usage": {"input_tokens": 7, "output_tokens": 2}
            }),
        )],
    )
    .await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/chat", || async {
                    let result = agent("请使用中文")
                        .with_messages([
                            Message::user("之前的问题"),
                            Message::assistant("之前的回答"),
                        ])
                        .prompt("回应")
                        .await
                        .unwrap();
                    assert_eq!(result.provider, "deepseek");
                    assert_eq!(result.model, "resolved-chat");
                    assert_eq!(result.usage.prompt_tokens, Some(3));
                    assert_eq!(result.usage.completion_tokens, Some(5));
                    Response::json(StatusCode::OK, json!({"output": result.text}))
                })?;
                Route::get("/responses", || async {
                    let result = agent("Use English")
                        .with_messages([Message::user("Earlier"), Message::assistant("Answer")])
                        .prompt_with(
                            "Respond",
                            PromptOptions {
                                provider: Some("openai"),
                                model: Some("explicit-model"),
                                timeout: None,
                            },
                        )
                        .await
                        .unwrap();
                    assert_eq!(result.provider, "openai");
                    assert_eq!(result.model, "resolved-responses");
                    assert_eq!(result.usage.prompt_tokens, Some(7));
                    assert_eq!(result.usage.completion_tokens, Some(2));
                    assert_eq!(result.reasoning, "Public summary");
                    Response::json(
                        StatusCode::OK,
                        json!({"output": result.text, "meta": result.meta()}),
                    )
                })?;
                Route::get("/error", || async {
                    let error = agent("").prompt("Fail").await.unwrap_err();
                    assert_eq!(error, AiError::ProviderStatus(429));
                    assert!(!error.to_string().contains("secret service response"));
                    Response::json(StatusCode::OK, json!({"verified": true}))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=deepseek\nDEEPSEEK_URL={chat_url}\nDEEPSEEK_API_KEY=local-fixture-key\nDEEPSEEK_MODEL=shared-default\nOPENAI_URL={response_url}\nOPENAI_API_KEY=local-fixture-key\nOPENAI_MODEL=unused-default\n"
        ),
    )?;
    assert_eq!(
        app.get("/chat").await?.json::<Value>()?,
        json!({"output":"你好"})
    );
    assert_eq!(
        app.get("/responses").await?.json::<Value>()?,
        json!({"output":"OK","meta":{"provider":"openai","model":"resolved-responses",
            "citations":[{"url":"https://example.test/source","title":"Source","start_index":0,"end_index":2}]}})
    );
    assert_eq!(
        app.get("/error").await?.json::<Value>()?,
        json!({"verified":true})
    );
    let chat = chat_server.await?;
    let responses = response_server.await?;
    assert_eq!(chat[0]["model"], "shared-default");
    assert_eq!(
        chat[0]["messages"],
        json!([
            {"role":"system","content":"请使用中文"},
            {"role":"user","content":"之前的问题"},
            {"role":"assistant","content":"之前的回答"},
            {"role":"user","content":"回应"}
        ])
    );
    assert_eq!(
        responses[0]["input"],
        json!([
            {"type":"message","role":"user","content":"Earlier"},
            {"type":"message","role":"assistant","content":"Answer"},
            {"type":"message","role":"user","content":"Respond"}
        ])
    );
    assert_eq!(responses[0]["instructions"], "Use English");
    assert_eq!(responses[0]["store"], true);
    assert_eq!(responses[0]["model"], "explicit-model");
    Ok(())
}

struct Support;
impl Agent for Support {
    fn instructions(&self) -> &str {
        "回答问题"
    }

    fn messages(&self) -> &[Message] {
        static HISTORY: std::sync::LazyLock<Vec<Message>> = std::sync::LazyLock::new(|| {
            vec![Message::user("历史问题"), Message::assistant("历史回答")]
        });
        HISTORY.as_slice()
    }
}

#[tokio::test]
async fn fake_and_manager_are_application_local() -> TestResult {
    let configure = |builder: openrust::foundation::configuration::ApplicationBuilder| {
        builder
            .with_providers(openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/probe", || async {
                    let faked = Support::is_faked().unwrap();
                    let value = match Support.prompt("测试问题").await {
                        Ok(response) => response.text,
                        Err(AiError::DefaultNotConfigured) => "no default".into(),
                        Err(error) => panic!("{error}"),
                    };
                    Response::json(StatusCode::OK, json!({"result": value, "faked": faked}))
                })
            }))
    };
    let mut first = TestApp::create(configure)?;
    let mut second = TestApp::create(configure)?;
    first.boot().await?;
    second.boot().await?;
    let manager = first.app().container().make::<AiManager>()?;
    let fake = manager.fake::<Support>(["模拟答案".into()]);
    fake.assert_never_prompted();
    assert_eq!(
        first.get("/probe").await?.json::<Value>()?,
        json!({"result":"模拟答案","faked":true})
    );
    assert_eq!(
        second.get("/probe").await?.json::<Value>()?,
        json!({"result":"no default","faked":false})
    );
    fake.assert_prompted(|prompt| {
        prompt.text == "测试问题" && prompt.messages[0] == Message::user("历史问题")
    });
    fake.assert_not_prompted(|prompt| prompt.text == "其他问题");
    fake.assert_prompted_times(1);
    fake.prevent_stray_prompts(true);
    assert_eq!(
        manager
            .prompt(&Support, "另一个", PromptOptions::default())
            .await
            .unwrap_err(),
        AiError::FakeExhausted
    );
    let second_manager = second.app().container().make::<AiManager>()?;
    assert!(!Arc::ptr_eq(&manager, &second_manager));
    assert!(second_manager.text_fake::<Support>().is_none());
    assert!(matches!(Ai::manager(), Err(AiError::Unavailable)));
    Ok(())
}

#[tokio::test]
async fn artisan_catalog_merges_config_and_survives_missing_upstream_listing() -> TestResult {
    let (url, server) = upstream(
        "GET",
        "/private/v1/models",
        vec![
            (
                200,
                json!({
                    "data": [
                        {"id": "Local", "name": "已发现", "input_modalities": ["text"]},
                        {"id": "local"},
                        {"id": "Local"}
                    ]
                }),
            ),
            (404, json!({"error": {"message": "private path"}})),
        ],
    )
    .await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_providers())
            .with_config(|_, env| {
                Ok([(
                    "ai".into(),
                    json!({
                        "default": "test",
                        "providers": {"test": {
                            "driver": "openai-compatible",
                            "url": env.get_utf8("OPENRUST_AI_FIXTURE_URL")?,
                            "key_env": "OPENRUST_AI_FIXTURE_KEY",
                            "models": {"text": {"default": "Local", "ids": ["manual"]}}
                        }}
                    }),
                )]
                .into())
            })
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!("OPENRUST_AI_FIXTURE_KEY=local-fixture-key\nOPENRUST_AI_FIXTURE_URL={url}\n"),
    )?;
    let first = app.artisan(["ai:models", "test", "--json"]).await;
    first.assert_successful();
    let first: Value = serde_json::from_str(&first.stdout)?;
    let models = first["models"].as_array().unwrap();
    assert_eq!(models.len(), 3);
    assert_eq!(models[0]["id"], "Local");
    assert_eq!(models[0]["name"], "已发现");
    assert_eq!(models[0]["configured"], true);
    assert_eq!(models[0]["discovered"], true);
    assert_eq!(models[1]["id"], "local");
    assert_eq!(models[2]["id"], "manual");
    assert_eq!(models[2]["discovered"], false);

    let second = app.artisan(["ai:models", "test", "--json"]).await;
    second.assert_successful();
    let second: Value = serde_json::from_str(&second.stdout)?;
    assert_eq!(second["models"].as_array().unwrap().len(), 2);
    assert_eq!(second["models"][0]["id"], "Local");
    assert_eq!(second["models"][1]["id"], "manual");
    assert!(
        second["discovery_error"]
            .as_str()
            .unwrap()
            .contains("HTTP 404")
    );
    assert!(
        !second["discovery_error"]
            .as_str()
            .unwrap()
            .contains("private path")
    );
    server.await?;
    Ok(())
}

#[tokio::test]
async fn sdk_text_stream_reads_before_eof_and_keeps_upstream_usage() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/private/v1", listener.local_addr()?);
    let (received_delta, finish) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (mut socket, request) =
            read_request(socket, "POST", "/private/v1/chat/completions").await;
        let request = request.unwrap();
        assert_eq!(request["model"], "fixture-model");
        assert_eq!(request["stream"], true);
        assert_eq!(request["stream_options"]["include_usage"], true);
        socket
            .get_mut()
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let frame = "data: {\"model\":\"fixture-model\",\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\r\n\r\n";
        let split = frame.find('你').unwrap() + 1;
        let bytes = frame.as_bytes();
        socket.get_mut().write_all(&bytes[..split]).await.unwrap();
        tokio::task::yield_now().await;
        socket.get_mut().write_all(&bytes[split..]).await.unwrap();
        finish.await.unwrap();
        socket.get_mut().write_all(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n").await.unwrap();
    });
    let mut app = TestApp::create(|builder| builder.with_providers(openrust_providers()))?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=deepseek\nDEEPSEEK_URL={url}\nDEEPSEEK_API_KEY=local-fixture-key\nDEEPSEEK_MODEL=fixture-model\n"
        ),
    )?;
    app.boot().await?;
    let manager = app.app().container().make::<AiManager>()?;
    let mut stream = manager
        .stream(
            &Support,
            "验证",
            PromptOptions {
                timeout: Some(Duration::from_secs(5)),
                ..Default::default()
            },
        )
        .await?
        .into_stream();
    assert!(matches!(
        stream.next().await.transpose()?.unwrap().data,
        StreamEventData::StreamStart { .. }
    ));
    assert!(matches!(
        stream.next().await.transpose()?.unwrap().data,
        StreamEventData::TextStart { .. }
    ));
    let part = stream.next().await.transpose()?.unwrap();
    assert!(matches!(part.data, StreamEventData::TextDelta { delta, .. } if delta == "你好"));
    received_delta.send(()).unwrap();
    assert!(matches!(
        stream.next().await.transpose()?.unwrap().data,
        StreamEventData::TextEnd { .. }
    ));
    let end = stream.next().await.transpose()?.unwrap();
    assert!(
        matches!(end.data, StreamEventData::StreamEnd { reason, usage }
        if reason == "stop" && usage.prompt_tokens == Some(4) && usage.completion_tokens == Some(2))
    );
    assert!(stream.next().await.is_none());
    server.await?;
    Ok(())
}

#[tokio::test]
async fn responses_stream_is_exposed_as_laravel_sse() -> TestResult {
    use openrust::json_schema::JsonSchema;

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/private/v1", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (mut socket, request) = read_request(socket, "POST", "/private/v1/responses").await;
        let request = request.unwrap();
        assert_eq!(request["stream"], true);
        assert_eq!(request["text"]["format"]["type"], "json_schema");
        assert_eq!(
            request["text"]["format"]["schema"]["properties"]["ok"]["type"],
            "boolean"
        );
        let response = concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: {\"type\":\"response.created\",\"response\":{\"model\":\"local\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"{\\\"ok\\\":true}\"}\n\n",
            "data: {\"type\":\"response.output_text.done\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":3}}}\n\n"
        );
        socket
            .get_mut()
            .write_all(response.as_bytes())
            .await
            .unwrap();
    });
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/stream", || async {
                    let schema = JsonSchema::object([("ok", JsonSchema::boolean().into())]);
                    agent("")
                        .with_schema(schema)
                        .stream("test")
                        .await
                        .unwrap()
                        .to_response()
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=openai\nOPENAI_URL={url}\nOPENAI_API_KEY=local-fixture-key\nOPENAI_MODEL=local\n"
        ),
    )?;
    let response = app.get("/stream").await?;
    response.assert_ok();
    let body = std::str::from_utf8(response.body())?;
    let event = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .take_while(|line| *line != "[DONE]")
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(event.len(), 5);
    assert_eq!(event[2]["type"], "text_delta");
    assert_eq!(event[2]["delta"], r#"{"ok":true}"#);
    assert_eq!(event[4]["usage"]["prompt_tokens"], 2);
    assert!(body.ends_with("data: [DONE]\n\n"));
    server.await?;
    Ok(())
}

#[tokio::test]
async fn structured_prompt_sends_nested_schema_and_rejects_invalid_results() -> TestResult {
    use openrust::json_schema::JsonSchema;

    let nested = JsonSchema::object([("name", JsonSchema::string().required(true).into())])
        .without_additional_properties();
    let schema = JsonSchema::object([(
        "rows",
        JsonSchema::array().items(nested).required(true).into(),
    )])
    .without_additional_properties();
    let expected = json!({"rows":[{"name":"sample"}]});
    let (chat_url, chat_server) = upstream(
        "POST",
        "/private/v1/chat/completions",
        vec![
            (
                200,
                json!({"choices":[{"message":{"content":expected.to_string()}}]}),
            ),
            (
                200,
                json!({"choices":[{"message":{"content":"literal answer"}}]}),
            ),
            (200, json!({"choices":[{"message":{"refusal":"refused"}}]})),
        ],
    )
    .await;
    let (responses_url, response_server) = upstream("POST", "/private/v1/responses", vec![
        (200, json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":expected.to_string()}]}]})),
    ]).await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/structured", || async {
                    let nested =
                        JsonSchema::object([("name", JsonSchema::string().required(true).into())]);
                    let schema = JsonSchema::object([(
                        "rows",
                        JsonSchema::array().items(nested).required(true).into(),
                    )]);
                    let agent = agent("Return rows.").with_schema(schema).strict();
                    let good = agent.prompt("A row").await.unwrap();
                    assert_eq!(good.structured["rows"][0]["name"], "sample");
                    assert_eq!(
                        good.decode::<Value>().unwrap(),
                        json!({"rows":[{"name":"sample"}]})
                    );
                    assert_eq!(
                        agent.prompt("Malformed").await.unwrap_err(),
                        AiError::InvalidStructuredResponse
                    );
                    assert_eq!(agent.prompt("Refused").await.unwrap_err(), AiError::Refused);
                    let response = agent
                        .prompt_with(
                            "explicit provider",
                            PromptOptions {
                                provider: Some("openai"),
                                model: Some("schema-model"),
                                timeout: None,
                            },
                        )
                        .await
                        .unwrap();
                    Response::json(StatusCode::OK, response)
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_PROVIDER=deepseek\nDEEPSEEK_URL={chat_url}\nDEEPSEEK_API_KEY=local-fixture-key\nDEEPSEEK_MODEL=fixture\nOPENAI_URL={responses_url}\nOPENAI_API_KEY=local-fixture-key\n"
        ),
    )?;
    assert_eq!(app.get("/structured").await?.json::<Value>()?, expected);
    let expected_schema = openrust::json_schema::Type::from(schema).to_array()?;
    for request in chat_server.await? {
        assert_eq!(request["response_format"]["type"], "json_schema");
        assert_eq!(
            request["response_format"]["json_schema"]["schema"],
            expected_schema
        );
        assert_eq!(request["response_format"]["json_schema"]["strict"], true);
    }
    let requests = response_server.await?;
    assert_eq!(requests[0]["text"]["format"]["type"], "json_schema");
    assert_eq!(requests[0]["text"]["format"]["schema"], expected_schema);
    assert_eq!(requests[0]["text"]["format"]["strict"], true);
    Ok(())
}
