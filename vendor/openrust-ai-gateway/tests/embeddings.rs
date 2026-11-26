use std::{collections::BTreeMap, error::Error, fs, path::Path, time::Duration};

use openrust::{
    config::{ConfigError, Environment},
    foundation::configuration::Routing,
    http::{Body, Response, StatusCode},
    support::facades::Route,
    testing::{TestApp, TestResponse},
};
use openrust_ai::{Ai, AiError, Embeddings};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn config(_: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut gateway = openrust_ai_gateway::default_config(env)?;
    gateway["models"] =
        json!({"vector":{"provider":"openai","model":"internal-embed","endpoints":["embeddings"]}});
    gateway["keys"] = json!({"caller":{"key_env":"CALLER_KEY","models":["vector"]}});
    Ok(BTreeMap::from([("ai_gateway".into(), gateway)]))
}

async fn client(
    app: &mut TestApp,
    path: &str,
    data: Value,
) -> Result<TestResponse, Box<dyn Error + Send + Sync>> {
    app.call(
        http::Request::post(path)
            .header("authorization", "Bearer downstream-embed")
            .header("content-type", "application/json")
            .body(Body::from(data.to_string()))?,
    )
    .await
}

#[tokio::test]
async fn sdk_and_gateway_embeddings_validate_and_preserve_input_order() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/provider/v1", listener.local_addr()?);
    let fixture = tokio::spawn(async move {
        let ready = json!({
            "object":"list", "model":"internal-embed",
            "data":[{"index":1,"embedding":[0.5,0.25]}, {"index":0,"embedding":[0.25,0.5]}],
            "usage":{"prompt_tokens":9,"total_tokens":9},
        });
        let malformed = json!({
            "model":"internal-embed", "data":[
                {"index":0,"embedding":[0.5,0.25]}, {"index":0,"embedding":[0.25,0.5]}
            ],
        });
        for (request_number, response) in [ready.clone(), malformed, ready].into_iter().enumerate()
        {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /provider/v1/embeddings HTTP/1.1");
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
            assert_eq!(headers["authorization"], "Bearer upstream-embed");
            let mut body = vec![0; headers["content-length"].parse().unwrap()];
            socket.read_exact(&mut body).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["model"], "internal-embed");
            assert_eq!(body["input"], json!(["  first  ", "second"]));
            assert_eq!(body["dimensions"], 2);
            if request_number < 2 {
                assert_eq!(body["encoding_format"], "float");
            }
            let payload = response.to_string();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            socket
                .get_mut()
                .write_all(headers.as_bytes())
                .await
                .unwrap();
            socket
                .get_mut()
                .write_all(payload.as_bytes())
                .await
                .unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        let mut providers = openrust_ai::openrust_providers();
        providers.extend(openrust_ai_gateway::openrust_providers());
        builder
            .with_providers(providers)
            .with_config(config)
            .with_routing(Routing::default().web(|| {
                Route::get("/sdk-embeddings", || async {
                    assert!(matches!(
                        Ai::embedding_provider(Some("deepseek")),
                        Err(AiError::EmbeddingsNotSupported)
                    ));
                    assert!(matches!(
                        Embeddings::r#for(["   "]),
                        Err(AiError::InvalidPrompt)
                    ));
                    assert!(matches!(
                        Embeddings::r#for(["valid"])
                            .unwrap()
                            .dimensions(0)
                            .generate()
                            .await,
                        Err(AiError::InvalidConfiguration(_))
                    ));
                    let inputs = ["  first  ", "second"];
                    let response = Embeddings::r#for(inputs)
                        .unwrap()
                        .dimensions(2)
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(response.first(), Some([0.25, 0.5].as_slice()));
                    assert_eq!(response.embeddings[1], [0.5, 0.25]);
                    assert_eq!(response.usage.prompt_tokens, Some(9));
                    assert_eq!(response.usage.completion_tokens, None);
                    let invalid = Embeddings::r#for(inputs)
                        .unwrap()
                        .dimensions(2)
                        .generate_with(Some("openai"), Some("internal-embed"))
                        .await
                        .unwrap_err();
                    assert_eq!(invalid, AiError::InvalidResponse);
                    Ok::<_, openrust::http::HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_GATEWAY_ENABLED=true\nAI_EMBEDDINGS_PROVIDER=openai\nOPENAI_URL={url}\nOPENAI_EMBEDDING_MODEL=internal-embed\nOPENAI_API_KEY=upstream-embed\nCALLER_KEY=downstream-embed\n"
        ),
    )?;
    assert_eq!(app.get("/sdk-embeddings").await?.status(), StatusCode::OK);
    let denied = client(
        &mut app,
        "/v1/chat/completions",
        json!({"model":"vector","messages":[]}),
    )
    .await?;
    assert_eq!(denied.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        denied.json::<Value>()?["error"]["code"],
        "unsupported_endpoint"
    );
    let response = client(
        &mut app,
        "/v1/embeddings",
        json!({"model":"vector","input":["  first  ","second"],"dimensions":2}),
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let result = response.json::<Value>()?;
    assert_eq!(result["model"], "vector");
    assert_eq!(result["usage"]["prompt_tokens"], 9);
    assert_eq!(result["data"][0]["index"], 1);
    fixture.await?;
    Ok(())
}
