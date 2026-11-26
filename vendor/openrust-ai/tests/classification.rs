use std::{error::Error, fs, time::Duration};

use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    support::{Str, facades::Route},
    testing::TestApp,
};
use openrust_ai::{
    AiError, Classification, ImageAttachment,
    classification::{Answer, Boolean, Choice, ChoiceOption, Score, ScoreLevel},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::timeout,
};

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAAD0lEQVR4nGPg2S9/4D8DAAjEAqoqtepMAAAAAElFTkSuQmCC";

#[tokio::test]
async fn decisions_classify_typed_questions_and_report_use()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = tokio::spawn(async move {
        for invalid in [false, true] {
            let (socket, _) = timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end(), "POST /v1/decisions HTTP/1.1");
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
                    authorized = value.trim() == "Bearer decisions-fixture";
                }
            }
            assert!(authorized);
            let mut bytes = vec![0; size];
            socket.read_exact(&mut bytes).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                body["model"],
                if invalid { "override" } else { "default-model" }
            );
            let value = if invalid {
                assert_eq!(body["input"], "An item");
                json!({"answers":[{"name":"category","type":"choice","choice":"missing","probabilities":[]}]})
            } else {
                assert_eq!(
                    body["input"][0]["content"][0]["text"],
                    "{\"topic\":\"returns\"}"
                );
                assert_eq!(
                    body["input"][0]["content"][1]["image_url"],
                    format!("data:image/png;base64,{PNG}")
                );
                let questions = &body["questions"];
                assert_eq!(
                    questions[0],
                    json!({"type":"predicate","name":"eligible","instructions":"Can this be returned?\n\nTrue: It was bought recently\n\nFalse: It was never bought"})
                );
                assert_eq!(
                    questions[1]["choices"][1],
                    json!({"value":"refund","description":"Needs a refund"})
                );
                assert_eq!(
                    questions[2]["levels"][2],
                    json!({"label":"high","description":"Action today"})
                );
                assert_eq!(questions[3]["name"], "private_question");
                json!({
                    "model":"model-actual",
                    "answers":[
                        {"name":"eligible","type":"predicate","probability":0.77},
                        {"name":"category","type":"choice","choice":"refund","confidence":0.89,"probabilities":[{"value":"refund","probability":0.7},{"value":"other","probability":0.3}]},
                        {"name":"severity","type":"score","score":1.6,"probabilities":[{"value":2,"label":"high","probability":0.8}]},
                        {"name":"private_question","type":"refusal"}
                    ],
                    "usage":{"input_tokens":20,"output_tokens":5,"input_tokens_details":{"cached_tokens":2,"cache_write_tokens":3},
                        "output_tokens_details":{"reasoning_tokens":1}}
                })
            };
            let output = value.to_string();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                output.len()
            );
            socket.get_mut().write_all(header.as_bytes()).await.unwrap();
            socket.get_mut().write_all(output.as_bytes()).await.unwrap();
        }
    });
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/classify", || async {
                    assert!(matches!(
                        Classification::of("   "),
                        Err(AiError::InvalidClassification)
                    ));
                    assert!(matches!(
                        Classification::of("x").unwrap().classify().await,
                        Err(AiError::InvalidClassification)
                    ));
                    assert!(matches!(
                        Classification::of("x")
                            .unwrap()
                            .question("choice", Choice::new("categorize", ["a", "a"]))
                            .classify()
                            .await,
                        Err(AiError::InvalidClassification)
                    ));
                    let result = Classification::of(json!({"topic":"returns"}))
                        .unwrap()
                        .question(
                            "eligible",
                            Boolean::new("Can this be returned?")
                                .when_true("It was bought recently")
                                .when_false("It was never bought"),
                        )
                        .question(
                            "category",
                            Choice::new(
                                "Which category?",
                                [
                                    "other".into(),
                                    ChoiceOption::new("refund", "Needs a refund"),
                                ],
                            ),
                        )
                        .question(
                            "severity",
                            Score::new(
                                "How urgent?",
                                [
                                    "low".into(),
                                    "medium".into(),
                                    ScoreLevel::new("high", "Action today"),
                                ],
                            ),
                        )
                        .question("private_question", Boolean::new("Can you answer?"))
                        .attachments([
                            ImageAttachment::from_bytes(Str::from_base64(PNG).unwrap()).unwrap()
                        ])
                        .classify()
                        .await
                        .unwrap();
                    let Some(Answer::Boolean(eligible)) = result.answer("eligible") else {
                        panic!("missing boolean")
                    };
                    assert!(eligible.is_true() && !eligible.is_true_at(0.9));
                    let Some(Answer::Choice(category)) = result.answer("category") else {
                        panic!("missing choice")
                    };
                    assert_eq!(category.choice, "refund");
                    assert_eq!(category.probability_of("refund"), 0.7);
                    let Some(Answer::Score(severity)) = result.answer("severity") else {
                        panic!("missing score")
                    };
                    assert_eq!(severity.level(), 2);
                    assert_eq!(severity.label(), Some("high"));
                    assert_eq!(severity.normalized(), 0.8);
                    assert_eq!(result.answer("private_question"), Some(&Answer::Refusal));
                    assert_eq!(result.usage.tokens.prompt_tokens, Some(20));
                    assert_eq!(result.usage.cached_input_tokens, Some(2));
                    assert_eq!(result.usage.reasoning_tokens, Some(1));
                    assert_eq!(result.model, "model-actual");
                    assert!(matches!(
                        Classification::of("An item")
                            .unwrap()
                            .question("category", Choice::new("Categorize", ["a", "b"]))
                            .classify_with(Some("openai"), Some("override"))
                            .await,
                        Err(AiError::InvalidResponse)
                    ));
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_CLASSIFICATION_PROVIDER=openai\nOPENAI_CLASSIFICATION_MODEL=default-model\nOPENAI_URL=http://{address}/v1\nOPENAI_API_KEY=decisions-fixture\n"
        ),
    )?;
    assert_eq!(app.get("/classify").await?.status(), StatusCode::OK);
    upstream.await?;
    Ok(())
}

#[tokio::test]
async fn classification_fake_and_decide_use_normal_application_entry_points()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use std::collections::BTreeMap;

    use openrust::support::Collection;
    use openrust_ai::{
        Ai, DecideOptions, Decides, DecidesBetween, DecidesText,
        classification::{BooleanAnswer, ChoiceAnswer},
    };

    let upstream = TcpListener::bind("127.0.0.1:0").await?;
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/fake-decisions", || async {
                    assert!(!Classification::is_faked().unwrap());
                    let fake = Ai::fake_classification([BTreeMap::from([(
                        "decision".into(),
                        Answer::Boolean(BooleanAnswer { probability: 0.8 }),
                    )])
                    .into()])
                    .unwrap();
                    fake.prevent_stray_classifications(true);
                    Classification::assert_nothing_classified().unwrap();
                    assert!(
                        Str::decide("a sale request", "Is it a sale?")
                            .await
                            .unwrap()
                    );
                    assert!(matches!(
                        Str::of("a new request").decide("Is it new?").await,
                        Err(AiError::FakeExhausted)
                    ));
                    Classification::assert_classified(|request| {
                        request.state == "a sale request"
                            && request.model.as_deref() == Some("fixture-model")
                    })
                    .unwrap();
                    Classification::assert_not_classified(|request| {
                        request.state == "a new request"
                    })
                    .unwrap();

                    let fake = Classification::fake_with(|request| {
                        let answer = match &request.questions[0].1 {
                            openrust_ai::classification::Question::Choice(question) => {
                                let selected = question.options()[1].value.clone();
                                Answer::Choice(ChoiceAnswer {
                                    choice: selected.clone(),
                                    probabilities: BTreeMap::from([(selected, 0.64)]),
                                    confidence: None,
                                })
                            }
                            _ => Answer::Boolean(BooleanAnswer { probability: 0.7 }),
                        };
                        Ok(BTreeMap::from([("decision".into(), answer)]).into())
                    })
                    .unwrap();
                    assert!(
                        !Str::of("Example")
                            .decide_with(
                                "Does it match?",
                                DecideOptions {
                                    threshold: Some(0.8),
                                    ..Default::default()
                                }
                            )
                            .await
                            .unwrap()
                    );
                    let values = Collection::make(["refund", "sale"]);
                    assert_eq!(
                        values.decide("Which category?", "Purchase").await.unwrap(),
                        Some(&"sale")
                    );
                    let values = Collection::make([(1, "Refund"), (2, "New purchase")]);
                    assert_eq!(
                        values
                            .decide_by(
                                "Which category?",
                                json!({"text":"Purchase"}),
                                |item| ChoiceOption::new(item.0.to_string(), item.1),
                                DecideOptions {
                                    model: Some("overridden"),
                                    timeout: Some(Duration::from_secs(9)),
                                    ..Default::default()
                                },
                            )
                            .await
                            .unwrap(),
                        Some(&(2, "New purchase"))
                    );
                    let keys = Collection::from_entries([
                        ("refund", "Return a product"),
                        ("sale", "Buy a product"),
                    ]);
                    assert_eq!(
                        keys.decide_key(
                            "Choose",
                            "purchase",
                            DecideOptions {
                                threshold: Some(0.7),
                                ..Default::default()
                            }
                        )
                        .await
                        .unwrap(),
                        None
                    );
                    assert_eq!(
                        keys.decide_key("Choose", "purchase", DecideOptions::default())
                            .await
                            .unwrap()
                            .as_deref(),
                        Some("sale")
                    );
                    assert!(matches!(
                        Collection::make(["duplicate", "duplicate"])
                            .decide("Choose", "purchase")
                            .await,
                        Err(AiError::InvalidClassification)
                    ));
                    fake.assert_classified(|request| {
                        request.timeout == Duration::from_secs(9)
                            && request.model.as_deref() == Some("overridden")
                    });

                    Classification::fake([]).unwrap();
                    let generated = Classification::of("A sample")
                        .unwrap()
                        .question("eligible", Boolean::new("Is it eligible?"))
                        .question("category", Choice::new("Category?", ["a", "b"]))
                        .question("severity", Score::new("How urgent?", ["low", "high"]))
                        .attachments([
                            ImageAttachment::from_bytes(Str::from_base64(PNG).unwrap()).unwrap()
                        ])
                        .classify()
                        .await
                        .unwrap();
                    assert!(matches!(
                        generated.answer("eligible"),
                        Some(Answer::Boolean(_))
                    ));
                    assert!(matches!(
                        generated.answer("category"),
                        Some(Answer::Choice(_))
                    ));
                    assert!(matches!(
                        generated.answer("severity"),
                        Some(Answer::Score(_))
                    ));
                    Classification::assert_classified(|request| request.attachments.len() == 1)
                        .unwrap();
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
            }))
    })?;
    fs::write(
        app.root().join(".env.testing"),
        format!(
            "AI_CLASSIFICATION_PROVIDER=openai\nOPENAI_CLASSIFICATION_MODEL=fixture-model\nOPENAI_URL=http://{}/v1\nOPENAI_API_KEY=unused-fixture\n",
            upstream.local_addr()?
        ),
    )?;
    assert_eq!(app.get("/fake-decisions").await?.status(), StatusCode::OK);
    assert!(
        timeout(Duration::from_millis(100), upstream.accept())
            .await
            .is_err()
    );
    Ok(())
}
