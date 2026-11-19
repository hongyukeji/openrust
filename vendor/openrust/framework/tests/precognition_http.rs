use ::http::HeaderValue;
use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::access::{AuthorizationError, Response as Authorization},
    contracts::{
        container::Container as _,
        foundation::Application as _,
        http::{Kernel as _, Middleware},
        session::Session as _,
        validation::{Factory as _, Validator as _},
    },
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, Kernel, ValidatedFormRequest},
    },
    http::{Body, HttpError, HttpFuture, Method, Next, Request, Response, StatusCode},
    routing::{ControllerDispatcher, RouteGroup, RoutingError},
    support::{ProviderResult, ServiceProvider, facades::Route},
    validation::{Data, Factory, RuleSet, ValidatedInput, ValidationFuture, ValidationResult},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

#[derive(Default)]
struct Counters {
    factories: AtomicUsize,
    prepared: AtomicUsize,
    authorized: AtomicUsize,
    passed: AtomicUsize,
    actions: AtomicUsize,
    persistence: OnceLock<(openrust::database::Connection, String)>,
}
#[derive(Default)]
struct Form(Arc<Counters>);
impl FormRequest for Form {
    fn prepareForValidation(&mut self, _: &mut Data) -> ValidationResult<()> {
        self.0.prepared.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn authorize<'a>(
        &'a self,
        request: &'a mut Request,
        _: &'a Data,
    ) -> ValidationFuture<'a, Result<(), AuthorizationError>> {
        Box::pin(async move {
            self.0.authorized.fetch_add(1, Ordering::SeqCst);
            if request.headers().contains_key("x-denied") {
                return Err(AuthorizationError::Denied(Authorization::deny("Forbidden")));
            }
            Ok(())
        })
    }
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([
            ("name", "required|string|min:2"),
            ("users.*.email", "required|string"),
        ])
    }
    fn validationRules(&self, request: &Request) -> ValidationResult<RuleSet> {
        let rules = self.rules()?;
        if request.headers().contains_key("x-upload") {
            return rules.field(
                "attachment",
                [
                    openrust::validation::Rule::required(),
                    openrust::validation::rules::File::types(["txt"]).into(),
                ],
            );
        }
        if !request.isPrecognitive() {
            return rules.field("confirmation", [openrust::validation::Rule::required()]);
        }
        Ok(rules)
    }
    fn passedValidation(&mut self, _: &mut Request, _: &ValidatedInput) -> ValidationResult<()> {
        self.0.passed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
struct Controller(Arc<Counters>);
impl Controller {
    fn store<'a>(
        &'a self,
        _: &'a mut Request,
        _: ValidatedFormRequest<Form>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            self.0.actions.fetch_add(1, Ordering::SeqCst);
            if let Some((connection, table)) = self.0.persistence.get() {
                connection
                    .statement(&format!("INSERT INTO {table} (name) VALUES ('Ada')"), &[])
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
            }
            Response::json(StatusCode::OK, json!({"stored":true}))
        })
    }
    fn plain<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            self.0.actions.fetch_add(1, Ordering::SeqCst);
            Ok(Response::new(StatusCode::OK, Body::from("stored")))
        })
    }
}
struct Provider(Arc<Counters>);
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let counters = self.0.clone();
            app.container()
                .bindAsync::<Controller, _>(move |_| {
                    let counters = counters.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        counters.factories.fetch_add(1, Ordering::SeqCst);
                        Ok(Arc::new(Controller(counters)))
                    })
                })
                .await?;
            let counters = self.0.clone();
            app.container()
                .bind::<Form, _>(move |_| Ok(Arc::new(Form(counters.clone()))))?;
            Ok(())
        })
    }
}
struct Early;
impl Middleware for Early {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if let Some(vary) = request.headers().get("x-early") {
                let mut response = Response::new(StatusCode::FORBIDDEN, Body::from("middleware"));
                response.parts_mut().headers.append("vary", vary.clone());
                response
                    .parts_mut()
                    .headers
                    .append("vary", HeaderValue::from_static("Second"));
                return Ok(response);
            }
            Ok(next.run(request).await)
        })
    }
}
fn plain(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::new(StatusCode::OK, Body::from("closure"))) })
}
fn precognitive_view() -> openrust::view::View {
    panic!("Precognition must not invoke the View callback")
}
fn precognitive_api() -> openrust::http::ApiResponse<Value> {
    panic!("Precognition must not invoke the ApiResponse callback")
}
async fn injected(form: ValidatedFormRequest<Form>) -> Result<Response, HttpError> {
    Response::json(StatusCode::OK, form.validated().all())
}
fn routes() -> Result<(), RoutingError> {
    Route::r#match(
        &[Method::POST],
        "/raw",
        ControllerDispatcher::withFormRequest(Controller::store),
    )?;
    Route::group(
        RouteGroup::new().middleware_name("precognitive"),
        |router| {
            router
                .r#match(
                    &[Method::POST],
                    "/form",
                    ControllerDispatcher::withFormRequest(Controller::store),
                )?
                .middleware(Arc::new(Early));
            router.r#match(
                &[Method::POST],
                "/plain",
                ControllerDispatcher::new(Controller::plain),
            )?;
            router.r#match(&[Method::POST], "/closure", plain)?;
            router.r#match(&[Method::POST], "/injected", injected)?;
            router.r#match(&[Method::POST], "/view", precognitive_view)?;
            router.r#match(&[Method::POST], "/api-response", precognitive_api)?;
            Ok(())
        },
    )?;
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}
async fn start(counters: Arc<Counters>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withProviders(vec![Box::new(Provider(counters))])
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    serve(app).await
}
async fn serve(app: Application) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        addr,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}
async fn send(addr: SocketAddr, path: &str, headers: &str, body: &str, status: u16) -> String {
    timeout(Duration::from_secs(10), async {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let wire = format!("POST /api/{path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}", body.len());
        socket.write_all(wire.as_bytes()).await.unwrap(); let mut response = String::new(); socket.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{response}"); response
    }).await.unwrap()
}
const VALID: &str = r#"{"name":"Ada","confirmation":true,"users":[{"email":"a"}]}"#;
const PRE: &str = "Precognition: true\r\n";
#[tokio::test]
async fn precognition_resolves_dependencies_authorizes_and_never_calls_actions() {
    let c = Arc::new(Counters::default());
    let (addr, server) = start(c.clone()).await;
    let response = send(addr, "form", PRE, VALID, 204).await;
    assert!(
        response.contains("precognition-success: true\r\n")
            && response.contains("precognition: true\r\n")
            && response.contains("vary: Precognition\r\n")
    );
    assert!(response.ends_with("\r\n\r\n"));
    assert_eq!(c.factories.load(Ordering::SeqCst), 1);
    assert_eq!(c.prepared.load(Ordering::SeqCst), 1);
    assert_eq!(c.authorized.load(Ordering::SeqCst), 1);
    assert_eq!(c.passed.load(Ordering::SeqCst), 1);
    assert_eq!(c.actions.load(Ordering::SeqCst), 0);
    send(
        addr,
        "form",
        "Precognition: true\r\nPrecognition-Validate-Only: name\r\n",
        r#"{"name":"Ada"}"#,
        204,
    )
    .await;
    send(
        addr,
        "form",
        "Precognition: true\r\nPrecognition-Validate-Only:\r\n",
        "{}",
        204,
    )
    .await;
    assert_eq!(c.passed.load(Ordering::SeqCst), 1);
    let invalid = send(addr, "form", PRE, "{}", 422).await;
    assert!(invalid.contains("precognition: true\r\n"));
    assert!(!invalid.contains("precognition-success:"));
    send(
        addr,
        "form",
        "Precognition: true\r\nX-Denied: yes\r\n",
        VALID,
        403,
    )
    .await;
    for path in ["plain", "closure", "injected", "view", "api-response"] {
        send(addr, path, PRE, VALID, 204).await;
    }
    assert_eq!(c.actions.load(Ordering::SeqCst), 0);
    let normal = send(addr, "form", "", VALID, 200).await;
    assert!(normal.contains("vary: Precognition\r\n"));
    assert!(!normal.contains("precognition: true"));
    send(addr, "raw", PRE, VALID, 200).await;
    send(addr, "form", "Precognition: True\r\n", VALID, 200).await;
    assert_eq!(c.actions.load(Ordering::SeqCst), 3);
    send(addr, "form", "", r#"{"name":"Ada"}"#, 422).await;
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn precognition_decorates_middleware_short_circuits_using_first_vary_value() {
    let (addr, server) = start(Arc::default()).await;
    for (vary, expected) in [
        ("*", "*, Precognition"),
        ("Precognition", "Precognition, Precognition"),
        ("Origin", "Origin, Precognition"),
        ("0", "Precognition"),
    ] {
        let response = send(
            addr,
            "form",
            &format!("{PRE}X-Early: {vary}\r\n"),
            VALID,
            403,
        )
        .await;
        assert!(
            response.contains(&format!("vary: {expected}\r\n")),
            "{response}"
        );
        assert!(!response.contains("precognition-success:"));
        assert!(!response.contains("Second"));
    }
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn precognition_state_is_isolated_across_concurrent_requests_and_apps() {
    let a = Arc::new(Counters::default());
    let b = Arc::new(Counters::default());
    let (one, s1) = start(a.clone()).await;
    let (two, s2) = start(b.clone()).await;
    for _ in 0..8 {
        tokio::join!(
            send(one, "form", PRE, VALID, 204),
            send(one, "form", "", VALID, 200),
            send(two, "raw", PRE, VALID, 200)
        );
    }
    assert_eq!(a.actions.load(Ordering::SeqCst), 8);
    assert_eq!(b.actions.load(Ordering::SeqCst), 8);
    s1.abort();
    s2.abort();
    let _ = s1.await;
    let _ = s2.await;
}
#[tokio::test]
async fn precognitive_rule_selection_preserves_expanded_paths_and_literal_dots() {
    for (selector, expected) in [
        ("users.*.email", vec!["users.0.email", "users.1.email"]),
        ("users.0.*", vec!["users.0.email"]),
        ("users.*", vec![]),
        (" users.*.email", vec![]),
        (
            "users.0.email,profile.name",
            vec!["profile.name", "users.0.email"],
        ),
        ("", vec![]),
        ("*", vec![]),
        ("profile.name", vec!["profile.name"]),
    ] {
        let native = ::http::Request::builder()
            .header("precognition-validate-only", selector)
            .body(Body::empty())
            .unwrap();
        let request = Request::from(native);
        let rules = request
            .filterPrecognitiveRules(
                RuleSet::parse([
                    ("users.*.email", "required"),
                    (r"profile\.name", "required"),
                ])
                .unwrap(),
            )
            .unwrap();
        let mut validator = Factory::default()
            .make(
                Data::new(json!({"users":[{},{}]})).unwrap(),
                rules,
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
        let _ = validator.validate().await;
        let errors: Value = serde_json::to_value(validator.errors().messages()).unwrap();
        let actual: Vec<_> = errors
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected, "selector: {selector}; errors: {errors}");
    }
}

#[tokio::test]
async fn fixed_php_selector_oracle_matches_rust() {
    use base64::Engine as _;
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/precognition/selectors.json")).unwrap();
    let mut rules = RuleSet::new();
    for attribute in fixture["attributes"].as_array().unwrap() {
        let path = attribute
            .as_str()
            .unwrap()
            .replace('\\', "\\\\")
            .replace('.', "\\.")
            .replace('*', "\\*");
        rules = rules
            .field(path, [openrust::validation::Rule::required()])
            .unwrap();
    }
    for case in fixture["cases"].as_array().unwrap() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(case["header"].as_str().unwrap())
            .unwrap();
        let Ok(header) = HeaderValue::from_bytes(&bytes) else {
            continue;
        };
        let request = Request::from(
            ::http::Request::builder()
                .header("precognition-validate-only", header)
                .body(Body::empty())
                .unwrap(),
        );
        let filtered = request.filterPrecognitiveRules(rules.clone()).unwrap();
        let mut validator = Factory::default()
            .make(
                Data::new(json!({})).unwrap(),
                filtered,
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
        let _ = validator.validate().await;
        let actual = validator.errors().messages();
        let expected: std::collections::BTreeSet<_> = case["selected"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            actual
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            expected,
            "header bytes: {bytes:?}"
        );
    }
}

#[tokio::test]
async fn precognition_does_not_persist_controller_writes_on_mysql_8_4() {
    let url =
        std::env::var("OPENRUST_TEST_MYSQL_URL").expect("Run through tests/mysql_acceptance.py");
    let manager = openrust::database::DatabaseManager::new(
        Arc::new(openrust::config::Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"mysql","connections":{"mysql":{"url":url}}}),
        )]))),
        ".",
    );
    let connection = manager.connection(None).unwrap();
    let version = connection.scalar("SELECT VERSION()", &[]).await.unwrap();
    let openrust::database::Value::Text(version) = version else {
        panic!("Missing MySQL version")
    };
    assert!(version.starts_with("8.4."));
    let table = format!("precognition_{:016x}", fastrand::u64(..));
    connection
        .statement(&format!("CREATE TABLE {table} (name VARCHAR(50))"), &[])
        .await
        .unwrap();
    let c = Arc::new(Counters::default());
    c.persistence
        .set((connection.clone(), table.clone()))
        .ok()
        .unwrap();
    let (addr, server) = start(c).await;
    for _ in 0..3 {
        send(addr, "form", PRE, VALID, 204).await;
    }
    let count = connection
        .scalar(&format!("SELECT COUNT(*) FROM {table}"), &[])
        .await
        .unwrap();
    assert_eq!(count, openrust::database::Value::Integer(0));
    send(addr, "form", "", VALID, 200).await;
    let count = connection
        .scalar(&format!("SELECT COUNT(*) FROM {table}"), &[])
        .await
        .unwrap();
    assert_eq!(count, openrust::database::Value::Integer(1));
    let mut factory = Factory::default();
    factory.setPresenceVerifier(openrust::validation::DatabasePresenceVerifier::new(
        Arc::new(manager),
    ));
    let spec = format!("required|exists:{table}");
    for (selector, valid) in [("users.0.name", true), ("users.1.name", false)] {
        let request = Request::from(
            ::http::Request::builder()
                .header("precognition-validate-only", selector)
                .body(Body::empty())
                .unwrap(),
        );
        let rules = request
            .filterPrecognitiveRules(RuleSet::parse([("users.*.name", spec.as_str())]).unwrap())
            .unwrap();
        let mut validator = factory
            .make(
                Data::new(json!({"users":[{"name":"Ada"},{"name":"Unknown"}]})).unwrap(),
                rules,
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
        assert_eq!(validator.validate().await.is_ok(), valid);
    }

    server.abort();
    let _ = server.await;
    connection
        .statement(&format!("DROP TABLE {table}"), &[])
        .await
        .unwrap();
}

struct SessionProbe;
impl Middleware for SessionProbe {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if request.headers().contains_key("x-mutate") {
                request.session_mut()?.put("mutation", json!(true))?;
            }
            if request.headers().contains_key("x-seed") {
                request.session_mut()?.flash("message", json!("saved"))?;
            }
            let session = request.session()?;
            let data = json!({"message": session.get("message"), "mutation": session.get("mutation"), "token": session.token(), "previous": session.previousUrl()});
            let mut response = next.run(request).await;
            response.parts_mut().headers.insert(
                "x-session-probe",
                HeaderValue::from_str(&data.to_string()).unwrap(),
            );
            Ok(response)
        })
    }
}
fn session_routes() -> Result<(), RoutingError> {
    Route::group(
        RouteGroup::new().middleware(Arc::new(SessionProbe)),
        |router| {
            router.get("/state", plain)?;
            router.r#match(&[Method::GET, Method::POST], "/raw", plain)?;
            router
                .r#match(&[Method::GET, Method::POST], "/precog", plain)?
                .middleware_name("precognitive");
            Ok(())
        },
    )
}
#[tokio::test]
async fn confirmed_precognition_preserves_session_flash_csrf_and_previous_url() {
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([
                ("app".into(), json!({"key":"a".repeat(32)})),
                (
                    "session".into(),
                    json!({"driver":"array", "cookie":"precog_session", "lottery":[0,100]}),
                ),
            ]))
        })
        .withRouting(Routing::default().web(session_routes))
        .create()
        .unwrap();
    let (addr, server) = serve(app).await;
    async fn web(addr: SocketAddr, method: &str, path: &str, headers: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(format!("{method} /{path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n{headers}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut wire = String::new();
        stream.read_to_string(&mut wire).await.unwrap();
        wire
    }
    fn probe(wire: &str) -> Value {
        serde_json::from_str(
            wire.lines()
                .find_map(|line| line.strip_prefix("x-session-probe: "))
                .unwrap(),
        )
        .unwrap()
    }
    let seed = web(addr, "GET", "state", "X-Seed: yes\r\n").await;
    assert!(seed.starts_with("HTTP/1.1 200 "), "{seed}");
    let cookie = seed
        .lines()
        .filter_map(|line| line.strip_prefix("set-cookie: "))
        .find(|cookie| cookie.starts_with("precog_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let headers = format!("Cookie: {cookie}\r\n");
    let token = probe(&seed)["token"].clone();
    let rejected = web(
        addr,
        "POST",
        "precog",
        &format!("{headers}{PRE}Content-Length: 0\r\n"),
    )
    .await;
    assert!(rejected.starts_with("HTTP/1.1 419 "), "{rejected}");
    assert!(rejected.contains("precognition: true\r\n"));
    assert!(!rejected.contains("precognition-success:"));
    let accepted = web(
        addr,
        "POST",
        "precog",
        &format!(
            "{headers}{PRE}X-CSRF-TOKEN: {}\r\nContent-Length: 0\r\n",
            token.as_str().unwrap()
        ),
    )
    .await;
    assert!(accepted.starts_with("HTTP/1.1 204 "), "{accepted}");

    let pre = web(
        addr,
        "GET",
        "precog",
        &format!("{headers}{PRE}X-Mutate: yes\r\n"),
    )
    .await;
    assert!(pre.starts_with("HTTP/1.1 204 "), "{pre}");
    assert!(pre.contains("set-cookie:"));
    let read = web(addr, "GET", "state", &headers).await;
    let data = probe(&read);
    assert_eq!(data["message"], "saved");
    assert_eq!(data["mutation"], Value::Null);
    assert_eq!(data["token"], token);
    assert_eq!(data["previous"], "http://localhost/state");
    let raw = web(
        addr,
        "GET",
        "raw",
        &format!("{headers}{PRE}X-Mutate: yes\r\n"),
    )
    .await;
    assert!(raw.starts_with("HTTP/1.1 200 "), "{raw}");
    let read = web(addr, "GET", "state", &headers).await;
    let data = probe(&read);
    assert_eq!(data["message"], Value::Null);
    assert_eq!(data["mutation"], true);
    assert_eq!(data["previous"], "http://localhost/raw");
    server.abort();
    let _ = server.await;
}

#[tokio::test]
#[ignore = "Requires the pinned browser helper fixture"]
async fn official_frontend_helper_works_in_chromium() {
    let c = Arc::new(Counters::default());
    let (addr, server) = start(c.clone()).await;
    let script = std::env::var("OPENRUST_PRECOGNITION_BROWSER_SCRIPT").unwrap();
    let output = tokio::process::Command::new("node")
        .arg(script)
        .arg(format!("http://{addr}"))
        .output()
        .await
        .unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(c.actions.load(Ordering::SeqCst), 1);
    server.abort();
    let _ = server.await;
}
