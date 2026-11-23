use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteParameter, Router, SignatureMode, SignatureQueryFilter},
    support::facades::Url,
    validation::RuleSet,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn links(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let params = [
            RouteParameter::named("user", "42"),
            RouteParameter::named("email", "signed@example.test"),
        ];
        Response::json(
            StatusCode::OK,
            json!({
                "absolute":Url::signed_route("unsubscribe",&params,None,SignatureMode::Absolute)?,
                "relative":Url::signed_route("unsubscribe",&params,None,SignatureMode::Relative)?,
                "expired":Url::temporary_signed_route("unsubscribe",SystemTime::now()-Duration::from_secs(60),&params,SignatureMode::Absolute)?,
                "custom":Url::temporary_signed_route("custom",SystemTime::now()-Duration::from_secs(60),&params,SignatureMode::Absolute)?,
                "guarded":Url::signed_route("guarded",&params,None,SignatureMode::Absolute)?,
            }),
        )
    })
}
fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let absolute = request.has_valid_signature(true)?;
        let relative = request.has_valid_relative_signature()?;
        let ignored = request.has_valid_signature_while_ignoring(&["page", "order"], true)?;
        let ignored_relative =
            request.has_valid_relative_signature_while_ignoring(&["page", "order"])?;
        let ignore = |name: &str| matches!(name, "page" | "order");
        assert_eq!(
            ignored,
            request.has_valid_signature_while_ignoring(
                SignatureQueryFilter::Predicate(&ignore),
                true
            )?
        );
        assert_eq!(
            ignored_relative,
            request.has_valid_relative_signature_while_ignoring(
                SignatureQueryFilter::Predicate(&ignore)
            )?
        );
        let names = vec!["page", "order"];
        assert_eq!(
            ignored,
            request.has_valid_signature_while_ignoring(&names, true)?
        );
        let facade = request
            .with_container(async {
                openrust::support::facades::Request::has_valid_signature_while_ignoring(
                    SignatureQueryFilter::Predicate(&ignore),
                    true,
                )
            })
            .await?;
        assert_eq!(ignored, facade);
        Response::json(
            if ignored || ignored_relative {
                StatusCode::OK
            } else {
                StatusCode::UNAUTHORIZED
            },
            json!({
                "absolute":absolute,"relative":relative,"ignored":ignored,"ignored_relative":ignored_relative,
                "signed_email":request.query("email")?,"input_email":request.input("email").await?,
            }),
        )
    })
}
fn validate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::parse([("name", "required|string"), ("email", "required|email")])?;
        let data = if request.uri().path() == "/profile" {
            request.validate_with_bag("profile", rules).await?
        } else {
            request.validate(rules).await?
        };
        assert!(
            !request.is_precognitive(),
            "指定字段预验证必须在入口提前返回，不能继续业务操作"
        );
        Response::json(StatusCode::OK, data.into_value())
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn start(
    key: &str,
    old: Option<&str>,
) -> (tempfile::TempDir, SocketAddr, tokio::task::JoinHandle<()>) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("app.json"), serde_json::to_vec(&json!({"env":"testing","url":"http://localhost","url_explicit":false,"key":key,"previous_keys":old.into_iter().collect::<Vec<_>>(),"locale":"en","supported_locales":[]})).unwrap()).unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_config(|base, _| {
            Ok(BTreeMap::from([
                (
                    "app".into(),
                    serde_json::from_slice(&std::fs::read(base.join("app.json")).unwrap()).unwrap(),
                ),
                ("session".into(), json!({"driver":"array"})),
            ]))
        })
        .with_exceptions(|exceptions| {
            exceptions.render::<HttpError>(|error, request| {
                (matches!(error, HttpError::InvalidSignature) && request.is("custom/*")).then(
                    || {
                        Response::json(
                            StatusCode::FORBIDDEN,
                            json!({"message":"This link has expired."}),
                        )
                        .unwrap()
                    },
                )
            });
        })
        .with_routing(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/links", links)?;
            router.get("/", url_details)?;
            router.get("/chapter/", url_details)?;
            router
                .get("/custom/{user}", inspect)?
                .name("custom")
                .middleware_name("signed");
            router
                .get("/unsubscribe/{user}", inspect)?
                .name("unsubscribe");
            router
                .get("/guarded/{user}", inspect)?
                .name("guarded")
                .middleware_name("signed");
            router
                .post("/validate", validate)?
                .middleware_name("precognitive");
            router
                .post("/profile", validate)?
                .middleware_name("precognitive");
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (root, addr, server)
}
async fn send(
    address: SocketAddr,
    method: &str,
    target: &str,
    body: &str,
    headers: &str,
) -> (u16, Value, String) {
    let path = if target.starts_with("http") {
        let url = url::Url::parse(target).unwrap();
        format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        )
    } else {
        target.to_owned()
    };
    timeout(Duration::from_secs(5),async {
        let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let data=response.split_once("\r\n\r\n").unwrap().1;
        let value=if data.is_empty(){Value::Null}else{serde_json::from_str(data).unwrap()};(status,value,response)
    }).await.unwrap()
}
#[tokio::test]
async fn official_signed_link_recipe_uses_request_instance_and_preserves_unsigned_body_boundary() {
    let (_root, address, server) = start("first-signing-key", None).await;
    let (_, urls, _) = send(address, "GET", "/links", "", "").await;
    for mode in ["absolute", "relative"] {
        let link = urls[mode].as_str().unwrap();
        let (status, data, _) =
            send(address, "GET", link, r#"{"email":"body@example.test"}"#, "").await;
        assert_eq!(status, 200);
        assert_eq!(data[mode], true);
        assert_eq!(data["signed_email"], "signed@example.test");
        assert_eq!(data["input_email"], "body@example.test");
        let (status, data, _) = send(
            address,
            "GET",
            &format!("{link}&page=2&order=latest"),
            "",
            "",
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(data[mode], false);
        assert_eq!(
            data[if mode == "absolute" {
                "ignored"
            } else {
                "ignored_relative"
            }],
            true
        );
        let (status, _, _) = send(address, "GET", &link.replace("/42?", "/43?"), "", "").await;
        assert_eq!(status, 401);
    }
    assert_eq!(
        send(address, "GET", urls["expired"].as_str().unwrap(), "", "")
            .await
            .0,
        401
    );
    assert_eq!(
        send(address, "GET", urls["guarded"].as_str().unwrap(), "", "")
            .await
            .0,
        200
    );
    assert_eq!(
        send(
            address,
            "GET",
            &format!("{}&page=2", urls["guarded"].as_str().unwrap()),
            "",
            ""
        )
        .await
        .0,
        403
    );
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn request_signature_verification_keeps_application_keys_isolated_and_accepts_rotation() {
    let (_r, a, s) = start("old-signing-key", None).await;
    let (_r2, b, s2) = start("different-signing-key", None).await;
    let (_r3, c, s3) = start("new-signing-key", Some("old-signing-key")).await;
    let (_, urls, _) = send(a, "GET", "/links", "", "").await;
    let link = urls["relative"].as_str().unwrap();
    assert_eq!(send(a, "GET", link, "", "").await.0, 200);
    assert_eq!(send(b, "GET", link, "", "").await.0, 401);
    assert_eq!(send(c, "GET", link, "", "").await.0, 200);
    for task in [s, s2, s3] {
        task.abort();
        let _ = task.await;
    }
}
#[tokio::test]
async fn controller_request_validation_and_named_bag_follow_the_http_lifecycle() {
    let (_r, address, server) = start("validation-key", None).await;
    for target in ["/validate", "/profile"] {
        assert_eq!(
            send(address, "POST", target, r#"{"name":"Ada"}"#, "")
                .await
                .0,
            422
        );
        let (status, data, _) = send(
            address,
            "POST",
            target,
            r#"{"name":"Ada","email":"ada@example.test","admin":true}"#,
            "",
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(data, json!({"name":"Ada","email":"ada@example.test"}));
        let headers = "Precognition: true\r\nPrecognition-Validate-Only: name\r\n";
        let (status, _, wire) = send(address, "POST", target, r#"{"name":"Ada"}"#, headers).await;
        assert_eq!(status, 204, "{wire}");
        assert!(wire.contains("precognition-success: true"));
    }
    server.abort();
    let _ = server.await;
}

fn url_details(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let full = request.full_url()?;
        let generated = request.with_container(async {
            Ok::<_, HttpError>(json!({"current":Url::current()?,"full":Url::full()?,"root":Url::to("/", &[], None)?}))
        }).await?;
        Response::json(
            StatusCode::OK,
            json!({"request_full":full,"generated":generated}),
        )
    })
}

#[tokio::test]
async fn current_and_full_urls_keep_root_and_normalized_query_semantics() {
    let (_root, address, server) = start("url-chapter-key", None).await;
    let origin = format!("http://{address}");
    for (target, current, full) in [
        ("/", origin.clone(), origin.clone()),
        (
            "/?z=1&a=hello%20world",
            origin.clone(),
            format!("{origin}/?a=hello%20world&z=1"),
        ),
        (
            "/chapter/?z=1&a=hello%20world",
            format!("{origin}/chapter"),
            format!("{origin}/chapter?a=hello%20world&z=1"),
        ),
    ] {
        let (status, value, _) = send(address, "GET", target, "", "").await;
        assert_eq!(status, 200, "{value}");
        assert_eq!(value["generated"]["current"], current);
        assert_eq!(value["generated"]["full"], full);
        assert_eq!(value["request_full"], full);
        assert_eq!(value["generated"]["root"], origin);
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn expired_signed_routes_use_bootstrap_exception_renderer() {
    let (_root, address, server) = start("custom-signed-key", None).await;
    let (_, links, _) = send(address, "GET", "/links", "", "").await;
    let (status, value, _) = send(address, "GET", links["custom"].as_str().unwrap(), "", "").await;
    assert_eq!(status, 403);
    assert_eq!(value, json!({"message":"This link has expired."}));
    let (status, value, _) = send(address, "GET", "/guarded/42", "", "").await;
    assert_eq!(status, 403);
    assert_ne!(value, json!({"message":"This link has expired."}));
    server.abort();
    let _ = server.await;
}
