use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    extract::{Request, State},
    response::IntoResponse,
    routing::post,
};
use openrust::{
    database::{
        DatabaseError, Value,
        query::embeddings::{Embeddings, HttpEmbeddingProvider},
    },
    testing::TestApp,
};
use serde_json::{Value as JsonValue, json};

fn postgres_application() -> TestApp {
    TestApp::create(|app| {
        app.withConfig(|_, _| {
            Ok([
                ("app".into(), json!({"env":"testing"})),
                (
                    "database".into(),
                    json!({"default":"selected","connections":{"selected":{"driver":"pgsql"}}}),
                ),
                (
                    "cache".into(),
                    json!({"default":"array","stores":{"array":{"driver":"array"}}}),
                ),
            ]
            .into())
        })
    })
    .unwrap()
}

#[tokio::test]
async fn text_vectors_reuse_http_embeddings_and_keep_application_caches_separate() {
    async fn embed(State(calls): State<Arc<AtomicUsize>>, request: Request) -> impl IntoResponse {
        let (parts, body) = request.into_parts();
        assert_eq!(parts.headers["authorization"], "Bearer fixture");
        let body = axum::body::to_bytes(body, 65536).await.unwrap();
        let request: JsonValue = serde_json::from_slice(&body).unwrap();
        assert!(matches!(
            request["model"].as_str(),
            Some("test-model" | "alternate-model")
        ));
        assert_eq!(request["dimensions"], 2);
        assert_eq!(request["encoding_format"], "float");
        let previous = calls.fetch_add(1, Ordering::Relaxed);
        if request["input"] == "retry" && previous == 2 {
            return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "{}".to_owned());
        }
        let vector = match (request["input"].as_str(), request["model"].as_str()) {
            (Some("short"), _) => json!([1.0]),
            (_, Some("alternate-model")) => json!([0.0, 1.0]),
            _ => json!([1.0, 0.0]),
        };
        (
            axum::http::StatusCode::OK,
            json!({"data":[{"index":0,"embedding":vector}]}).to_string(),
        )
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let service = Router::new()
        .route("/v1/embeddings", post(embed))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/embeddings", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let provider = |model: &str| {
        Arc::new(Embeddings::new(
            HttpEmbeddingProvider::new(&endpoint, model, "fixture", Some(2)).unwrap(),
        ))
    };
    let mut first = postgres_application();
    let mut second = postgres_application();
    first.instance(provider("test-model")).await.unwrap();
    second.instance(provider("test-model")).await.unwrap();
    let pg = first.database().await.unwrap();
    let query = pg
        .table("documents")
        .selectVectorDistanceText("embedding", "shared text", Some("distance"))
        .await
        .unwrap()
        .whereVectorSimilarToText("embedding", "shared text", 0.5, true)
        .await
        .unwrap();
    assert_eq!(
        query.getBindings().unwrap(),
        [
            "[1.0,0.0]".into(),
            "[1.0,0.0]".into(),
            Value::Real(0.5),
            "[1.0,0.0]".into()
        ]
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    second
        .database()
        .await
        .unwrap()
        .table("documents")
        .orderByVectorDistanceText("embedding", "shared text")
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(matches!(
        pg.table("documents")
            .whereVectorDistanceLessThanText("embedding", "retry", 0.3)
            .await,
        Err(DatabaseError::Embedding(_))
    ));
    pg.table("documents")
        .orWhereVectorDistanceLessThanText("embedding", "retry", 0.3)
        .await
        .unwrap();
    pg.table("documents")
        .orderByVectorDistanceText("embedding", "retry")
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    for _ in 0..2 {
        assert!(matches!(
            pg.table("documents")
                .orderByVectorDistanceText("embedding", "short")
                .await,
            Err(DatabaseError::InvalidQuery("embedding response dimensions"))
        ));
    }
    assert_eq!(calls.load(Ordering::Relaxed), 6);

    first.instance(provider("alternate-model")).await.unwrap();
    let new_model = pg
        .table("documents")
        .orderByVectorDistanceText("embedding", "shared text")
        .await
        .unwrap();
    assert_eq!(new_model.getBindings().unwrap(), ["[0.0,1.0]".into()]);
    assert_eq!(calls.load(Ordering::Relaxed), 7);
    server.abort();
}

#[tokio::test]
async fn text_vectors_report_missing_providers_and_reject_unsupported_drivers() {
    let mut pg = postgres_application();
    let mut sqlite = TestApp::sqlite(|builder| builder).unwrap();
    assert!(matches!(
        pg.database()
            .await
            .unwrap()
            .table("documents")
            .whereVectorSimilarToText("embedding", "text", 0.5, true)
            .await,
        Err(DatabaseError::Embedding(_))
    ));
    assert!(matches!(
        sqlite
            .database()
            .await
            .unwrap()
            .table("documents")
            .whereVectorSimilarToText("embedding", "text", 0.5, true)
            .await,
        Err(DatabaseError::UnsupportedOperation(
            "vector distance queries"
        ))
    ));
}
