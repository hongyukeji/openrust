use std::{error::Error, fs, time::Duration};

use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    support::facades::Route,
    testing::TestApp,
};
use openrust_ai::{AiError, Reranking};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn native_reranking_drivers_preserve_original_indexes() -> TestResult {
    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", upstream.local_addr()?);
    let hits = tokio::spawn(async move {
        let expected = [
            (
                "cohere/v2",
                "top_n",
                "results",
                "cohere-key",
                "cohere-default",
                false,
            ),
            ("jina/v1", "top_n", "results", "jina-key", "selected", false),
            (
                "voyage/v1",
                "top_k",
                "data",
                "voyage-key",
                "selected",
                false,
            ),
            (
                "router/api/v1",
                "top_n",
                "results",
                "router-key",
                "selected",
                false,
            ),
            (
                "cohere/v2",
                "top_n",
                "results",
                "cohere-key",
                "cohere-default",
                true,
            ),
        ];
        for (path, limit, field, token, model, duplicate) in expected {
            let (socket, _) = timeout(Duration::from_secs(5), upstream.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), format!("POST /{path}/rerank HTTP/1.1"));
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
                    authorized = value.trim() == format!("Bearer {token}");
                }
            }
            assert!(authorized);
            let mut bytes = vec![0; size];
            socket.read_exact(&mut bytes).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["model"], model);
            assert_eq!(body["query"], "How do I return an item?");
            assert_eq!(body[limit], 2);
            assert_eq!(
                body["documents"],
                json!(["Shipping", "Exchanges", "Returns"])
            );

            let mut response = if path == "cohere/v2" {
                json!({"meta":{"billed_units":{"input_tokens":17,"search_units":1}}})
            } else {
                json!({"model":"actual","usage":{"total_tokens":18}})
            };
            response[field] = json!([
                {"index":2,"relevance_score":0.92},
                {"index":if duplicate {2} else {0},"relevance_score":0.31},
            ]);
            let payload = response.to_string();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len(),
            );
            socket.get_mut().write_all(head.as_bytes()).await.unwrap();
            socket
                .get_mut()
                .write_all(payload.as_bytes())
                .await
                .unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/rerank", || async {
                    let documents = ["Shipping", "Exchanges", "Returns"];
                    let query = "How do I return an item?";
                    assert!(Reranking::of(["   "]).is_err());
                    assert!(matches!(
                        Reranking::of(documents)
                            .unwrap()
                            .limit(0)
                            .rerank(query)
                            .await,
                        Err(AiError::InvalidConfiguration(_))
                    ));
                    let first = Reranking::of(documents)
                        .unwrap()
                        .limit(2)
                        .rerank(query)
                        .await
                        .unwrap();
                    assert_eq!(first.provider, "cohere");
                    assert_eq!(first.model, "cohere-default");
                    assert_eq!(first.usage.prompt_tokens, Some(17));
                    assert_eq!(documents[first.results[0].index], "Returns");
                    for provider in ["jina", "voyageai", "openrouter"] {
                        let reranked = Reranking::of(documents)
                            .unwrap()
                            .limit(2)
                            .rerank_with(query, Some(provider), Some("selected"))
                            .await
                            .unwrap();
                        assert_eq!(reranked.provider, provider);
                        assert_eq!(reranked.model, "actual");
                        assert_eq!(
                            reranked
                                .results
                                .iter()
                                .map(|item| item.index)
                                .collect::<Vec<_>>(),
                            [2, 0]
                        );
                        assert_eq!(reranked.usage.prompt_tokens, Some(18));
                    }
                    assert_eq!(
                        Reranking::of(documents)
                            .unwrap()
                            .limit(2)
                            .rerank(query)
                            .await,
                        Err(AiError::InvalidResponse)
                    );
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_RERANKING_PROVIDER=cohere\nCOHERE_URL={url}/cohere/v2\nCOHERE_API_KEY=cohere-key\nCOHERE_RERANK_MODEL=cohere-default\nJINA_URL={url}/jina/v1\nJINA_API_KEY=jina-key\nVOYAGE_URL={url}/voyage/v1\nVOYAGE_API_KEY=voyage-key\nOPENROUTER_URL={url}/router/api/v1\nOPENROUTER_API_KEY=router-key\n"
        ),
    )?;
    assert_eq!(app.get("/rerank").await?.status(), StatusCode::OK);
    hits.await?;
    Ok(())
}
