use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    validation::{Rule, RuleContext, RuleFailure, RuleSet, Value},
};
use serde_json::json;
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn validate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field(
            "title",
            [Rule::custom(
                |context: &RuleContext<'_>, _: &str, _: Option<&Value>| {
                    let replacements = || {
                        BTreeMap::from([
                            ("attribute".into(), "chosen".into()),
                            ("token".into(), "X".into()),
                        ])
                    };
                    let before = RuleFailure::new("rules.moment").translate(
                        context,
                        Default::default(),
                        None,
                    );
                    openrust::support::facades::Lang::set_locale("fr").map_err(|source| {
                        openrust::validation::ValidationError::Execution {
                            source: Box::new(source),
                        }
                    })?;
                    let after = RuleFailure::new("rules.moment").translate(
                        context,
                        Default::default(),
                        None,
                    );
                    openrust::support::facades::Lang::set_locale("en").map_err(|source| {
                        openrust::validation::ValidationError::Execution {
                            source: Box::new(source),
                        }
                    })?;
                    Ok(vec![
                        before,
                        after,
                        RuleFailure::new("rules.invalid").translate(
                            context,
                            replacements(),
                            Some("fr"),
                        ),
                        RuleFailure::new("rules.invalid").translate(
                            context,
                            Default::default(),
                            Some("zh-Hans"),
                        ),
                        RuleFailure::new("rules.invalid").translate(
                            context,
                            Default::default(),
                            None,
                        ),
                        RuleFailure::new("rules.invalid").translate(
                            context,
                            Default::default(),
                            Some("es"),
                        ),
                        RuleFailure::new("rules.items").translate_choice(
                            context,
                            1_i64,
                            Default::default(),
                            Some("en"),
                        ),
                        RuleFailure::new("rules.items").translate_choice(
                            context,
                            2_i64,
                            Default::default(),
                            Some("en"),
                        ),
                        RuleFailure::new("rules.absent").translate(
                            context,
                            Default::default(),
                            Some("fr"),
                        ),
                        RuleFailure::new("literal :token").translate(
                            context,
                            replacements(),
                            Some("fr"),
                        ),
                        RuleFailure::new("nested :token").translate(
                            context,
                            BTreeMap::from([("token".into(), ":attribute / :token".into())]),
                            Some("fr"),
                        ),
                        RuleFailure::new("validation.required").translate(
                            context,
                            Default::default(),
                            Some("fr"),
                        ),
                    ])
                },
            )],
        )?;
        request
            .validate_with(
                rules,
                BTreeMap::from([("required".into(), "not the rule message".into())]),
                Default::default(),
            )
            .await?;
        Response::json(StatusCode::OK, json!("unexpected success"))
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn send(address: SocketAddr) -> serde_json::Value {
    timeout(Duration::from_secs(5), async {
        let mut stream=TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("POST /validate HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{{\"title\":\"x\"}}").as_bytes()).await.unwrap();
        let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 422"),"{response}");serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }).await.unwrap()
}
#[tokio::test]
async fn rule_failure_translates_each_message_without_changing_the_request_locale() {
    let root = tempfile::tempdir().unwrap();
    for (locale, invalid) in [
        ("en", "EN :attribute"),
        ("fr", "FR :attribute :token"),
        ("zh-Hans", "中文 :attribute"),
    ] {
        let directory = root.path().join("lang").join(locale);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("rules.json"),
            json!({"invalid":invalid,"moment":format!("{locale} snapshot :attribute"),"items":"{1} One :attribute|[2,*] :count :attribute items"})
                .to_string(),
        )
        .unwrap();
        std::fs::write(
            directory.join("validation.json"),
            json!({"required":"FR required :attribute"}).to_string(),
        )
        .unwrap();
    }
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"locale":"en","fallback_locale":"en","supported_locales":[]}),
            )]))
        })
        .with_routing(Routing::new(|_| {
            let mut router = Router::new();
            router.post("/validate", validate)?;
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "reference/validation-failure-translation.json"
    ))
    .unwrap();
    let mut requests = Vec::new();
    for _ in 0..24 {
        requests.push(tokio::spawn(send(address)));
    }
    for request in requests {
        assert_eq!(request.await.unwrap()["errors"], expected["errors"]);
    }
    server.abort();
    let _ = server.await;
}
