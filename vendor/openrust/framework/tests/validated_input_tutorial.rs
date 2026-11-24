use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        http::Kernel as _,
        support::{ArrayAccess, Arrayable, ValidatedData},
        validation::{Factory as _, Validator as _},
    },
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, Kernel, ValidatedFormRequest},
    },
    http::{HttpError, HttpFuture, Request, RequestValue, Response, StatusCode},
    routing::Router,
    support::{ArrayKey, Collection, ValidatedInput},
    validation::{Data, Factory, Rule, RuleSet, ValidationResult},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
fn cases() -> &'static Value {
    static CASES: OnceLock<Value> = OnceLock::new();
    CASES.get_or_init(|| {
        serde_json::from_str(include_str!("reference/validated-input.json")).unwrap()
    })
}
fn contract<T: ValidatedData>(input: &T) -> Value {
    Arrayable::to_array(input)
}
fn outputs(input: ValidatedInput) -> ValidationResult<Value> {
    let only = input.only(&["name", "email"])?;
    let except = input.except(&["name", "email"])?;
    let nested = input.only(&["profile.role", "rows.1.id", "a.b"])?;
    let merged = input.merge(json!({"name":"B","profile":{"role":"admin"}}))?;
    let collected = input.collect();
    let iteration: Collection<RequestValue> = Collection::from_entries(&input);
    Ok(
        json!({"only":only,"except":except,"nested":nested,"merge":merged,"numeric_merge":input.merge(json!(["last"]))?,"collect":collected,"iteration":iteration,"nullable_exists":ArrayAccess::offset_exists(&input,"nullable"),"nullable_value":input["nullable"],"missing":input["absent"],"all":contract(&input)}),
    )
}
fn show(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = serde_json::from_slice(&request.body_bytes().await?)?;
        Response::json(StatusCode::OK, outputs(ValidatedInput::new(data)?)?)
    })
}
#[derive(Default)]
struct Signup;
impl FormRequest for Signup {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("name", "required|string"), ("email", "required|email")])
    }
}
async fn form(form: ValidatedFormRequest<Signup>) -> Result<Response, HttpError> {
    let input = form.safe();
    assert_eq!(input.all(), form.validated().all());
    let only = input.only(&["name"])?;
    let email = input.except(&["name"])?;
    let merged = input.merge(json!({"trusted":true}))?;
    Response::json(
        StatusCode::OK,
        json!({"only":only,"except":email,"merged":merged,"collect":input.collect(),"indexed":input["email"],"borrowed":input.iter().count()}),
    )
}
fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut validator = openrust::support::facades::Validator::make(
            Data::from_request(request).await?,
            RuleSet::parse([
                ("photo", "required|file"),
                ("nested.file", "required|file"),
                ("name", "required|string"),
            ])?,
        )?;
        validator.validate_request(request).await?;
        let input = validator.safe()?;
        let selected = input.only(&["photo", "nested.file"])?;
        let removed = input.except(&["photo"])?;
        let merged = input.merge(json!({"name":"Trusted"}))?;
        for set in [&selected, &merged] {
            assert_eq!(set.file("photo").unwrap().get(), "hello");
            assert_eq!(set.file("nested.file").unwrap().get(), "hello");
        }
        assert!(removed.file("photo").is_none());
        assert!(removed.file("nested.file").is_some());
        let collection = input.collect();
        let RequestValue::File(file) = collection.get("photo").unwrap() else {
            panic!("集合必须保留文件类型");
        };
        assert_eq!(file.get(), "hello");
        let mut changed = input.clone();
        ArrayAccess::offset_set(
            &mut changed,
            Some(ArrayKey::from("photo")),
            RequestValue::Value(json!("replaced")),
        )?;
        assert!(changed.file("photo").is_none());
        assert!(input.file("photo").is_some());
        let mut borrowed = input.clone();
        let RequestValue::File(file) = ArrayAccess::offset_get(&borrowed, "photo").unwrap() else {
            panic!("offset必须保留文件类型");
        };
        let typed = RequestValue::Array(indexmap::IndexMap::from([(
            ArrayKey::from("added"),
            RequestValue::File(file),
        )]));
        borrowed = borrowed.merge_fields(typed)?;
        assert!(borrowed.file("added").is_some());
        ArrayAccess::offset_unset(&mut borrowed, "photo");
        assert!(borrowed.file("photo").is_none());
        assert!(borrowed.file("added").is_some());
        Response::json(
            StatusCode::OK,
            json!({"selected":selected,"removed":removed,"merged":merged,"collection":collection,"changed":changed,"borrowed":borrowed}),
        )
    })
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}
async fn send(address: SocketAddr, path: &str, body: &str, content: &str) -> (u16, Value) {
    timeout(Duration::from_secs(5),async{
 let mut stream=TcpStream::connect(address).await.unwrap();stream.write_all(format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();let mut response=String::new();stream.read_to_string(&mut response).await.unwrap();let status=response.split_whitespace().nth(1).unwrap().parse().unwrap();let body=response.split_once("\r\n\r\n").unwrap().1;(status,serde_json::from_str(body).unwrap())
}).await.unwrap()
}
#[tokio::test]
async fn validated_input_tutorials_and_form_safe_work_over_real_http() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_owned())
        .with_config(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"locale":"en","supported_locales":[]}),
            )]))
        })
        .with_routing(Routing::new(|_| {
            let mut r = Router::new();
            r.post("/show", show)?;
            r.post("/form", form)?;
            r.post("/upload", upload)?;
            Ok(r)
        }))
        .create()
        .unwrap();
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
    for (index, case) in cases().as_array().unwrap().iter().enumerate() {
        let (status, data) = send(
            address,
            "/show",
            &case["input"].to_string(),
            "application/json",
        )
        .await;
        assert_eq!(status, 200, "{data}");
        for field in [
            "only",
            "except",
            "nested",
            "merge",
            "numeric_merge",
            "collect",
            "iteration",
            "nullable_exists",
            "nullable_value",
            "missing",
        ] {
            if case[field].as_array().is_some_and(Vec::is_empty)
                && data[field]
                    .as_object()
                    .is_some_and(serde_json::Map::is_empty)
            {
                continue;
            }
            assert_eq!(data[field], case[field], "case {index}: {field}");
        }
    }
    let (status, data) = send(
        address,
        "/form",
        r#"{"name":"A","email":"a@example.com","admin":true}"#,
        "application/json",
    )
    .await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(
        data,
        json!({"only":{"name":"A"},"except":{"email":"a@example.com"},"merged":{"name":"A","email":"a@example.com","trusted":true},"collect":{"name":"A","email":"a@example.com"},"indexed":"a@example.com","borrowed":2})
    );
    assert_eq!(
        send(address, "/form", "{}", "application/json").await.0,
        422
    );
    let body=["photo","nested[file]"].iter().map(|name|format!("--form\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"input.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n")).collect::<String>()+"--form\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nOriginal\r\n--form--\r\n";
    let (status, data) = send(
        address,
        "/upload",
        &body,
        "multipart/form-data; boundary=form",
    )
    .await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(
        data["selected"],
        json!({"photo":null,"nested":{"file":null}})
    );
    assert!(data["collection"]["photo"].is_null());
    assert_eq!(data["changed"]["photo"], "replaced");
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn subsets_and_mutations_are_independent_and_keep_numeric_and_literal_keys() {
    let input =
        ValidatedInput::new(json!({"a.b":1,"a":{"b":2},"items":[1,2,3],"nullable":null})).unwrap();
    assert_eq!(input.only(&[r"a\.b"]).unwrap().all(), &json!({"a.b":1}));
    assert_eq!(input.only(&["a.b"]).unwrap().all(), &json!({"a":{"b":2}}));
    assert!(input.except(&[r"a\.b"]).unwrap().get(r"a\.b").is_none());
    assert_eq!(input["a.b"], 2);
    assert_eq!(input[r"a\.b"], 1);
    assert_eq!(input["missing"], Value::Null);
    assert!(input.offset_exists("nullable"));
    let mut changed = input.clone();
    changed
        .offset_set(Some(ArrayKey::from("a.b")), RequestValue::Value(json!(3)))
        .unwrap();
    assert_eq!(changed[r"a\.b"], 3);
    assert_eq!(changed["a.b"], 2);
    changed.offset_unset("a.b");
    assert!(changed.get(r"a\.b").is_none());
    assert_eq!(input[r"a\.b"], 1);
    let mut numbers = ValidatedInput::new(json!(["a", "b"])).unwrap();
    numbers.offset_unset("0");
    assert_eq!(numbers.all(), &json!({"1":"b"}));
    numbers
        .offset_set(None, RequestValue::Value(json!("c")))
        .unwrap();
    assert_eq!(numbers.all(), &json!({"1":"b","2":"c"}));
    assert_eq!(
        numbers.merge(json!(["d"])).unwrap().all(),
        &json!(["b", "c", "d"])
    );
    let mut trailing = ValidatedInput::new(json!(["a", "b"])).unwrap();
    trailing.offset_unset("1");
    trailing
        .offset_set(None, RequestValue::Value(json!("c")))
        .unwrap();
    assert_eq!(trailing.all(), &json!({"0":"a","2":"c"}));
    let mut empty = ValidatedInput::new(json!(["a"])).unwrap();
    empty.offset_unset("0");
    empty
        .offset_set(None, RequestValue::Value(json!("b")))
        .unwrap();
    assert_eq!(empty.all(), &json!({"1":"b"}));
    assert!(matches!(
        input.only(&["items.*"]),
        Err(openrust::validation::ValidationError::InvalidPath)
    ));
    let owned: Vec<_> = input.clone().into_iter().collect();
    assert_eq!(owned.len(), 4);
    let iter: openrust::support::ValidatedInputIter<'_> = (&input).into_iter();
    assert_eq!(iter.count(), 4);
    let factory = Factory::default();
    let mut validator = factory
        .make(
            Data::new(json!({"name":"A"})).unwrap(),
            RuleSet::new().field("name", [Rule::required()]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.safe().is_err());
    let mut safe = validator.validate().await.unwrap();
    safe.offset_set(None, RequestValue::Value(json!("appended")))
        .unwrap();
    assert_eq!(safe.all(), &json!({"name":"A","0":"appended"}));
    let mut numeric_validator = factory
        .make(
            Data::new(json!(["a", "b"])).unwrap(),
            RuleSet::parse([("*", "string")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let mut safe = numeric_validator.validate().await.unwrap();
    safe.offset_set(None, RequestValue::Value(json!("c")))
        .unwrap();
    assert_eq!(safe.all(), &json!(["a", "b", "c"]));
}
