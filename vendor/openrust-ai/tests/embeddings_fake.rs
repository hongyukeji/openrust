use std::{error::Error, time::Duration};

use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    support::facades::Route,
    testing::TestApp,
};
use openrust_ai::{AiError, Embeddings};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn application() -> Result<TestApp, Box<dyn Error + Send + Sync>> {
    TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/has-fake", || async {
                    Response::json(
                        StatusCode::OK,
                        json!({"faked":Embeddings::is_faked().unwrap()}),
                    )
                })?;
                Route::get("/fake", || async {
                    let sequence =
                        Embeddings::fake([vec![vec![0.1, 0.2], vec![0.3, 0.4]]]).unwrap();
                    sequence.prevent_stray_embeddings(true);
                    Embeddings::assert_nothing_generated();
                    let vectors = Embeddings::r#for(["first", "second"])
                        .unwrap()
                        .dimensions(2)
                        .timeout(Duration::from_secs(5))
                        .cache(None, None)
                        .generate_with(Some("openai"), Some("fixture"))
                        .await
                        .unwrap();
                    assert_eq!(vectors.embeddings, [[0.1, 0.2], [0.3, 0.4]]);
                    assert_eq!(vectors.model, "fixture");
                    assert_eq!(vectors.usage.prompt_tokens, None);
                    Embeddings::assert_generated(|request| {
                        request.inputs == ["first", "second"]
                            && request.dimensions == Some(2)
                            && request.timeout == Duration::from_secs(5)
                            && request.provider.as_deref() == Some("openai")
                    });
                    let exhausted = Embeddings::r#for(["other"]).unwrap().generate().await;
                    assert_eq!(exhausted, Err(AiError::FakeExhausted));
                    Embeddings::assert_not_generated(|request| request.inputs == ["other"]);

                    Embeddings::fake_with(|request| {
                        Ok(vec![vec![request.inputs.len() as f32, 0.5]])
                    })
                    .unwrap();
                    let callback = Embeddings::r#for(["input"])
                        .unwrap()
                        .generate()
                        .await
                        .unwrap();
                    assert_eq!(callback.embeddings, [[1.0, 0.5]]);

                    Embeddings::fake([]).unwrap();
                    let generated = Embeddings::r#for(["auto"])
                        .unwrap()
                        .dimensions(12)
                        .generate()
                        .await
                        .unwrap();
                    let first = generated.first().unwrap();
                    assert_eq!(first.len(), 12);
                    assert!(
                        (first.iter().map(|value| value * value).sum::<f32>() - 1.0).abs() < 0.0001
                    );
                    assert!(Embeddings::fake_embedding(0).is_err());
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })?;
                Ok(())
            }))
    })
}

#[tokio::test]
async fn embeddings_fake_scripts_responses_and_stays_inside_the_application() -> TestResult {
    let mut first = application()?;
    let mut second = application()?;
    assert_eq!(first.get("/fake").await?.status(), StatusCode::OK);
    assert_eq!(
        first.get("/has-fake").await?.json::<Value>()?["faked"],
        true
    );
    assert_eq!(
        second.get("/has-fake").await?.json::<Value>()?["faked"],
        false
    );
    Ok(())
}
