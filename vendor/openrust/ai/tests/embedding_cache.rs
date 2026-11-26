use std::{collections::BTreeMap, error::Error, fs, path::Path, time::Duration};

use openrust::{
    config::{ConfigError, Environment},
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    support::facades::Route,
    testing::TestApp,
};
use openrust_ai::{Ai, Embeddings};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn config(_: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut ai = openrust_ai::default_config(env)?;
    ai["caching"]["embeddings"]["store"] = "vectors".into();
    Ok(BTreeMap::from([
        ("ai".into(), ai),
        (
            "cache".into(),
            json!({
                "default":"wrong-store",
                "stores":{"vectors":{"driver":"array"}}
            }),
        ),
    ]))
}

async fn upstream(count: usize, suffix: f32) -> (String, JoinHandle<Vec<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..count {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /v1/embeddings HTTP/1.1");
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
            let body: Value = serde_json::from_slice(&body).unwrap();
            let values: Vec<String> = serde_json::from_value(body["input"].clone()).unwrap();
            let data: Vec<_> = values.iter().enumerate().rev().map(|(index,value)| {
                json!({"index":index,"embedding":[f32::from(value.as_bytes()[0]),suffix]})
            }).collect();
            let response =
                json!({"model":"canonical","data":data,"usage":{"prompt_tokens":values.len()}})
                    .to_string();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len(),
            );
            socket.get_mut().write_all(header.as_bytes()).await.unwrap();
            socket
                .get_mut()
                .write_all(response.as_bytes())
                .await
                .unwrap();
            requests.push(values);
        }
        requests
    });
    (url, task)
}

#[tokio::test]
async fn embeddings_cache_batches_partial_hits_and_separates_rebuilt_providers() -> TestResult {
    let (primary, requests) = upstream(5, 1.0).await;
    let (isolated, isolated_requests) = upstream(1, 2.0).await;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_config(config)
            .with_routing(Routing::default().web(|| {
                Route::get("/embedding-cache", || async {
                    let initial = Embeddings::r#for(["Alpha", "Beta"])
                        .unwrap()
                        .cache(None, None)
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(initial.usage.prompt_tokens, Some(2));
                    let partial = Embeddings::r#for(["Beta", "Gamma", "Alpha", "Beta"])
                        .unwrap()
                        .cache(None, None)
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(partial.usage.prompt_tokens, Some(1));
                    assert_eq!(
                        partial.embeddings,
                        [[66.0, 1.0], [71.0, 1.0], [65.0, 1.0], [66.0, 1.0]]
                    );
                    let full_hit = Embeddings::r#for(["Gamma", "Beta"])
                        .unwrap()
                        .cache(None, None)
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(full_hit.usage.prompt_tokens, None);
                    assert_eq!(full_hit.embeddings, [[71.0, 1.0], [66.0, 1.0]]);
                    let bypass = Embeddings::r#for(["Gamma"])
                        .unwrap()
                        .cache(Some(0), None)
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(bypass.usage.prompt_tokens, Some(1));
                    let generated = Embeddings::r#for(["Alpha", "Beta"])
                        .unwrap()
                        .cache(None, Some(false))
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(generated.usage.prompt_tokens, Some(2));
                    let cached = Embeddings::r#for(["Alpha", "Beta"])
                        .unwrap()
                        .cache(None, Some(false))
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(cached.usage.prompt_tokens, None);
                    assert_eq!(cached.model, "canonical");

                    let manager = Ai::manager().unwrap();
                    for (name, expected) in [("openai", 1.0), ("deepseek", 2.0)] {
                        let source = manager.get_instance_config(name).unwrap();
                        Ai::build(json!({
                            "name":"runtime-test","driver":"openai-compatible","url":source["url"],
                            "key_env":"OPENAI_API_KEY",
                            "models":{"embeddings":{"default":"fixture"}}
                        }))
                        .unwrap();
                        let result = Embeddings::r#for(["Alpha"])
                            .unwrap()
                            .cache(None, None)
                            .generate_with(Some("runtime-test"), None)
                            .await
                            .unwrap();
                        assert_eq!(result.embeddings, [[65.0, expected]]);
                        assert_eq!(result.usage.prompt_tokens, Some(1));
                    }
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_EMBEDDINGS_PROVIDER=openai\nOPENAI_URL={primary}\nOPENAI_API_KEY=fixture-key\nOPENAI_EMBEDDING_MODEL=fixture\nDEEPSEEK_URL={isolated}\n"
        ),
    )?;
    assert_eq!(app.get("/embedding-cache").await?.status(), StatusCode::OK);
    assert_eq!(
        requests.await?,
        [
            vec!["Alpha", "Beta"],
            vec!["Gamma"],
            vec!["Gamma"],
            vec!["Alpha", "Beta"],
            vec!["Alpha"]
        ]
    );
    assert_eq!(isolated_requests.await?, [vec!["Alpha"]]);
    Ok(())
}
