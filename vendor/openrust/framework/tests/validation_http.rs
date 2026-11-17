use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    auth::access::{AuthorizationError, Response as Authorization},
    contracts::{
        container::Container as _, foundation::Application as _, http::Kernel as _,
        validation::Validator as _,
    },
    database::DatabaseManager,
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, Kernel, ValidatedFormRequest},
    },
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::{ControllerDispatcher, ResourceAction, ResourceController, RoutingError},
    support::facades::{Lang, Route, Validator},
    support::{ProviderResult, ServiceProvider},
    validation::{
        Data, Rule, RuleSet, ValidatedInput, ValidationFuture, ValidationResult, rules::File,
    },
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Submit;

impl FormRequest for Submit {
    fn prepareForValidation(&mut self, data: &mut Data) -> ValidationResult<()> {
        let mut input = data.as_value().clone();
        if let Some(name) = input.get("name").and_then(Value::as_str) {
            input["name"] = name.trim().into();
        }
        data.replace(input)
    }

    fn authorize<'a>(
        &'a self,
        request: &'a mut Request,
        _: &'a Data,
    ) -> ValidationFuture<'a, Result<(), AuthorizationError>> {
        Box::pin(async move {
            if request.headers().contains_key("x-denied") {
                return Err(AuthorizationError::Denied(Authorization::deny(
                    "Submission is forbidden.",
                )));
            }
            Ok(())
        })
    }

    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([
            ("name", "required|string|min:2"),
            ("users", "required|array"),
            ("users.*.email", "required|string"),
            ("users.*.score", "nullable|integer|min:0"),
            ("description", "sometimes|nullable|string"),
        ])
    }

    fn passedValidation(
        &mut self,
        request: &mut Request,
        _: &ValidatedInput,
    ) -> ValidationResult<()> {
        request.parts_mut().extensions.insert("passed");
        Ok(())
    }
}

async fn submit(request: &mut Request) -> Result<Response, HttpError> {
    let input = Submit.validateResolved(request).await?;
    Response::json(
        StatusCode::OK,
        json!({
            "input":input.into_value(),
            "hook":request.parts().extensions.get::<&'static str>()
        }),
    )
}

fn form(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let locale = request.route("locale").unwrap().to_owned();
        Lang::withLocale(&locale, submit(request))
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?
    })
}

fn manual(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::new(request.input(None).await?.unwrap())?;
        let rules = RuleSet::parse([("name", "required|string")])?;
        let input = Validator::make(data, rules)?.validate().await?;
        Response::json(StatusCode::OK, input.into_value())
    })
}

fn presence(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.route("mode").unwrap();
        let rules = match mode {
            "fluent" => RuleSet::new()
                .field(
                    "ids",
                    [Rule::exists("directory.accounts", Some("account_id"))
                        .whereIn("tenant", vec![7.into(), 8.into()])
                        .withoutTrashed(None)
                        .using(|query| query.r#where("account_id", ">", 0))
                        .into()],
                )?
                .field(
                    "email",
                    [Rule::unique("directory.accounts", Some("email"))
                        .ignore(1, Some("account_id"))
                        .r#where("tenant", 7)
                        .withoutTrashed(None)
                        .into()],
                )?,
            "tenants" => RuleSet::parse([("tenant", "exists:directory.accounts")])?,
            "update" => {
                RuleSet::parse([("email", "unique:directory.accounts,email,[id],account_id")])?
            }
            "csv" => RuleSet::parse([
                (
                    "alias",
                    r#"unique:directory.references,alias,NULL,reference,namespace,"team,west""#,
                ),
                (
                    "ref",
                    r#"exists:directory.references,reference,namespace,"team,west""#,
                ),
            ])?,
            "quoted-ignore" => RuleSet::parse([(
                "alias",
                r#"unique:directory.references,alias,"ref""quote",reference"#,
            )])?,
            "slash-ignore" => RuleSet::parse([(
                "alias",
                r#"unique:directory.references,alias,"ref\\slash",reference"#,
            )])?,
            "padded-ignore" => {
                RuleSet::parse([("alias", "unique:directory.references,alias,001,reference")])?
            }
            "dynamic-ignore" => RuleSet::parse([(
                "alias",
                "unique:directory.references,alias,[meta.id],reference",
            )])?,
            "numeric-ignore" => {
                RuleSet::parse([("alias", "unique:directory.references,alias,+1,reference")])?
            }
            "numbers" => RuleSet::parse([("numbers", "exists:directory.references,number")])?,
            "types" => RuleSet::parse([
                ("exists", "exists:directory.absent,id"),
                ("unique", "unique:directory.absent,id"),
            ])?,
            "broken" => RuleSet::parse([("id", "integer|exists:directory.absent,account_id")])?,
            _ => RuleSet::parse([
                (
                    "email",
                    "unique:accounts,NULL,NULL,id,tenant,7,deleted_at,NULL",
                ),
                (
                    "users.*.account_id",
                    "exists:directory.accounts,NULL,tenant,!8,deleted_at,NULL",
                ),
            ])?,
        };
        let data = Data::new(request.input(None).await?.unwrap())?;
        let input = Validator::make(data, rules)?.validate().await?;
        Response::json(StatusCode::OK, input.into_value())
    })
}

struct Upload(String);

impl FormRequest for Upload {
    fn prepareForValidation(&mut self, data: &mut Data) -> ValidationResult<()> {
        let mut value = data.as_value().clone();
        value["title"] = json!("prepared");
        data.replace(value)
    }

    fn rules(&self) -> ValidationResult<RuleSet> {
        match self.0.as_str() {
            "dimensions" => RuleSet::parse([(
                "attachments.*",
                "required|dimensions:min_width=100,max_height=100,min_ratio=0.99,max_ratio=101/100,ratio=1.009",
            )]),
            "dimensions-heic" => {
                RuleSet::parse([("attachments.*", "required|dimensions:width=112,height=112")])
            }
            "dimensions-icon" => {
                RuleSet::parse([("attachments.*", "required|dimensions:width=256,height=256")])
            }
            "dimensions-fluent" => RuleSet::new().field(
                "attachments.*",
                [
                    Rule::required(),
                    File::image(false)
                        .dimensions(
                            Rule::dimensions()
                                .width(100)
                                .height(100)
                                .minHeight(100)
                                .maxWidth(100)
                                .ratioBetween("99/100", 1.01),
                        )
                        .into(),
                ],
            ),
            "svg-dimensions" => RuleSet::parse([(
                "attachments.*",
                "required|image:allow_svg|dimensions:width=999",
            )]),
            "encoding" => RuleSet::parse([("attachments.*", "required|file|encoding:UTF-8")]),
            "encoding-fluent" => RuleSet::new().field(
                "attachments.*",
                [Rule::required(), File::default().encoding("ASCII").into()],
            ),
            "encoding-sjis" => RuleSet::new().field(
                "attachments.*",
                [Rule::required(), File::default().encoding("SJIS").into()],
            ),
            "encoding-utf16" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:UTF-16BE")])
            }
            "encoding-utf32" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:UTF-32LE")])
            }
            "encoding-utf16-bom" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:UTF-16")])
            }
            "encoding-utf32-bom" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:UTF-32")])
            }
            "mime-json" => RuleSet::parse([(
                "attachments.*",
                "required|file|mimes:json|mimetypes:application/json",
            )]),
            "mime-text" => RuleSet::parse([(
                "attachments.*",
                "required|file|mimes:txt|mimetypes:text/plain",
            )]),
            "mime-empty" => RuleSet::parse([(
                "attachments.*",
                "required|file|mimetypes:application/x-empty",
            )]),
            "mime-pdf" => RuleSet::parse([(
                "attachments.*",
                "required|file|mimes:pdf|mimetypes:application/pdf",
            )]),
            "encoding-cp932" => RuleSet::parse([("attachments.*", "required|file|encoding:CP932")]),
            "encoding-euc-cn" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:EUC-CN")])
            }
            "encoding-ucs2" => RuleSet::parse([("attachments.*", "required|file|encoding:UCS-2")]),
            "encoding-ucs4" => RuleSet::parse([("attachments.*", "required|file|encoding:UCS-4")]),
            "encoding-base64" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:BASE64")])
            }
            "encoding-qp" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:Quoted-Printable")])
            }
            "encoding-entities" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:HTML-ENTITIES")])
            }
            "encoding-uu" => RuleSet::parse([("attachments.*", "required|file|encoding:UUENCODE")]),
            "strict-base64" => RuleSet::new().field("value", [Rule::required(), Rule::base64()]),
            "strict-base64-string" => RuleSet::parse([("value", "required|base64")]),
            "encoding-utf7" => RuleSet::parse([("attachments.*", "required|file|encoding:UTF-7")]),
            "encoding-utf7-imap" => RuleSet::new().field(
                "attachments.*",
                [
                    Rule::required(),
                    File::default().encoding("UTF7-IMAP").into(),
                ],
            ),
            "encoding-input" => RuleSet::parse([("metadata", "required|array|encoding:ASCII")]),
            "encoding-cp850" => RuleSet::parse([("attachments.*", "required|file|encoding:CP850")]),
            "encoding-armscii" => RuleSet::new().field(
                "attachments.*",
                [
                    Rule::required(),
                    File::default().encoding("ArmSCII-8").into(),
                ],
            ),
            "encoding-windows-1254" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:Windows-1254")])
            }
            "encoding-big-5" => RuleSet::parse([("attachments.*", "required|file|encoding:BIG-5")]),
            "encoding-cp950" => RuleSet::new().field(
                "attachments.*",
                [Rule::required(), File::default().encoding("CP950").into()],
            ),
            "encoding-euc-kr" => {
                RuleSet::parse([("attachments.*", "required|file|encoding:EUC-KR")])
            }
            "encoding-uhc" => RuleSet::new().field(
                "attachments.*",
                [Rule::required(), File::default().encoding("UHC").into()],
            ),
            "svg" => RuleSet::parse([("attachments.*", "required|image:allow_svg")]),
            "image" => RuleSet::parse([("attachments.*", "required|image")]),
            "fluent" => RuleSet::parse([
                ("attachments", "required|array"),
                ("title", "required|string"),
            ])?
            .field(
                "attachments.*",
                [
                    Rule::required(),
                    File::types(["image/*", "png"])
                        .extensions(["PNG"])
                        .min(1)
                        .max("0.002mb")
                        .into(),
                ],
            ),
            _ => RuleSet::parse([
                ("attachments", "required|array"),
                (
                    "attachments.*",
                    "required|nullable|file|mimes:png|mimetypes:image/*|extensions:png|size:1|between:1,2",
                ),
                ("title", "required|string"),
            ]),
        }
    }
}

fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mode = request.route("mode").unwrap().to_owned();
        let input = if mode == "manual" {
            let data = Data::fromRequest(request).await?;
            let rules = RuleSet::parse([("attachments.0.title", "required|string")])?.field(
                "attachments.0.image",
                [Rule::required(), File::image(false).max("2mb").into()],
            )?;
            Validator::make(data, rules)?.validate().await?
        } else {
            Upload(mode.clone()).validateResolved(request).await?
        };
        let path = if mode == "manual" {
            "attachments.0.image"
        } else {
            "attachments.0"
        };
        let file = input.file(path);
        Response::json(
            StatusCode::OK,
            json!({
                "input": input.all(),
                "filename": file.map(|file| file.getClientOriginalName()),
                "size": file.map(|file| file.getSize()),
                "unvalidated": input.file("unvalidated").is_some(),
            }),
        )
    })
}

#[derive(Default)]
struct Injected {
    stage: u8,
    factory: usize,
}

impl FormRequest for Injected {
    fn prepareForValidation(&mut self, data: &mut Data) -> ValidationResult<()> {
        assert_eq!(self.stage, 0);
        self.stage = 1;
        let mut input = data.as_value().clone();
        if let Some(name) = input.get("name").and_then(Value::as_str) {
            input["name"] = name.trim().into();
        }
        data.replace(input)
    }

    fn authorize<'a>(
        &'a self,
        request: &'a mut Request,
        _: &'a Data,
    ) -> ValidationFuture<'a, Result<(), AuthorizationError>> {
        Box::pin(async move {
            assert_eq!(self.stage, 1);
            if request.headers().contains_key("x-denied") {
                return Err(AuthorizationError::Denied(Authorization::deny(
                    "Injected form denied.",
                )));
            }
            Ok(())
        })
    }

    fn rules(&self) -> ValidationResult<RuleSet> {
        assert_eq!(self.stage, 1);
        RuleSet::parse([("name", "required|string|min:2")])
    }

    fn passedValidation(
        &mut self,
        request: &mut Request,
        _: &ValidatedInput,
    ) -> ValidationResult<()> {
        assert_eq!(self.stage, 1);
        self.stage = 2;
        request.parts_mut().extensions.insert("validated");
        Ok(())
    }
}

async fn injected(form: ValidatedFormRequest<Injected>) -> Result<Response, HttpError> {
    Response::json(
        StatusCode::OK,
        json!({
            "name":form.validated().get("name"),
            "stage":form.form().stage,
            "factory":form.form().factory
        }),
    )
}

struct InjectionController(Arc<AtomicUsize>);

impl InjectionController {
    fn store<'a>(
        &'a self,
        request: &'a mut Request,
        form: ValidatedFormRequest<Injected>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::Relaxed);
            Response::json(
                StatusCode::OK,
                json!({
                    "name":form.validated().get("name"),
                    "stage":form.form().stage,
                    "factory":form.form().factory,
                    "workspace":request.route("workspace"),
                    "hook":request.parts().extensions.get::<&'static str>(),
                }),
            )
        })
    }
}

struct InjectionProvider {
    calls: Arc<AtomicUsize>,
    forms: Arc<AtomicUsize>,
}

impl ServiceProvider for InjectionProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let calls = self.calls.clone();
            app.container()
                .singleton::<InjectionController, _>(move |_| {
                    Ok(Arc::new(InjectionController(calls.clone())))
                })?;
            let forms = self.forms.clone();
            app.container()
                .bindAsync::<Injected, _>(move |_| {
                    let forms = forms.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        let factory = forms.fetch_add(1, Ordering::Relaxed) + 1;
                        Ok(Arc::new(Injected { stage: 0, factory }))
                    })
                })
                .await?;
            Ok(())
        })
    }
}

fn inject_routes() -> Result<(), RoutingError> {
    Route::r#match(&[Method::POST], "/injected", injected)?;
    Route::r#match(
        &[Method::POST],
        "/injected/{workspace}",
        ControllerDispatcher::withFormRequest(InjectionController::store),
    )?;
    Route::apiResource(
        "injected-posts",
        ResourceController::new()
            .only(&[ResourceAction::Store])
            .actionWithFormRequest(ResourceAction::Store, InjectionController::store),
    )
}

fn register() -> Result<(), RoutingError> {
    Route::r#match(&[Method::POST], "/submit/{locale}", form)?;
    Route::r#match(&[Method::POST], "/manual", manual)?;
    Route::r#match(&[Method::POST], "/presence/{mode}", presence)?;
    Route::r#match(&[Method::POST], "/upload/{mode}", upload)?;
    Ok(())
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(incoming).await
}

async fn start(root: PathBuf) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root)
        .withRouting(Routing::default().api(register))
        .create()
        .unwrap();
    serve(app).await
}

async fn serve(app: Application) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server)
}

async fn send(address: SocketAddr, path: &str, body: Value, denied: bool, status: u16) -> Value {
    let denied = if denied { "X-Denied: yes\r\n" } else { "" };
    send_bytes(
        address,
        path,
        "application/json",
        body.to_string().as_bytes(),
        denied,
        status,
    )
    .await
}

async fn send_bytes(
    address: SocketAddr,
    path: &str,
    content_type: &str,
    body: &[u8],
    headers: &str,
    status: u16,
) -> Value {
    timeout(Duration::from_secs(5), async {
        let mut socket = TcpStream::connect(address).await.unwrap();
        let wire = format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n", body.len());
        socket.write_all(wire.as_bytes()).await.unwrap();
        socket.write_all(body).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status} ")), "{response}");
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.contains("content-type: application/json"));
        serde_json::from_str(body).unwrap()
    }).await.unwrap()
}

#[tokio::test]
async fn form_requests_validate_and_translate_concurrent_real_http_requests() {
    let root = std::env::temp_dir().join(format!("openrust-validation-{:016x}", fastrand::u64(..)));
    std::fs::create_dir_all(root.join("lang/zh_CN")).unwrap();
    std::fs::write(
        root.join("lang/zh_CN/validation.json"),
        r#"{"required":"请填写 :attribute。","attributes":{"name":"名称"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("lang/zh_CN.json"),
        r#"{"(and :count more error)":"（另有 :count 条错误）"}"#,
    )
    .unwrap();
    let (address, server) = start(root.clone()).await;

    let (en, zh, deny) = tokio::join!(
        send(
            address,
            "/api/submit/en",
            json!({"users":[{"score":null}]}),
            false,
            422
        ),
        send(
            address,
            "/api/submit/zh_CN",
            json!({"users":[{"score":null}]}),
            false,
            422
        ),
        send(address, "/api/submit/en", json!({}), true, 403),
    );
    assert_eq!(
        en,
        json!({
            "message":"The name field is required. (and 1 more error)",
            "errors":{"name":["The name field is required."],"users.0.email":["The users.0.email field is required."]}
        })
    );
    assert_eq!(zh["errors"]["name"], json!(["请填写 名称。"]));
    assert_eq!(zh["message"], "请填写 名称。 （另有 1 条错误）");
    assert_eq!(deny, json!({"message":"Submission is forbidden."}));

    let accepted = send(
        address,
        "/api/submit/en",
        json!({
            "name":"  Ada  ",
            "users":[{"email":"ada@example.test","score":null,"secret":"ignored"}],
            "description":null,"extra":true
        }),
        false,
        200,
    )
    .await;
    assert_eq!(
        accepted,
        json!({
            "input":{"name":"Ada","users":[{"email":"ada@example.test","score":null}],"description":null},
            "hook":"passed"
        })
    );
    let manual = send(
        address,
        "/api/manual",
        json!({"name":"Ada","extra":true}),
        false,
        200,
    )
    .await;
    assert_eq!(manual, json!({"name":"Ada"}));
    server.abort();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn custom_rules_respect_presence_nullable_bail_and_custom_messages() {
    use openrust::{
        contracts::validation::{Factory as _, ValidationRule},
        validation::{Factory, Rule, RuleContext, RuleFailure},
    };
    use std::collections::BTreeMap;

    struct Verify;

    impl ValidationRule for Verify {
        fn validate<'a>(
            &'a self,
            context: &'a RuleContext<'_>,
            attribute: &'a str,
            value: Option<&'a Value>,
        ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                assert_eq!(attribute, "checked");
                assert_eq!(context.data.as_value()["checked"], value.unwrap().clone());
                Ok(vec![RuleFailure::new("The :attribute is already used.")])
            })
        }
    }

    let data = Data::new(json!({"null":null,"checked":"Ada","blocked":42})).unwrap();
    let rules = RuleSet::new()
        .field(
            "absent",
            [Rule::sometimes(), Rule::required(), Rule::custom(Verify)],
        )
        .unwrap()
        .field("null", [Rule::nullable(), Rule::custom(Verify)])
        .unwrap()
        .field(
            "blocked",
            [Rule::bail(), Rule::string(), Rule::custom(Verify)],
        )
        .unwrap()
        .field("checked", [Rule::custom(Verify)])
        .unwrap();
    let messages = BTreeMap::from([("blocked.string".into(), "Supply a text :attribute.".into())]);
    let attributes = BTreeMap::from([
        ("blocked".into(), "description".into()),
        ("checked".into(), "name".into()),
    ]);
    let mut validator = Factory::default()
        .make(data, rules, messages, attributes)
        .unwrap();
    assert!(validator.fails().await.unwrap());
    assert_eq!(
        validator.errors().get("blocked"),
        ["Supply a text description."]
    );
    assert_eq!(
        validator.errors().get("checked"),
        ["The name is already used."]
    );
    assert!(validator.errors().get("absent").is_empty());
    assert!(validator.errors().get("null").is_empty());
}

fn presence_config(url: &str, prefix: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "database".into(),
        json!({"default":"directory", "connections":{"directory":{
            "url":url,"prefix":prefix,"read":{"url":"mysql://invalid:invalid@127.0.0.1:1/invalid","port":1}
        }}}),
    )])
}

#[tokio::test]
async fn database_validation_uses_mysql_write_connection_over_real_http() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let prefix = format!("validate_{:016x}_", fastrand::u64(..));
    let config = presence_config(&url, &prefix);
    let db = DatabaseManager::new(
        Arc::new(openrust::config::Repository::new(config.clone())),
        ".",
    )
    .connection(None)
    .unwrap();
    let table = format!("{prefix}accounts");
    db.statement(&format!("CREATE TABLE {table} (account_id INT PRIMARY KEY, email VARCHAR(100), tenant INT, deleted_at INT)"), &[])
        .await
        .unwrap();
    db.statement(&format!("INSERT INTO {table} VALUES (1,'owned@example.test',7,NULL), (2,'foreign@example.test',8,NULL), (3,'deleted@example.test',7,1), (4,'second@example.test',7,NULL)"), &[])
        .await
        .unwrap();
    let references = format!("{prefix}references");
    db.statement(&format!("CREATE TABLE {references} (reference VARCHAR(64) PRIMARY KEY, alias VARCHAR(64), namespace VARCHAR(64), number DOUBLE)"), &[])
        .await.unwrap();
    for (reference, alias, namespace, number) in [
        ("001", "padded", "team,west", 1e20),
        ("1", "canonical", "team,east", 1.0),
        ("ref\"quote", "quoted", "team,west", 1.234567890123456),
        ("ref\\slash", "escaped", "team,east", 0.0),
    ] {
        use openrust::database::Value as Binding;
        db.statement(
            &format!("INSERT INTO {references} VALUES (?, ?, ?, ?)"),
            &[
                Binding::from(reference),
                alias.into(),
                namespace.into(),
                Binding::Real(number),
            ],
        )
        .await
        .unwrap();
    }
    let root = std::env::temp_dir().join(&prefix);
    std::fs::create_dir(&root).unwrap();
    let app = Application::configure(root.clone())
        .withConfig(|base, _| {
            Ok(presence_config(
                &std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),
                base.file_name().unwrap().to_str().unwrap(),
            ))
        })
        .withRouting(Routing::default().api(register))
        .create()
        .unwrap();
    let (address, server) = serve(app).await;
    let call = |mode, payload, status| send(address, mode, payload, false, status);
    assert_eq!(
        call(
            "/api/presence/fluent",
            json!({"ids":[1,1,4],"email":"owned@example.test"}),
            200
        )
        .await,
        json!({"ids":[1,1,4],"email":"owned@example.test"}),
    );
    let fluent = call(
        "/api/presence/fluent",
        json!({"ids":[1,3,777],"email":"second@example.test"}),
        422,
    )
    .await;
    assert_eq!(
        fluent["errors"]["ids"],
        json!(["The selected ids is invalid."])
    );
    assert_eq!(
        fluent["errors"]["email"],
        json!(["The email has already been taken."])
    );
    let duplicate = call(
        "/api/presence/strings",
        json!({"email":"owned@example.test","users":[{"account_id":2}]}),
        422,
    )
    .await;
    assert_eq!(
        duplicate["errors"]["email"],
        json!(["The email has already been taken."])
    );
    assert_eq!(
        duplicate["errors"]["users.0.account_id"],
        json!(["The selected users.0.account id is invalid."])
    );
    for email in [
        "foreign@example.test",
        "deleted@example.test",
        "a'b@example.test",
    ] {
        call(
            "/api/presence/strings",
            json!({"email":email,"users":[{"account_id":1}]}),
            200,
        )
        .await;
    }
    call(
        "/api/presence/update",
        json!({"id":1,"email":"owned@example.test"}),
        200,
    )
    .await;
    call(
        "/api/presence/fluent",
        json!({"ids":[],"email":"owned@example.test"}),
        200,
    )
    .await;
    let wrong_type = call("/api/presence/broken", json!({"id":"not-an-integer"}), 422).await;
    assert_eq!(
        wrong_type["errors"]["id"],
        json!(["The id field must be an integer."])
    );
    call("/api/presence/tenants", json!({"tenant":[7,8,7]}), 200).await;
    call("/api/presence/tenants", json!({"tenant":[7,999]}), 422).await;
    call("/api/presence/broken", json!({"id":1}), 500).await;
    let csv = call(
        "/api/presence/csv",
        json!({"alias":"padded","ref":"1"}),
        422,
    )
    .await;
    assert_eq!(
        csv["errors"]["alias"],
        json!(["The alias has already been taken."])
    );
    assert_eq!(
        csv["errors"]["ref"],
        json!(["The selected ref is invalid."])
    );
    call(
        "/api/presence/csv",
        json!({"alias":"canonical","ref":"001"}),
        200,
    )
    .await;
    for (uri, alias) in [
        ("/api/presence/padded-ignore", "padded"),
        ("/api/presence/numeric-ignore", "canonical"),
        ("/api/presence/quoted-ignore", "quoted"),
        ("/api/presence/slash-ignore", "escaped"),
    ] {
        call(uri, json!({"alias":alias}), 200).await;
    }
    call(
        "/api/presence/padded-ignore",
        json!({"alias":"canonical"}),
        422,
    )
    .await;
    call(
        "/api/presence/dynamic-ignore",
        json!({"alias":"padded","meta":{"id":"001"}}),
        200,
    )
    .await;
    call(
        "/api/presence/dynamic-ignore",
        json!({"alias":"escaped","meta":{"id":"ref\\\\slash"}}),
        200,
    )
    .await;
    call(
        "/api/presence/dynamic-ignore",
        json!({"alias":"canonical","meta":{"id":null}}),
        422,
    )
    .await;
    call(
        "/api/presence/dynamic-ignore",
        json!({"alias":"canonical","meta":{"id":[1]}}),
        422,
    )
    .await;
    for numbers in [
        json!([1, 1.0, "1", true]),
        json!([1e20, "1.0E+20"]),
        json!([1.234567890123456, "1.2345678901235"]),
    ] {
        call("/api/presence/numbers", json!({"numbers":numbers}), 200).await;
    }
    call("/api/presence/numbers", json!({"numbers":[0,"-0"]}), 422).await;
    let types = call(
        "/api/presence/types",
        json!({"exists":[[]],"unique":{}}),
        422,
    )
    .await;
    assert_eq!(
        types["errors"]["exists"],
        json!(["The selected exists is invalid."])
    );
    assert_eq!(
        types["errors"]["unique"],
        json!(["The unique has already been taken."])
    );
    server.abort();
    db.statement(&format!("DROP TABLE {table}"), &[])
        .await
        .unwrap();
    db.statement(&format!("DROP TABLE {references}"), &[])
        .await
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

async fn upload_file(
    address: SocketAddr,
    mode: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
    status: u16,
) -> Value {
    let mut body = format!("--upload-boundary\r\nContent-Disposition: form-data; name=\"attachments[]\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--upload-boundary\r\nContent-Disposition: form-data; name=\"unvalidated\"; filename=\"private.txt\"\r\nContent-Type: text/plain\r\n\r\nsecret\r\n--upload-boundary--\r\n");
    send_bytes(
        address,
        &format!("/api/upload/{mode}"),
        "multipart/form-data; boundary=upload-boundary",
        &body,
        "",
        status,
    )
    .await
}

#[tokio::test]
async fn validates_typed_multipart_files_in_form_requests_over_real_http() {
    let (address, server) = start(std::env::temp_dir()).await;
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(1024, 0);
    let accepted = upload_file(address, "rules", "avatar.PNG", "text/plain", &png, 200).await;
    assert_eq!(
        accepted,
        json!({
            "input": {"attachments": [null], "title": "prepared"},
            "filename": "avatar.PNG", "size": 1024, "unvalidated": false,
        })
    );
    upload_file(
        address,
        "fluent",
        "avatar.png",
        "application/octet-stream",
        &png,
        200,
    )
    .await;
    let disguised = upload_file(
        address,
        "rules",
        "avatar.png",
        "image/png",
        b"ordinary text",
        422,
    )
    .await;
    let errors = disguised["errors"]["attachments.0"].as_array().unwrap();
    assert!(
        errors
            .iter()
            .any(|error| error.as_str().unwrap().contains("file of type: png"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.as_str().unwrap().contains("file of type: image/*"))
    );
    let extension = upload_file(address, "rules", "avatar.txt", "image/png", &png, 422).await;
    assert_eq!(
        extension["errors"]["attachments.0"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        extension["errors"]["attachments.0"][0]
            .as_str()
            .unwrap()
            .contains("extensions: png")
    );

    let mut nested = b"--probe\r\nContent-Disposition: form-data; name=\"attachments[0][title]\"\r\n\r\nCover\r\n--probe\r\nContent-Disposition: form-data; name=\"attachments[0][image]\"; filename=\"cover.png\"\r\nContent-Type: text/plain\r\n\r\n".to_vec();
    nested.extend_from_slice(&png);
    nested.extend_from_slice(b"\r\n--probe--\r\n");
    let manual = send_bytes(
        address,
        "/api/upload/manual",
        "multipart/form-data; boundary=probe",
        &nested,
        "",
        200,
    )
    .await;
    assert_eq!(
        manual["input"],
        json!({"attachments":[{"title":"Cover","image":null}]})
    );
    assert_eq!(manual["filename"], "cover.png");

    png.push(0);
    let oversize = upload_file(address, "rules", "avatar.png", "image/png", &png, 422).await;
    assert_eq!(
        oversize["errors"]["attachments.0"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        oversize["errors"]["attachments.0"][0]
            .as_str()
            .unwrap()
            .contains("1 kilobytes")
    );
    upload_file(address, "image", "avatar.PhP8", "image/png", &png, 422).await;
    let svg = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    upload_file(address, "image", "avatar.svg", "image/svg+xml", svg, 422).await;
    upload_file(address, "svg", "avatar.svg", "text/plain", svg, 200).await;
    upload_file(
        address,
        "svg",
        "fake.svg",
        "image/svg+xml",
        b"<?xml version=\"1.0\"?><document><svg/></document>",
        422,
    )
    .await;
    let fake = send(
        address,
        "/api/upload/rules",
        json!({"attachments":[{"name":"avatar.png","mime":"image/png"}]}),
        false,
        422,
    )
    .await;
    assert!(
        fake["errors"]["attachments.0"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("must be a file."))
    );
    send(address, "/api/upload/rules", json!({}), false, 422).await;
    server.abort();
}

fn validation_png(width: u32, height: u32) -> Vec<u8> {
    use ::image::{ExtendedColorType, ImageEncoder as _, codecs::png::PngEncoder};
    let mut output = Vec::new();
    PngEncoder::new(&mut output)
        .write_image(
            &vec![0; (width * height) as usize],
            width,
            height,
            ExtendedColorType::L8,
        )
        .unwrap();
    output
}

#[tokio::test]
async fn validates_tiff_and_ico_dimensions_over_real_http() {
    let (address, server) = start(std::env::temp_dir()).await;
    let little_tiff = b"II*\0\x08\0\0\0\x02\0\
        \0\x01\x03\0\x01\0\0\0\x64\0\0\0\
        \x01\x01\x04\0\x01\0\0\0\x64\0\0\0\
        \0\0\0\0";
    // The EXIF width entry overrides the earlier width in the first directory.
    let big_tiff = b"MM\0*\0\0\0\x08\0\x03\
        \x01\0\0\x03\0\0\0\x01\0\x80\0\0\
        \x01\x01\0\x04\0\0\0\x01\0\0\0\x64\
        \xa0\x02\0\x01\0\0\0\x01\x64\0\0\0\
        \0\0\0\0";
    for bytes in [little_tiff.as_slice(), big_tiff.as_slice()] {
        upload_file(address, "dimensions", "size.tiff", "text/plain", bytes, 200).await;
        upload_file(address, "image", "size.tiff", "image/tiff", bytes, 422).await;
    }
    for bytes in [
        &little_tiff[..little_tiff.len() - 1],
        b"II*\0\xff\xff\xff\xff",
        b"II+\0\0\0\0\0",
    ] {
        upload_file(address, "dimensions", "size.tiff", "image/tiff", bytes, 422).await;
    }

    let mut icons = b"\0\0\x01\0\x03\0".to_vec();
    for (size, depth) in [(128, 8_u16), (64, 32), (100, 32)] {
        let mut entry = [0; 16];
        entry[..2].fill(size);
        entry[6..8].copy_from_slice(&depth.to_le_bytes());
        icons.extend_from_slice(&entry);
    }
    upload_file(address, "dimensions", "size.ico", "text/plain", &icons, 200).await;
    upload_file(address, "image", "size.ico", "image/x-icon", &icons, 422).await;
    icons.pop();
    upload_file(
        address,
        "dimensions",
        "size.ico",
        "image/x-icon",
        &icons,
        422,
    )
    .await;
    upload_file(
        address,
        "dimensions-icon",
        "size.ico",
        "image/x-icon",
        b"\0\0\x01\0\x01\0",
        200,
    )
    .await;
    for bytes in [b"\0\0\x01\0\0\0", b"\0\0\x01\0\0\x01", b"\0\0\x02\0\x01\0"] {
        upload_file(
            address,
            "dimensions",
            "size.ico",
            "image/x-icon",
            bytes,
            422,
        )
        .await;
    }
    server.abort();
}

#[tokio::test]
async fn validates_psd_and_jpeg2000_dimensions_over_real_http() {
    let (address, server) = start(std::env::temp_dir()).await;
    let mut psd = [0; 22];
    psd[..4].copy_from_slice(b"8BPS");
    psd[5] = 1;
    psd[17] = 100;
    psd[21] = 100;
    upload_file(address, "dimensions", "size.psd", "text/plain", &psd, 200).await;
    upload_file(
        address,
        "dimensions",
        "size.psd",
        "text/plain",
        &psd[..21],
        422,
    )
    .await;

    let mut stream = [0; 42];
    stream[..4].copy_from_slice(b"\xff\x4f\xff\x51");
    stream[5] = 41;
    stream[11] = 100;
    stream[15] = 100;
    stream[19] = 50; // PHP reports Xsiz, independent of XOsiz.
    stream[41] = 1;
    upload_file(
        address,
        "dimensions",
        "size.j2k",
        "text/plain",
        &stream,
        200,
    )
    .await;
    upload_file(
        address,
        "dimensions",
        "size.j2k",
        "text/plain",
        &stream[..41],
        422,
    )
    .await;

    let sig = b"\0\0\0\x0cjP  \r\n\x87\n";
    let mut jp2 = sig.to_vec();
    jp2.extend_from_slice(b"\0\0\0\x0cfreepad!");
    let codestream = jp2.len();
    jp2.extend_from_slice(b"\0\0\0\0jp2c");
    jp2.extend_from_slice(&stream);
    upload_file(address, "dimensions", "size.jp2", "text/plain", &jp2, 200).await;
    jp2[codestream + 3] = 1;
    upload_file(address, "dimensions", "size.jp2", "text/plain", &jp2, 422).await;
    jp2[codestream + 3] = 0;
    jp2[codestream + 4..codestream + 8].copy_from_slice(b"jp2h");
    upload_file(address, "dimensions", "size.jp2", "text/plain", &jp2, 422).await;

    stream[40] = 1;
    upload_file(
        address,
        "dimensions",
        "size.j2k",
        "text/plain",
        &stream,
        422,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn validates_legacy_image_dimensions_over_real_http() {
    use std::io::Write as _;

    let (address, server) = start(std::env::temp_dir()).await;
    let iff = b"FORM\0\0\0\0ILBMANNO\0\0\0\x03abc\0BMHD\0\0\0\x09\0\x64\0\x64\0\0\0\0\x01";
    let wbmp = b"\0\x80\0\x64\x64";
    let xbm = b"/* XBM */\n#define a_width 100\n#define a_height 100\n";
    let mut swf = [0; 40];
    swf[..4].copy_from_slice(b"FWS\x08");
    swf[4] = 40;
    swf[8..15].copy_from_slice(b"\x60\0\x3e\x80\0\x3e\x80");
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&swf[8..]).unwrap();
    let pixels = (0..=u8::MAX).collect::<Vec<_>>();
    for _ in 0..40 {
        encoder.write_all(&pixels).unwrap();
    }
    let mut swc = b"CWS\x08\0\0\0\0".to_vec();
    swc.extend(encoder.finish().unwrap());

    for (name, good, bad) in [
        ("size.iff", iff.as_slice(), &iff[..iff.len() - 1]),
        ("size.wbmp", wbmp.as_slice(), b"\0\0\x90\x01\x64"),
        ("size.xbm", xbm.as_slice(), b"#define file_width 100\n"),
        ("size.swf", swf.as_slice(), &swf[..39]),
        ("compressed.swf", swc.as_slice(), &swc[..swc.len() - 1]),
    ] {
        upload_file(address, "dimensions", name, "text/plain", good, 200).await;
        upload_file(address, "dimensions", name, "text/plain", bad, 422).await;
    }
    server.abort();
}

#[tokio::test]
async fn validates_single_byte_encodings_over_real_http() {
    let (address, server) = start(std::env::temp_dir()).await;
    let bytes: Vec<_> = (0..=u8::MAX).collect();
    upload_file(
        address,
        "encoding-cp850",
        "data.bin",
        "application/octet-stream",
        &bytes,
        200,
    )
    .await;
    for (mode, good, bad) in [
        ("encoding-armscii", b"abc\xa2\xfe", b"abc\xa1\xff"),
        ("encoding-windows-1254", b"abc\x8c\x91", b"abc\x8e\x9e"),
    ] {
        upload_file(
            address,
            mode,
            "data.bin",
            "application/octet-stream",
            good,
            200,
        )
        .await;
        let rejected = upload_file(
            address,
            mode,
            "data.bin",
            "application/octet-stream",
            bad,
            422,
        )
        .await;
        assert!(rejected["errors"]["attachments.0"].is_array());
    }
    server.abort();
}

#[tokio::test]
async fn validates_big5_and_korean_byte_sequences_over_real_http() {
    let (address, server) = start(std::env::temp_dir()).await;
    for (mode, valid, invalid) in [
        ("encoding-big-5", b"a\xa4\x40b", b"a\x81\x40b"),
        ("encoding-cp950", b"a\x81\x40b", b"a\xa3\xc0b"),
        ("encoding-euc-kr", b"a\xb0\xa1b", b"a\xc6\x41b"),
        ("encoding-uhc", b"a\xc6\x41b", b"a\xc6\x53b"),
        ("encoding-uhc", b"a\x81\xa1b", b"a\x81\x80b"),
    ] {
        upload_file(
            address,
            mode,
            "data.bin",
            "application/octet-stream",
            valid,
            200,
        )
        .await;
        for bytes in [invalid.as_slice(), &valid[..2]] {
            let response = upload_file(
                address,
                mode,
                "data.bin",
                "application/octet-stream",
                bytes,
                422,
            )
            .await;
            assert!(response["errors"]["attachments.0"].is_array());
        }
    }
    server.abort();
}

#[tokio::test]
async fn validates_upload_dimensions_and_encoding_over_real_http() {
    assert!(RuleSet::parse([("file", "encoding:FOOBAR")]).is_err());
    assert!(RuleSet::parse([("file", "dimensions:ratio=1/0")]).is_err());
    let (address, server) = start(std::env::temp_dir()).await;
    let square = validation_png(100, 100);
    let near_square = validation_png(101, 100);
    let rectangle = validation_png(105, 100);
    for png in [square.as_slice(), near_square.as_slice()] {
        upload_file(
            address,
            "dimensions",
            "size.png",
            "application/octet-stream",
            png,
            200,
        )
        .await;
    }
    upload_file(
        address,
        "dimensions-fluent",
        "size.png",
        "text/plain",
        &square,
        200,
    )
    .await;
    for (mode, bytes) in [
        ("dimensions", rectangle.as_slice()),
        ("dimensions", b"image/png is not metadata".as_slice()),
        ("dimensions-fluent", near_square.as_slice()),
    ] {
        let invalid = upload_file(address, mode, "size.png", "image/png", bytes, 422).await;
        assert_eq!(
            invalid["errors"]["attachments.0"],
            json!(["The attachments.0 field has invalid image dimensions."])
        );
    }
    use openrust::image::{Format, ImageManager, Limits};
    let image = ImageManager::new(Limits::default()).fromBytes(square.clone());
    for format in [Format::Jpeg, Format::Webp, Format::Avif, Format::Heic] {
        let bytes = image.clone().encode(format).toBytes().await.unwrap();
        let mode = if format == Format::Heic {
            "dimensions-heic"
        } else {
            "dimensions"
        };
        upload_file(
            address,
            mode,
            &format!("size.{}", format.extension()),
            "application/octet-stream",
            &bytes,
            200,
        )
        .await;
    }
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;
    upload_file(
        address,
        "svg-dimensions",
        "size.svg",
        "application/octet-stream",
        svg,
        200,
    )
    .await;

    upload_file(
        address,
        "encoding",
        "text.txt",
        "application/octet-stream",
        "☃".as_bytes(),
        200,
    )
    .await;
    let invalid_utf8 = upload_file(
        address,
        "encoding",
        "text.txt",
        "text/plain; charset=utf-8",
        b"\xf0\x28\x8c\x28",
        422,
    )
    .await;
    assert_eq!(
        invalid_utf8["errors"]["attachments.0"],
        json!(["The attachments.0 field must be encoded in UTF-8."])
    );
    upload_file(
        address,
        "encoding-fluent",
        "text.txt",
        "text/plain",
        b"Rust",
        200,
    )
    .await;
    upload_file(
        address,
        "encoding-fluent",
        "text.txt",
        "text/plain",
        "☃".as_bytes(),
        422,
    )
    .await;
    let mut sjis = [0x82, 0xa0].repeat(3000);
    upload_file(
        address,
        "encoding-sjis",
        "text.txt",
        "application/octet-stream",
        &sjis,
        200,
    )
    .await;
    sjis.push(0x82);
    upload_file(
        address,
        "encoding-sjis",
        "text.txt",
        "application/octet-stream",
        &sjis,
        422,
    )
    .await;
    for (mode, valid, invalid) in [
        (
            "encoding-utf16",
            b"\x00A".as_slice(),
            b"\xd8\x00".as_slice(),
        ),
        (
            "encoding-utf32",
            b"A\x00\x00\x00".as_slice(),
            b"\x00\x00\x11\x00".as_slice(),
        ),
        (
            "encoding-utf16-bom",
            b"\xff\xfeA\x00".as_slice(),
            b"\xff\xfe\x00\xd8".as_slice(),
        ),
        (
            "encoding-utf32-bom",
            b"\xff\xfe\x00\x00A\x00\x00\x00".as_slice(),
            b"\xff\xfe\x00\x00\x00\x00\x11\x00".as_slice(),
        ),
    ] {
        upload_file(
            address,
            mode,
            "text.txt",
            "application/octet-stream",
            valid,
            200,
        )
        .await;
        upload_file(
            address,
            mode,
            "text.txt",
            "application/octet-stream",
            invalid,
            422,
        )
        .await;
    }
    send(
        address,
        "/api/upload/encoding-input",
        json!({"metadata":{"key":["ok",3]}}),
        false,
        200,
    )
    .await;
    send(
        address,
        "/api/upload/encoding-input",
        json!({"metadata":{"☃":["ok"]}}),
        false,
        422,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn route_and_controller_arguments_resolve_form_requests_before_actions() {
    let root = std::env::temp_dir();
    let calls = Arc::new(AtomicUsize::new(0));
    let forms = Arc::new(AtomicUsize::new(0));
    let default_app = Application::configure(root.clone())
        .withRouting(Routing::default().api(inject_routes))
        .create()
        .unwrap();
    let bound_app = Application::configure(root)
        .withProviders(vec![Box::new(InjectionProvider {
            calls: calls.clone(),
            forms: forms.clone(),
        })])
        .withRouting(Routing::default().api(inject_routes))
        .create()
        .unwrap();
    let (default_addr, default_server) = serve(default_app).await;
    let (bound_addr, bound_server) = serve(bound_app).await;
    let (default, direct, resource) = tokio::join!(
        send(
            default_addr,
            "/api/injected",
            json!({"name":"  Ada  "}),
            false,
            200
        ),
        send(
            bound_addr,
            "/api/injected/team",
            json!({"name":"  Grace  "}),
            false,
            200
        ),
        send(
            bound_addr,
            "/api/injected-posts",
            json!({"name":"  Lin  "}),
            false,
            200
        ),
    );
    assert_eq!(default, json!({"name":"Ada","stage":2,"factory":0}));
    assert_eq!(direct["workspace"], "team");
    assert_eq!(direct["name"], "Grace");
    assert_eq!(resource["name"], "Lin");
    assert_eq!(resource["stage"], 2);
    assert_eq!(direct["hook"], "validated");
    assert_eq!(resource["hook"], "validated");
    let mut indices = [
        direct["factory"].as_u64().unwrap(),
        resource["factory"].as_u64().unwrap(),
    ];
    indices.sort();
    assert_eq!(indices, [1, 2]);
    assert_eq!(calls.load(Ordering::Relaxed), 2);

    let denied = send(bound_addr, "/api/injected/team", json!({}), true, 403).await;
    assert_eq!(denied["message"], "Injected form denied.");
    let invalid = send(
        bound_addr,
        "/api/injected-posts",
        json!({"name":" "}),
        false,
        422,
    )
    .await;
    assert_eq!(
        invalid["errors"]["name"],
        json!(["The name field is required."])
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(forms.load(Ordering::Relaxed), 4);
    default_server.abort();
    bound_server.abort();
}

#[tokio::test]
async fn real_uploads_classify_file_content_and_distinguish_legacy_encodings() {
    let (address, server) = start(std::env::temp_dir()).await;
    for bytes in [br#"{"hello":"world"}"#.as_slice(), b"[]".as_slice()] {
        upload_file(
            address,
            "mime-json",
            "payload.bin",
            "text/plain",
            bytes,
            200,
        )
        .await;
    }
    for bytes in [b"null".as_slice(), b"[] unfinished".as_slice()] {
        upload_file(
            address,
            "mime-json",
            "payload.json",
            "application/json",
            bytes,
            422,
        )
        .await;
        upload_file(
            address,
            "mime-text",
            "payload.json",
            "application/json",
            bytes,
            200,
        )
        .await;
    }
    upload_file(
        address,
        "mime-text",
        "utf16.txt",
        "application/octet-stream",
        b"\xff\xfeA\x00B\x00\n\x00",
        200,
    )
    .await;
    upload_file(
        address,
        "mime-text",
        "table.csv",
        "text/csv",
        b"id,name\n1,Ada\n2,Lin\n",
        200,
    )
    .await;
    upload_file(address, "mime-empty", "empty.txt", "text/plain", b"", 200).await;
    upload_file(
        address,
        "mime-pdf",
        "file.bin",
        "text/plain",
        b"%PDF-1.4\n%%EOF\n",
        200,
    )
    .await;
    upload_file(
        address,
        "mime-pdf",
        "file.pdf",
        "application/pdf",
        b"hello",
        422,
    )
    .await;

    for (mode, good, bad) in [
        (
            "encoding-sjis",
            b"\x82\xa0".as_slice(),
            b"\x87\x90".as_slice(),
        ),
        ("encoding-cp932", b"\x87\x90".as_slice(), b"\x80".as_slice()),
        (
            "encoding-euc-cn",
            b"\xbe\xbf".as_slice(),
            b"\x81\x40".as_slice(),
        ),
        ("encoding-ucs2", b"\xd8\x00".as_slice(), b"\xd8".as_slice()),
        (
            "encoding-ucs4",
            b"\x00\x11\x00\x00".as_slice(),
            b"\xff\xff\xff\xff".as_slice(),
        ),
    ] {
        upload_file(
            address,
            mode,
            "data.bin",
            "application/octet-stream",
            good,
            200,
        )
        .await;
        upload_file(
            address,
            mode,
            "data.bin",
            "application/octet-stream",
            bad,
            422,
        )
        .await;
    }
    assert!(RuleSet::parse([("file", "encoding:latin1")]).is_err());
    assert!(RuleSet::parse([("file", "encoding:US-ASCII")]).is_err());
    server.abort();
}

#[tokio::test]
async fn transfer_encoding_and_canonical_base64_follow_laravel_over_http() {
    let (address, server) = start(std::env::temp_dir()).await;
    for encoded in ["Zg==", "AAAA", "aGVsbG8="] {
        send(
            address,
            "/api/upload/strict-base64-string",
            json!({"value":encoded}),
            false,
            200,
        )
        .await;
    }
    let long = "YWJj".repeat(4096);
    send(
        address,
        "/api/upload/strict-base64",
        json!({"value":long}),
        false,
        200,
    )
    .await;
    for permissive in [
        "A",
        "Zg",
        "Zh==",
        "Zg=== ",
        "Z g= =",
        "Zg==YQ==",
        "Zm9v\r\nYmFy",
    ] {
        upload_file(
            address,
            "encoding-base64",
            "encoded.txt",
            "text/plain",
            permissive.as_bytes(),
            200,
        )
        .await;
        let invalid = send(
            address,
            "/api/upload/strict-base64",
            json!({"value":permissive}),
            false,
            422,
        )
        .await;
        assert_eq!(
            invalid["errors"]["value"],
            json!(["The value field must be a valid Base64 string."])
        );
    }
    let invalid_type = send(
        address,
        "/api/upload/strict-base64",
        json!({"value":12}),
        false,
        422,
    )
    .await;
    assert_eq!(
        invalid_type["errors"]["value"],
        json!(["The value field must be a valid Base64 string."])
    );
    for invalid in [b"Zg==-".as_slice(), b"Zg==\x0b", b"YWJj\0", b"Zg==\xff"] {
        let invalid = upload_file(
            address,
            "encoding-base64",
            "encoded.txt",
            "application/octet-stream",
            invalid,
            422,
        )
        .await;
        assert_eq!(
            invalid["errors"]["attachments.0"],
            json!(["The attachments.0 field must be encoded in BASE64."])
        );
    }
    for (mode, payload) in [
        ("encoding-qp", b"caf=C3=A9\r\nnext=0G\xff".as_slice()),
        (
            "encoding-entities",
            b"&amp; &unknown; &#xZZ; \xff".as_slice(),
        ),
        ("encoding-uu", b"begin 644 file\n#0V%T\n`\nend\n".as_slice()),
        ("encoding-uu", b"begin 644 file\n!\xff\n".as_slice()),
    ] {
        upload_file(
            address,
            mode,
            "encoded.txt",
            "application/octet-stream",
            payload,
            200,
        )
        .await;
    }
    server.abort();
}

#[tokio::test]
async fn shifted_utf7_encodings_follow_their_byte_and_termination_rules_over_http() {
    let (address, server) = start(std::env::temp_dir()).await;
    for (mode, valid, invalid) in [
        (
            "encoding-utf7",
            ["Caf+AOk-", "+AOk!", "+2D3eAA-", "+//8-", "+-", "+", "+AOk"].as_slice(),
            ["+AOl-", "+2AA-", "+3AA-", "+A", "+AOk~", "+.", "\\", "~"].as_slice(),
        ),
        (
            "encoding-utf7-imap",
            ["Caf&AOk-", "&2D3eAA-", "&,,8-", "&-", "&ACY-", "\\", "~"].as_slice(),
            [
                "&AOl-", "&2AA-", "&3AA-", "&//8-", "&AEE-", "&AOk", "&", "\n",
            ]
            .as_slice(),
        ),
    ] {
        for sample in valid {
            upload_file(
                address,
                mode,
                "encoded.txt",
                "application/octet-stream",
                sample.as_bytes(),
                200,
            )
            .await;
        }
        for sample in invalid {
            let invalid = upload_file(
                address,
                mode,
                "encoded.txt",
                "application/octet-stream",
                sample.as_bytes(),
                422,
            )
            .await;
            let label = if mode == "encoding-utf7" {
                "UTF-7"
            } else {
                "UTF7-IMAP"
            };
            assert_eq!(
                invalid["errors"]["attachments.0"],
                json!([format!(
                    "The attachments.0 field must be encoded in {label}."
                )])
            );
        }
    }
    server.abort();
}
