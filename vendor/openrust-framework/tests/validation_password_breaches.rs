use openrust::{
    contracts::{http::Kernel as _, validation::UncompromisedVerifier},
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        http::Kernel,
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    validation::{NotPwnedVerifier, Value},
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
fn fixture() -> Value {
    let mut fixture: Value =
        serde_json::from_str(include_str!("reference/validation-password-breaches.json")).unwrap();
    let mut timeout = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            case["value"] == "secret" && case["mode"] == "exception" && case["threshold"] == 0
        })
        .unwrap()
        .clone();
    timeout["mode"] = json!("timeout");
    fixture["cases"].as_array_mut().unwrap().push(timeout);
    fixture
}
fn verify(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let index: usize = request.route("index").unwrap().parse().unwrap();
        let fixtures = fixture();
        let case = &fixtures["cases"][index];
        let endpoint = request.header("x-range-endpoint")?.unwrap().to_owned();
        let factory = Arc::new(openrust::http::client::Factory::new().unwrap());
        factory.global_request_middleware(move |request| {
            assert_eq!(request.url().host_str(), Some("api.pwnedpasswords.com"));
            assert_eq!(request.url().scheme(), "https");
            assert_eq!(request.url().path().len(), 12);
            let url = format!("{endpoint}/{index}{}", request.url().path());
            *request.url_mut() = url.parse().unwrap();
            Ok(())
        });
        let verifier = NotPwnedVerifier::new(factory, Some(std::time::Duration::from_millis(150)));
        let value = request.input(Some("value")).await?.unwrap_or(Value::Null);
        let passes = verifier
            .verify(&value, case["threshold"].as_i64().unwrap())
            .await?;
        Response::json(StatusCode::OK, json!({"passes":passes}))
    })
}
fn routes(_: &Application) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.post("/verify/{index}", verify)?;
    Ok(router)
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
#[tokio::test]
async fn breach_range_transport_and_failure_reporting_match_fixed_reference() {
    use axum::handler::Handler as _;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let range_address = listener.local_addr().unwrap();
    let observed = Arc::new(Mutex::new(Vec::<(usize, String)>::new()));
    let recorder = observed.clone();
    let range_server = tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {connection=listener.accept()=>{let(mut socket,_)=connection.unwrap();let recorder=recorder.clone();jobs.spawn(async move{let mut raw=Vec::new();loop{let mut byte=[0];if socket.read_exact(&mut byte).await.is_err(){return;}raw.push(byte[0]);if raw.ends_with(b"\r\n\r\n"){break;}assert!(raw.len()<65536);}let headers=String::from_utf8(raw).unwrap();let uri=headers.lines().next().unwrap().split(' ').nth(1).unwrap();let index:usize=uri.split('/').nth(1).unwrap().parse().unwrap();recorder.lock().unwrap().push((index,headers));let fixtures=fixture();let case=&fixtures["cases"][index];if case["mode"]=="exception"{return;}else if case["mode"]=="timeout"{tokio::time::sleep(std::time::Duration::from_millis(500)).await;}let body=case["body"].as_str().unwrap();let status=case["status"].as_u64().unwrap();let wire=format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());let _=socket.write_all(wire.as_bytes()).await;});},Some(job)=jobs.join_next(),if !jobs.is_empty()=>{job.unwrap();}}
        }
    });
    let reports = Arc::new(AtomicUsize::new(0));
    let count = reports.clone();
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_routing(Routing::new(routes))
            .with_middleware(|middleware| {
                middleware.skip_trim_strings_when(|_| true);
                middleware.convert_empty_strings_to_null(|_| true);
            })
            .with_exceptions(move |exceptions| {
                exceptions.report::<openrust::http::client::Error>(move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                    ReportDecision::Stop
                });
            })
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for (index, case) in fixture()["cases"].as_array().unwrap().iter().enumerate() {
        let before = reports.load(Ordering::SeqCst);
        let body = json!({"value":case["value"]}).to_string();
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let wire = format!(
            "POST /verify/{index} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nx-range-endpoint: http://{range_address}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            socket.read_to_string(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{index}: {response}");
        let actual: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(actual["passes"], case["passes"], "{index}: {case}");
        assert_eq!(
            reports.load(Ordering::SeqCst) - before,
            case["reports"].as_u64().unwrap() as usize,
            "{index}"
        );
        let observed = observed.lock().unwrap();
        let requests: Vec<_> = observed.iter().filter(|(i, _)| *i == index).collect();
        if case["client_calls"].as_array().unwrap().is_empty() {
            assert!(requests.is_empty());
        } else {
            assert_eq!(requests.len(), 1, "{index}: {requests:?}");
            let wire = &requests[0].1;
            let expected = case["client_calls"][2]["url"]
                .as_str()
                .unwrap()
                .rsplit('/')
                .next()
                .unwrap();
            assert!(
                wire.starts_with(&format!("GET /{index}/range/{expected} HTTP/1.1\r\n")),
                "{wire}"
            );
            assert!(wire.to_ascii_lowercase().contains("add-padding: true\r\n"));
        }
    }
    server.abort();
    let _ = server.await;
    range_server.abort();
    let _ = range_server.await;
}
struct AlwaysSafe;
impl UncompromisedVerifier for AlwaysSafe {
    fn verify<'a>(
        &'a self,
        _: &'a Value,
        _: i64,
    ) -> openrust::validation::ValidationFuture<'a, openrust::validation::ValidationResult<bool>>
    {
        Box::pin(async { Ok(true) })
    }
}
fn production(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::contracts::{
            container::Container as _, foundation::Application as _, validation::Validator as _,
        };
        use openrust::support::facades::{App, Facade as _, Validator};
        let mut validator = Validator::make(
            openrust::validation::Data::from_request(request).await?,
            openrust::validation::RuleSet::new().field(
                "password",
                [openrust::validation::rules::Password::min(3)
                    .uncompromised(0)
                    .into()],
            )?,
        )?;
        if request.route("mode") == Some("override") {
            App::get_facade_root()?
                .container()
                .instance::<dyn UncompromisedVerifier>(Arc::new(AlwaysSafe))?;
        }
        Response::json(StatusCode::OK, validator.validate().await?)
    })
}
#[tokio::test]
async fn provider_contract_uses_pooled_http_and_resolves_rebinding_at_verification_time() {
    use axum::handler::Handler as _;
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let state = Arc::new(AtomicUsize::new(200));
    let status = state.clone();
    let expected = fixture()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["value"] == "secret" && case["mode"] == "one")
        .unwrap()["body"]
        .as_str()
        .unwrap()
        .to_owned();
    let router = axum::Router::new().fallback(move |request: axum::extract::Request| {
        let expected = expected.clone();
        let observed = observed.clone();
        let status = status.clone();
        async move {
            observed.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.headers().get("add-padding").unwrap(), "true");
            assert_eq!(request.uri().path().len(), 12);
            (
                http::StatusCode::from_u16(status.load(Ordering::SeqCst) as u16).unwrap(),
                expected,
            )
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let range_address = listener.local_addr().unwrap();
    let range_server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::new(|_| {
            let mut router = openrust::routing::Router::new();
            router.post("/production/{mode}", production)?;
            Ok(router)
        }))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let factory = app.make::<openrust::http::client::Factory>().await.unwrap();
    factory.global_request_middleware(move |request| {
        *request.url_mut() = format!("http://{range_address}{}", request.url().path())
            .parse()
            .unwrap();
        Ok(())
    });
    assert!(app.container().bound::<dyn UncompromisedVerifier>());
    let _factory = app.make::<openrust::validation::Factory>().await.unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    for (mode, value, expected_status, expected_calls, status) in [
        ("normal", "secret", 422, 1, 200),
        ("normal", "x", 422, 1, 200),
        ("normal", "secret", 200, 2, 503),
        ("override", "secret", 200, 2, 200),
        ("normal", "secret", 200, 2, 200),
    ] {
        state.store(status, Ordering::SeqCst);
        let body = json!({"password":value}).to_string();
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("POST /production/{mode} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {expected_status}")),
            "{response}"
        );
        if expected_status == 422 && value == "secret" {
            let response: Value =
                serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(
                response["errors"]["password"][0],
                "The given password has appeared in a data leak. Please choose a different password."
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
    }
    server.abort();
    let _ = server.await;
    range_server.abort();
    let _ = range_server.await;
}
