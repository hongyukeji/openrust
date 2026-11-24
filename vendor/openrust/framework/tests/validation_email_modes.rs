use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    validation::{Data, Factory, RuleSet},
};
use serde_json::{Value, json};
#[tokio::test]
async fn email_modes_follow_fixed_native_and_egulias_reference() {
    let fixture: Value = if let Some(path) = std::env::var_os("VALIDATION_EMAIL_REFERENCE") {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    } else {
        serde_json::from_str(include_str!("reference/validation-email-modes.json")).unwrap()
    };
    let cases = fixture.get("cases").unwrap_or(&fixture).as_array().unwrap();
    let factory = Factory::default();
    let mut mismatches = Vec::new();
    for case in cases {
        let mode = case["mode"].as_str().unwrap();
        let rule = format!("email:{mode}");
        let mut validator = factory
            .make(
                Data::new(json!({"email":case["value"]})).unwrap(),
                RuleSet::parse([("email", rule.as_str())]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        validator.fake_dns_lookups(true);
        let actual = validator.validate().await.is_ok();
        if actual != case["passes"].as_bool().unwrap() {
            mismatches.push(json!({"case":case,"actual":actual}));
        }
    }
    if let Some(path) = std::env::var_os("EMAIL_MISMATCH_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&mismatches).unwrap()).unwrap();
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches: {}",
        mismatches.len(),
        mismatches
            .into_iter()
            .take(5)
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

use hickory_resolver::{
    TokioResolver,
    config::{NameServerConfigGroup, ResolverConfig},
    name_server::TokioConnectionProvider,
    proto::{
        op::{Message, MessageType, ResponseCode},
        rr::{
            Name, RData, Record, RecordType,
            rdata::{A, AAAA, MX},
        },
    },
};
use openrust::contracts::validation::{DataAwareRule, ValidationRule, ValidatorAwareRule};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    support::facades::Validator as ValidatorFacade,
    validation::{
        Rule, RuleBuilder, RuleContext, RuleFailure, ValidationFuture, ValidationResult,
        ValidatorHandle, rules::Email,
    },
};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream, UdpSocket},
    task::JoinHandle,
};
#[derive(Default)]
struct Allowed {
    allowed: bool,
    parent: Option<ValidatorHandle>,
}
impl DataAwareRule for Allowed {
    fn set_data(&mut self, data: &Data) -> ValidationResult<()> {
        self.allowed = data.as_value()["allowed"] == true;
        Ok(())
    }
}
impl ValidatorAwareRule for Allowed {
    fn set_validator(&mut self, parent: ValidatorHandle) -> ValidationResult<()> {
        self.parent = Some(parent);
        Ok(())
    }
}
impl ValidationRule for Allowed {
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext<'_>,
        _: &'a str,
        _: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
        Box::pin(async move {
            assert_eq!(
                self.parent
                    .as_ref()
                    .unwrap()
                    .with(|parent| Ok(parent.get_data().as_value().clone()))?,
                context.data.as_value().clone()
            );
            if context.data.as_value()["inject"] == true {
                self.parent.as_ref().unwrap().with(|parent| {
                    parent.errors_mut().add("side", "嵌套验证器错误。");
                    Ok(())
                })?;
            }
            Ok(if self.allowed {
                Vec::new()
            } else {
                vec![RuleFailure::new("需要允许邮箱域名。")]
            })
        })
    }
}
fn fluent(mode: &str) -> Email {
    match mode {
        "rfc" | "unknown" => Email::new().rfc_compliant(false),
        "strict" => Email::new().strict(),
        "dns" => Email::new().validate_mx_record(),
        "spoof" => Email::new().prevent_spoofing(),
        "filter" => Email::new().with_native_validation(false),
        "filter_unicode" => Email::new().with_native_validation(true),
        "rfc,strict" => Email::new().rfc_compliant(false).strict(),
        "rfc,dns,spoof" => Email::new()
            .rfc_compliant(false)
            .validate_mx_record()
            .prevent_spoofing(),
        _ => unreachable!(),
    }
}
fn builder(mode: &str) -> Rule {
    match mode {
        "rfc" | "unknown" => Rule::email().rfc_compliant(false),
        "strict" => Rule::email().rfc_compliant(true),
        "dns" => Rule::email().validate_mx_record(),
        "spoof" => Rule::email().prevent_spoofing(),
        "filter" => Rule::email().with_native_validation(false),
        "filter_unicode" => Rule::email().with_native_validation(true),
        "rfc,strict" => Rule::email().rfc_compliant(false).rfc_compliant(true),
        "rfc,dns,spoof" => Rule::email()
            .rfc_compliant(false)
            .validate_mx_record()
            .prevent_spoofing(),
        _ => unreachable!(),
    }
}
fn submit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if let Some(value) = request.header("x-fake")? {
            ValidatorFacade::fake_dns_lookups(value == "yes")?;
        }
        let mode: String =
            serde_json::from_str(request.header("x-mode")?.unwrap_or("\"rfc\"")).unwrap();
        let rule = match request.header("x-entry")?.unwrap_or("string") {
            "typed" => fluent(&mode).into(),
            "builder" => builder(&mode),
            "custom" => Email::new()
                .rfc_compliant(false)
                .rules([Rule::aware(Allowed::default())])
                .into(),
            "custom-builder" => Rule::email().rules([Rule::aware(Allowed::default())]),
            _ => Rule::parse(&format!("email:{mode}"))?,
        };
        let rules = RuleSet::new().field("email", [Rule::bail(), rule])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::OK, data)
    })
}
fn routes(_: &Application) -> Result<Router, openrust::routing::RoutingError> {
    let mut router = Router::new();
    router.post("/email", submit)?;
    Ok(router)
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn serve(
    resolver: TokioResolver,
    fake: bool,
) -> (SocketAddr, JoinHandle<()>, tempfile::TempDir, Arc<Factory>) {
    use axum::handler::Handler as _;
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .with_middleware(|middleware| {
            middleware.skip_trim_strings_when(|_| true);
            middleware.convert_empty_strings_to_null(|_| true);
        })
        .with_routing(Routing::new(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let factory = app.make::<Factory>().await.unwrap();
    factory.fake_dns_lookups(fake);
    factory.resolver(move |mut validator| {
        validator.set_dns_resolver(resolver.clone());
        Ok(validator)
    });
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let task = tokio::spawn(async move {
        axum::serve(listener, transport.with_state(kernel).into_make_service())
            .await
            .unwrap();
    });
    (address, task, root, factory)
}
async fn send(
    address: SocketAddr,
    mode: &str,
    entry: &str,
    data: Value,
    extra: &str,
) -> (u16, Value) {
    tokio::time::timeout(Duration::from_secs(10),async{
 let mode=serde_json::to_string(mode).unwrap();let body=data.to_string();let mut socket=TcpStream::connect(address).await.unwrap();let wire=format!("POST /email HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: application/json\r\nX-Mode: {mode}\r\nX-Entry: {entry}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());socket.write_all(wire.as_bytes()).await.unwrap();let mut response=String::new();socket.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let data=serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();(status,data)
 }).await.unwrap()
}
async fn dns_server() -> (
    TokioResolver,
    JoinHandle<()>,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
) {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let failing = Arc::new(AtomicBool::new(true));
    let state = failing.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let server = tokio::spawn(async move {
        let mut buffer = [0; 4096];
        loop {
            let (size, peer) = socket.recv_from(&mut buffer).await.unwrap();
            let request = Message::from_vec(&buffer[..size]).unwrap();
            let query = request.queries()[0].clone();
            let name = query.name().to_ascii().to_ascii_lowercase();
            count.fetch_add(1, Ordering::Relaxed);
            if name == "timeout.mail." {
                continue;
            }
            let mut response = Message::new();
            response
                .set_id(request.id())
                .set_message_type(MessageType::Response)
                .set_recursion_desired(true)
                .set_recursion_available(true)
                .add_query(query.clone());
            let fail = name == "fail.mail." && query.query_type() == RecordType::A
                || name == "aaaa-fail.mail." && query.query_type() == RecordType::AAAA
                || name == "recover.mail." && state.load(Ordering::Relaxed);
            if fail {
                response.set_response_code(ResponseCode::ServFail);
            } else {
                let data = match (name.as_str(), query.query_type()) {
                    (
                        "a.mail." | "parent.mail." | "aaaa-fail.mail." | "recover.mail.",
                        RecordType::A,
                    ) => Some(RData::A(A("192.0.2.1".parse().unwrap()))),
                    ("aaaa.mail.", RecordType::AAAA) => {
                        Some(RData::AAAA(AAAA("2001:db8::1".parse().unwrap())))
                    }
                    ("mx.mail." | "fail.mail.", RecordType::MX) => Some(RData::MX(MX::new(
                        10,
                        Name::from_ascii("mx.target.mail.").unwrap(),
                    ))),
                    ("null.mail." | "child.parent.mail.", RecordType::MX) => {
                        Some(RData::MX(MX::new(0, Name::root())))
                    }
                    ("null.mail.", RecordType::A) => {
                        Some(RData::A(A("192.0.2.1".parse().unwrap())))
                    }
                    _ => None,
                };
                if let Some(data) = data {
                    response.add_answer(Record::from_rdata(query.name().clone(), 0, data));
                }
            }
            socket
                .send_to(&response.to_vec().unwrap(), peer)
                .await
                .unwrap();
        }
    });
    let config = ResolverConfig::from_parts(
        None,
        Vec::new(),
        NameServerConfigGroup::from_ips_clear(&[address.ip()], address.port(), true),
    );
    let mut builder =
        TokioResolver::builder_with_config(config, TokioConnectionProvider::default());
    builder.options_mut().timeout = Duration::from_millis(100);
    builder.options_mut().attempts = 1;
    builder.options_mut().cache_size = 0;
    (builder.build(), server, failing, calls)
}
#[tokio::test]
async fn all_email_modes_and_real_dns_run_over_http_with_application_isolated_fakes() {
    assert!(ValidatorFacade::fake_dns_lookups(true).is_err());
    let (resolver, dns, recover, calls) = dns_server().await;
    let (fake, server, root, factory) = serve(resolver.clone(), true).await;
    let (real, real_server, real_root, real_factory) = serve(resolver.clone(), false).await;
    let fixture: Value =
        serde_json::from_str(include_str!("reference/validation-email-modes.json")).unwrap();
    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        for entry in if index % 31 == 0
            && [
                "rfc",
                "strict",
                "dns",
                "spoof",
                "filter",
                "filter_unicode",
                "rfc,strict",
                "rfc,dns,spoof",
                "unknown",
            ]
            .contains(&case["mode"].as_str().unwrap())
        {
            vec!["string", "typed", "builder"]
        } else {
            vec!["string"]
        } {
            let (status, data) = send(
                fake,
                case["mode"].as_str().unwrap(),
                entry,
                json!({"email":case["value"]}),
                "",
            )
            .await;
            assert_eq!(
                status,
                if case["passes"] == true { 200 } else { 422 },
                "{case}/{entry}: {data}"
            );
        }
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    for (host, expected) in [
        ("mx.mail", 200),
        ("a.mail", 200),
        ("aaaa.mail", 200),
        ("none.mail", 422),
        ("null.mail", 422),
        ("fail.mail", 422),
        ("aaaa-fail.mail", 200),
        ("timeout.mail", 422),
        ("child.parent.mail", 200),
        ("recover.mail", 422),
    ] {
        let (status, data) = send(
            real,
            "rfc,dns,spoof",
            "builder",
            json!({"email":format!("user@{host}")}),
            "",
        )
        .await;
        assert_eq!(status, expected, "{host}: {data}");
    }
    let before = calls.load(Ordering::Relaxed);
    assert_eq!(
        send(
            real,
            "rfc,dns,spoof",
            "builder",
            json!({"email":"bad..local@mx.mail"}),
            ""
        )
        .await
        .0,
        422
    );
    assert!(calls.load(Ordering::Relaxed) > before);
    let before = calls.load(Ordering::Relaxed);
    assert_eq!(
        send(
            real,
            "rfc,dns,spoof",
            "builder",
            json!({"email":"bad\r@mx.mail"}),
            ""
        )
        .await
        .0,
        422
    );
    assert_eq!(calls.load(Ordering::Relaxed), before);
    recover.store(false, Ordering::Relaxed);
    assert_eq!(
        send(
            real,
            "dns",
            "typed",
            json!({"email":"user@recover.mail"}),
            ""
        )
        .await
        .0,
        200
    );
    for entry in ["custom", "custom-builder"] {
        for (email, allowed, expected, count) in [
            ("user@mx.mail", true, 200, 0),
            ("user@mx.mail", false, 422, 1),
            ("not-email", false, 422, 2),
        ] {
            let (status, data) = send(
                real,
                "rfc",
                entry,
                json!({"email":email,"allowed":allowed}),
                "",
            )
            .await;
            assert_eq!(status, expected, "{data}");
            if count > 0 {
                assert_eq!(
                    data["errors"]["email"].as_array().unwrap().len(),
                    count,
                    "{data}"
                );
            }
        }
    }
    let (status, data) = send(
        real,
        "rfc",
        "custom",
        json!({"email":"user@mx.mail","allowed":true,"inject":true}),
        "",
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(data["errors"]["email"], json!(["嵌套验证器错误。"]));
    assert!(data["errors"].get("side").is_none());
    for _ in 0..12 {
        let (a, b) = tokio::join!(
            send(fake, "dns", "typed", json!({"email":"user@none.mail"}), ""),
            send(
                real,
                "dns",
                "builder",
                json!({"email":"user@none.mail"}),
                ""
            )
        );
        assert_eq!(a.0, 200);
        assert_eq!(b.0, 422);
    }
    assert_eq!(
        send(
            fake,
            "dns",
            "string",
            json!({"email":"user@none.mail"}),
            "X-Fake: no\r\n"
        )
        .await
        .0,
        422
    );
    assert_eq!(
        send(
            fake,
            "dns",
            "string",
            json!({"email":"user@none.mail"}),
            "X-Fake: yes\r\n"
        )
        .await
        .0,
        200
    );
    assert_eq!(
        send(real, "dns", "string", json!({"email":"user@none.mail"}), "")
            .await
            .0,
        422
    );
    let mut existing = real_factory
        .make(
            Data::new(json!({"email":"user@none.mail"})).unwrap(),
            RuleSet::parse([("email", "email:dns")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(existing.validate().await.is_err());
    real_factory.fake_dns_lookups(true);
    assert!(existing.validate().await.is_ok());
    existing.fake_dns_lookups(false);
    assert!(existing.validated().is_err());
    assert!(existing.validate().await.is_err());
    factory.fake_dns_lookups(false);
    server.abort();
    real_server.abort();
    dns.abort();
    let _ = tokio::join!(server, real_server, dns);
    drop((root, real_root));
}

#[tokio::test]
async fn nested_email_rules_reuse_the_same_factory_resolver_and_fresh_value_labels() {
    use std::collections::BTreeMap;
    let factory = Factory::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    factory.resolver(move |mut validator| {
        counter.fetch_add(1, Ordering::SeqCst);
        validator.set_value_names(BTreeMap::from([(
            "other".into(),
            BTreeMap::from([("blocked".into(), "factory".into())]),
        )]));
        Ok(validator)
    });
    let email = Email::new().rules([Rule::parse("declined_if:other,blocked").unwrap()]);
    let mut validator = factory
        .make(
            Data::new(json!({"email":"user@example.com","other":"blocked"})).unwrap(),
            RuleSet::new().field("email", [email.into()]).unwrap(),
            BTreeMap::from([("email.declined_if".into(), ":value".into())]),
            BTreeMap::new(),
        )
        .unwrap();
    validator.set_value_names(BTreeMap::from([(
        "other".into(),
        BTreeMap::from([("blocked".into(), "parent".into())]),
    )]));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(validator.validate().await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        validator.errors().get("email", None).into_messages(),
        vec!["factory"]
    );
    let mut later = factory
        .make(
            Data::new(json!({"email":"user@example.com"})).unwrap(),
            RuleSet::new()
                .field(
                    "email",
                    [Email::new()
                        .rules([Rule::parse("later_email_policy").unwrap()])
                        .into()],
                )
                .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let late_calls = Arc::new(AtomicUsize::new(0));
    let observed = late_calls.clone();
    factory.extend(
        "later_email_policy",
        move |_, _, _, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        },
        None,
    );
    assert!(later.validate().await.is_ok());
    assert_eq!(late_calls.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}
