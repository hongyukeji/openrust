use std::{error::Error, fs, sync::Arc, time::Duration};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::configuration::Routing,
    http::{Response, StatusCode},
    support::facades::Route,
    testing::TestApp,
};
use openrust_ai::{Agent, Ai, AiError, AiManager, PromptOptions, agent, openrust_providers};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
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
                bodies.push(serde_json::from_slice(&body).unwrap());
            }
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
                    {"type": "reasoning"},
                    {"type": "message", "content": [{"type": "output_text", "text": "OK"}]}
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
                    let result = agent("请使用中文").prompt("回应").await.unwrap();
                    assert_eq!(result.provider, "deepseek");
                    assert_eq!(result.model, "resolved-chat");
                    assert_eq!(result.usage.prompt_tokens, Some(3));
                    assert_eq!(result.usage.completion_tokens, Some(5));
                    Response::json(StatusCode::OK, json!({"output": result.text}))
                })?;
                Route::get("/responses", || async {
                    let result = agent("Use English")
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
                    Response::json(StatusCode::OK, json!({"output": result.text}))
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
        json!({"output":"OK"})
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
            {"role":"user","content":"回应"}
        ])
    );
    assert_eq!(responses[0]["input"], "Respond");
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
    fake.assert_prompted(|prompt| prompt.text == "测试问题");
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
