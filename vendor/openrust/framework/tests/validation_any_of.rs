use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::Router,
    support::facades::Validator,
    validation::{
        Data, Rule, RuleSet,
        rules::{AnyOf, AnyOfRules},
    },
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
        serde_json::from_str(include_str!("reference/validation-any-of.json")).unwrap()
    })
}
fn group(value: &Value) -> Result<RuleSet, openrust::validation::ValidationError> {
    let entries: Vec<(String, String)> = if let Some(fields) = value.as_object() {
        fields
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
            .collect()
    } else {
        value
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.as_str().unwrap().to_owned()))
            .collect()
    };
    RuleSet::parse(entries.iter().map(|(k, v)| (k.as_str(), v.as_str())))
}
fn validate(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let index = request
            .uri()
            .path()
            .rsplit('/')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let case = &cases()[index];
        let groups = case["groups"].as_array().unwrap();
        let rule: Rule = if case["kind"] == "scalar" {
            let groups: Result<Vec<Vec<Rule>>, _> = groups
                .iter()
                .map(|g| {
                    g.as_array()
                        .unwrap()
                        .iter()
                        .map(|v| Rule::parse(v.as_str().unwrap()))
                        .collect()
                })
                .collect();
            Rule::any_of(groups?).into()
        } else {
            let groups: Result<Vec<_>, _> = groups.iter().map(group).collect();
            AnyOf::new(groups?).into()
        };
        let input = serde_json::from_slice(&request.body_bytes().await?)?;
        let mut validator = Validator::make_with(
            Data::new(input)?,
            RuleSet::new().field("value", [rule])?,
            if case["custom"].as_bool().unwrap() {
                BTreeMap::from([(
                    "value.any_of".into(),
                    "Configured :attribute failure.".into(),
                )])
            } else {
                BTreeMap::new()
            },
            BTreeMap::from([("value".into(), "selected".into())]),
        )?;
        validator.validate_request(request).await?;
        Response::json(StatusCode::OK, json!({"passes":true}))
    })
}
fn files(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let input = Data::from_request(request).await?;
        let scalar = || -> openrust::validation::ValidationResult<Rule> {
            Ok(Rule::any_of([vec![
                Rule::parse("required")?,
                Rule::parse("file")?,
                Rule::parse("max:2")?,
            ]])
            .into())
        };
        let rules = RuleSet::new()
            .field("upload", [Rule::required(), scalar()?])?
            .field(r"container.doc\.name", [Rule::required(), scalar()?])?
            .field(
                "payload",
                [
                    Rule::required(),
                    AnyOf::new([RuleSet::parse([("document", "required|file|max:2")])?]).into(),
                ],
            )?;
        let mut validator = Validator::make(input, rules)?;
        let data = validator.validate_request(request).await?;
        Response::json(
            StatusCode::OK,
            json!({"files":data.file("upload").is_some() && data.file("payload.document").is_some() && data.file(r"container.doc\.name").is_some()}),
        )
    })
}
fn mixed(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let choice = Rule::any_of([
            AnyOfRules::Value(vec![Rule::any_of([vec![Rule::string()]]).into()]),
            AnyOfRules::Fields(RuleSet::parse([("name", "required|string")])?),
        ]);
        let rules = RuleSet::new().field("value", [Rule::required(), choice.into()])?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::OK, data.into_value())
    })
}
fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let log = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let observed = log.clone();
        Validator::extend(
            "probe",
            |attribute, value, _, validator| {
                assert_eq!(attribute, "0");
                assert_eq!(value, Some(&json!("abc")));
                assert_eq!(validator.get_data().as_value(), &json!(["abc"]));
                Ok(false)
            },
            Some("fallback"),
        )?;
        Validator::replacer("probe", move |message, _, _, _, _| {
            observed.lock().unwrap().push(message.to_owned());
            message.to_owned()
        })?;
        let rule = Rule::any_of([
            vec![Rule::parse("probe")?],
            vec![Rule::string()],
            vec![Rule::parse("probe")?],
        ]);
        let mut validator = Validator::make_with(
            Data::new(json!({"value":"abc","other":true}))?,
            RuleSet::new().field("value", [rule.into()])?,
            BTreeMap::from([("probe".into(), "Inherited :attribute.".into())]),
            BTreeMap::from([("0".into(), "candidate".into())]),
        )?;
        let data = validator.validate_request(request).await?;
        let observed = log.lock().unwrap().clone();
        Response::json(
            StatusCode::OK,
            json!({"data":data.into_value(),"messages":observed}),
        )
    })
}
fn database(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let rules = RuleSet::new().field(
            "value",
            [Rule::any_of([
                vec![Rule::exists("choices", Some("id")).into()],
                vec![Rule::parse("in:manual")?],
            ])
            .into()],
        )?;
        let data = request.validate(rules).await?;
        Response::json(StatusCode::OK, data.into_value())
    })
}
async fn serve(app: Application) -> (SocketAddr, tokio::task::JoinHandle<()>) {
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
    (address, server)
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
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
async fn any_of_matches_fixed_php_scalar_and_field_groups_over_real_http() {
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
            r.post("/case/{index}", validate)?;
            r.post("/files", files)?;
            r.post("/probe", probe)?;
            r.post("/mixed", mixed)?;
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
        let (status, data, _) = send(
            address,
            "POST",
            &format!("/case/{index}"),
            &case["input"].to_string(),
            "",
        )
        .await;
        assert_eq!(
            status,
            if case["passes"].as_bool().unwrap() {
                200
            } else {
                422
            },
            "case {index} {data}"
        );
        if status == 422 {
            assert_eq!(
                data["errors"]["value"], case["errors"]["value"],
                "case {index}"
            );
        }
    }
    for (body, status) in [
        (r#"{"value":"text"}"#, 200),
        (r#"{"value":{"name":"A"}}"#, 200),
        (r#"{"value":{"name":1}}"#, 422),
    ] {
        assert_eq!(send(address, "POST", "/mixed", body, "").await.0, status);
    }
    let (status, data, _) = send(address, "POST", "/probe", "{}", "").await;
    assert_eq!(status, 200, "{data}");
    assert_eq!(
        data,
        json!({"data":{"value":"abc"},"messages":["Inherited candidate."]})
    );
    for (content, status) in [("hello".to_owned(), 200), ("x".repeat(3072), 422)] {
        let body=["upload","container[doc.name]","payload[document]"].iter().map(|name|format!("--form-boundary\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"sample.txt\"\r\nContent-Type: text/plain\r\n\r\n{content}\r\n")).collect::<String>()+"--form-boundary--\r\n";
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("POST /files HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: multipart/form-data; boundary=form-boundary\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{response}"
        );
        let data: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        if status == 200 {
            assert_eq!(data["files"], true);
        } else {
            for field in ["upload", "container.doc.name", "payload"] {
                assert!(data["errors"][field].is_array(), "{data}");
            }
        }
    }
    server.abort();
    let _ = server.await;
}

fn database_config(url: &str, prefix: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("app".into(), json!({"locale":"en","supported_locales":[]})),
        (
            "database".into(),
            json!({"default":"directory","connections":{"directory":{"url":url,"prefix":prefix,"read":{"url":"mysql://invalid:invalid@127.0.0.1:1/invalid","port":1}}}}),
        ),
    ])
}

#[tokio::test]
async fn any_of_children_use_the_mysql_factory_and_write_presence_connection() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let prefix = format!("anyof_{:016x}_", fastrand::u64(..));
    let config = database_config(&url, &prefix);
    let connection = openrust::database::DatabaseManager::new(
        Arc::new(openrust::config::Repository::new(config.clone())),
        ".",
    )
    .connection(None)
    .unwrap();
    let table = format!("{prefix}choices");
    connection
        .statement(&format!("CREATE TABLE {table} (id INT PRIMARY KEY)"), &[])
        .await
        .unwrap();
    connection
        .statement(&format!("INSERT INTO {table} VALUES (1)"), &[])
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join(&prefix);
    std::fs::create_dir(&base).unwrap();
    let app = Application::configure(base)
        .with_config(|base, _| {
            Ok(database_config(
                &std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),
                base.file_name().unwrap().to_str().unwrap(),
            ))
        })
        .with_routing(Routing::new(|_| {
            let mut r = Router::new();
            r.post("/database", database)?;
            Ok(r)
        }))
        .create()
        .unwrap();
    let (address, server) = serve(app).await;
    for (body, status) in [
        (r#"{"value":1}"#, 200),
        (r#"{"value":999}"#, 422),
        (r#"{"value":"manual"}"#, 200),
    ] {
        let (code, data, _) = send(address, "POST", "/database", body, "").await;
        assert_eq!(code, status, "{data}");
    }
    server.abort();
    let _ = server.await;
    connection
        .statement(&format!("DROP TABLE {table}"), &[])
        .await
        .unwrap();
}
