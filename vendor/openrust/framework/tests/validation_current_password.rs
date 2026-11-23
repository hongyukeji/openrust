#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::{Auth, AuthIdentifier, AuthManager},
    contracts::{
        auth::Authenticatable,
        hashing::Hasher,
        http::Kernel as _,
        validation::{Factory as _, Validator as _},
    },
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, Kernel},
    },
    hashing::HashOptions,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Route, Validator},
    validation::{Data, Factory, RuleSet},
};
use serde_json::{Value, json};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct User(String);
impl Authenticatable for User {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        "user".into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        Some(&self.0)
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}
struct CurrentForm;
impl FormRequest for CurrentForm {
    fn rules(&self) -> openrust::validation::ValidationResult<RuleSet> {
        RuleSet::parse([("current_password", "current_password")])
    }
    fn validation_rules(
        &self,
        request: &Request,
    ) -> openrust::validation::ValidationResult<RuleSet> {
        RuleSet::parse([(
            "current_password",
            request
                .header("x-rule")
                .unwrap()
                .unwrap_or("current_password"),
        )])
    }
    fn error_bag(&self) -> &str {
        "updatePassword"
    }
}
fn submit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let selected = request.header("x-default")?.unwrap_or("web").to_owned();
        Auth::should_use(request, &selected)?;
        if request.header("x-resolver")? == Some("yes") {
            let user = Auth::user(request).await?;
            let mut validator = Validator::make(
                Data::from_request(request).await?,
                RuleSet::parse([("current_password", "current_password:web")])?,
            )?;
            validator.set_user_resolver(move |guard| {
                Ok(if guard == Some("web") {
                    user.clone()
                } else {
                    None
                })
            });
            validator.validate_with_bag("updatePassword").await?;
        } else {
            CurrentForm.validate_resolved(request).await?;
        }
        Response::json(StatusCode::OK, json!({"ok":true}))
    })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::post("/current-password", submit)?;
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn send(address: SocketAddr, body: Value, headers: &str) -> (u16, Value) {
    timeout(Duration::from_secs(5), async {
        let body=body.to_string(); let mut socket=TcpStream::connect(address).await.unwrap();
        let wire=format!("POST /api/current-password HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",body.len());
        socket.write_all(wire.as_bytes()).await.unwrap(); let mut response=String::new(); socket.read_to_string(&mut response).await.unwrap();
        let status=response.split_whitespace().nth(1).unwrap().parse().unwrap(); let body=response.split_once("\r\n\r\n").unwrap().1;
        (status,serde_json::from_str(body).unwrap())
    }).await.unwrap()
}

#[tokio::test]
async fn current_password_matches_php_with_real_named_guards_and_hasher() {
    let root = std::env::temp_dir().join(format!(
        "openrust-current-password-{:016x}",
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut app=Application::configure(root.clone()).with_config(|_,_|Ok([
        ("app".into(),json!({"debug":false,"key":"a".repeat(32)})),
        ("hashing".into(),json!({"driver":"bcrypt","bcrypt":{"rounds":4}})),
        ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"current-web"},"admin":{"driver":"current-admin"}}})),
    ].into())).with_routing(Routing::default().api(routes)).create().unwrap();
    app.boot().await.unwrap();
    let hasher = app.make::<dyn Hasher>().await.unwrap();
    let mut hashes = std::collections::BTreeMap::new();
    for (profile, password) in [
        ("normal", "correct-password"),
        ("empty", ""),
        ("number", "123"),
    ] {
        hashes.insert(
            profile,
            hasher
                .make(password.as_bytes(), &HashOptions::new())
                .await
                .unwrap(),
        );
    }
    let manager = app.make::<AuthManager>().await.unwrap();
    manager.via_request("current-web", move |request| {
        let hash = hashes[request.header("x-profile").unwrap().unwrap_or("normal")].clone();
        Box::pin(async move { Ok(Some(Box::new(User(hash)) as Box<dyn Authenticatable>)) })
    });
    manager.via_request("current-admin", |_| Box::pin(async { Ok(None) }));
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/api-f01/fixtures/current_password.json"
    ))
    .unwrap();
    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let headers = format!(
            "X-Default: {}\r\nX-Rule: {}\r\nX-Profile: {}\r\n",
            case["default"].as_str().unwrap(),
            case["rule"].as_str().unwrap(),
            case["profile"].as_str().unwrap()
        );
        let (status, body) =
            send(address, json!({"current_password":case["value"]}), &headers).await;
        let expected = if case.get("exception").is_some() {
            500
        } else if case["passes"] == true {
            200
        } else {
            422
        };
        assert_eq!(status, expected, "case {index}: {case}; response {body}");
        if status == 422 {
            assert_eq!(body["errors"], case["messages"], "case {index}");
        }
    }
    for (body, rule, status) in [
        (json!({}), "current_password:unknown", 200),
        (
            json!({"current_password":null}),
            "nullable|current_password:unknown",
            200,
        ),
        (
            json!({"current_password":[]}),
            "bail|string|current_password:unknown",
            422,
        ),
        (
            json!({"current_password":""}),
            "required|string|current_password:unknown",
            422,
        ),
    ] {
        let (actual, _) = send(address, body, &format!("X-Rule: {rule}\r\n")).await;
        assert_eq!(actual, status);
    }
    for _ in 0..10 {
        let (web, admin) = tokio::join!(
            send(
                address,
                json!({"current_password":"correct-password"}),
                "X-Default: web\r\n"
            ),
            send(
                address,
                json!({"current_password":"correct-password"}),
                "X-Default: admin\r\n"
            ),
        );
        assert_eq!(web.0, 200);
        assert_eq!(admin.0, 422);
    }
    for (profile, password, status) in [
        ("normal", "correct-password", 200),
        ("normal", "bad", 422),
        ("empty", "", 200),
    ] {
        let (actual, _) = send(
            address,
            json!({"current_password":password}),
            &format!("X-Resolver: yes\r\nX-Profile: {profile}\r\n"),
        )
        .await;
        assert_eq!(actual, status);
    }
    server.abort();
    let _ = server.await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn current_password_missing_context_and_presence_are_explicit() {
    let factory = Factory::default();
    for (data, rule, valid) in [
        (json!({}), "current_password:unknown", true),
        (
            json!({"current_password":null}),
            "nullable|current_password:unknown",
            true,
        ),
        (
            json!({"current_password":"password"}),
            "current_password",
            false,
        ),
    ] {
        let mut validator = factory
            .make(
                Data::new(data).unwrap(),
                RuleSet::parse([("current_password", rule)]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let result = validator.validate().await;
        if valid {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result,
                Err(openrust::validation::ValidationError::Execution { .. })
            ));
        }
    }
    let mut validator = factory
        .make(
            Data::new(json!({"current_password":"password"})).unwrap(),
            RuleSet::new()
                .field(
                    "current_password",
                    [openrust::validation::Rule::current_password(Some("admin"))],
                )
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    validator.set_user_resolver(|guard| {
        assert_eq!(guard, Some("admin"));
        Ok(None)
    });
    assert!(validator.fails().await.unwrap());
    assert_eq!(
        validator
            .errors()
            .get("current_password", None)
            .into_messages(),
        ["The password is incorrect."]
    );
}
