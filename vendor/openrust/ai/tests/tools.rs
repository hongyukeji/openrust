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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in replies {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), format!("POST /v1/{path} HTTP/1.1"));
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
                    authorized = value.trim() == "Bearer fixture-key";
                }
            }
            assert!(authorized);
            let mut body = vec![0; size];
            socket.read_exact(&mut body).await.unwrap();
            requests.push(serde_json::from_slice(&body).unwrap());
            let payload = response.to_string();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
