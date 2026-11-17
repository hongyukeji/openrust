use std::{net::SocketAddr, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::ControllerDispatcher,
    support::{BackedEnum, ProviderResult, ServiceProvider, facades::Route},
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct EchoController(&'static str);

impl EchoController {
    fn show<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let all = request.input(None).await?;
            let nested = request.input(Some("team.name")).await?;
            let exact = request.query(Some("team.name"))?;
            let upload = request.file("attachments.0").await?;
            let has_file = request.hasFile("attachments").await?;
            let raw_size = request.body_bytes().await?.len();
            let response = Response::json(
                StatusCode::OK,
                json!({
                    "app": self.0, "input": all, "nested": nested, "query_exact": exact,
                    "file": upload.map(|file| json!({
                        "name": file.getClientOriginalName(),
                        "path": file.getClientOriginalPath(),
                        "mime": file.getClientMimeType(),
                        "bytes": file.get().as_ref(),
                        "size": file.getSize(),
                    })),
                    "has_file": has_file, "raw_size": raw_size,
                }),
            )?;
            Ok(response.header(
                ::http::header::CACHE_CONTROL,
                ::http::HeaderValue::from_static("no-store"),
            ))
        })
    }
}

struct Provider(&'static str);

impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let label = self.0;
            app.container()
                .bind::<EchoController, _>(move |_| Ok(Arc::new(EchoController(label))))?;
            Ok(())
        })
    }
}

fn stream(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let chunks = futures_util::stream::iter([Ok::<_, std::io::Error>("first"), Ok("second")]);
        Ok(Response::stream(StatusCode::OK, chunks))
    })
}

fn scalar_input(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut values = Vec::new();
        for strict in [false, true] {
            values.push(json!({
                "integer": request.integer("value", 42, strict).await?,
                "float": request.float("value", 1.25, strict).await?,
                "boolean": request.boolean("value", true, strict).await?,
            }));
        }
        Response::json(StatusCode::OK, values)
    })
}

#[derive(serde::Serialize)]
enum Stage {
    Draft,
    Ready,
}

impl BackedEnum for Stage {
    type Backing = String;

    fn try_from_backing(backing: String) -> Option<Self> {
        match backing.as_str() {
            "draft" => Some(Self::Draft),
            "ready" => Some(Self::Ready),
            _ => None,
        }
    }
}

#[derive(serde::Serialize)]
enum Toggle {
    Off,
    On,
}

impl BackedEnum for Toggle {
    type Backing = i64;

    fn try_from_backing(backing: i64) -> Option<Self> {
        match backing {
            0 => Some(Self::Off),
            1 => Some(Self::On),
            _ => None,
        }
    }
}

fn typed_values(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let text = request.string("label", "fallback", true).await?.trim();
        let loose = request.str("label", "fallback", false).await?;
        let items = request.array(Some("items")).await?;
        let collected: Vec<_> = request.collect(Some("items")).await?.into_iter().collect();
        let stage = request.r#enum("stage", Some(Stage::Draft)).await?;
        let toggle = request.r#enum::<Toggle>("toggle", None).await?;
        let cases: Vec<_> = request.enums::<Stage>("list").await?.into_iter().collect();
        Response::json(
            StatusCode::OK,
            json!({"text": text, "loose": loose, "items": items, "collected": collected, "stage": stage, "toggle": toggle, "cases": cases}),
        )
    })
}

fn dated_values(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let date = request.date("when", None, None).await?;
        let formatted = request
            .date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo"))
            .await?;
        let dates = [date, formatted].map(|date| {
            date.map(|date| {
                json!([
                    date.format("Y-m-d H:i:s.u P").unwrap(),
                    date.timezone(),
                    date.microseconds(),
                    date.timestamp()
                ])
            })
        });
        let keys = ["user.name", "user.note", "missing", "query"];
        let selected = request.only(&keys).await?;
        let array = request.arrayOnly(&keys).await?;
        let collection: Vec<_> = request.collectOnly(&keys).await?.into_iter().collect();
        let remainder = request
            .except(&["when", "formatted", "user.secret"])
            .await?;
        Response::json(
            StatusCode::OK,
            json!({"dates": dates, "selected": selected, "array": array, "collection": collection, "remainder": remainder}),
        )
    })
}

fn interval_values(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let interval = request.interval("duration", None).await?;
        let explicit = request.interval("seconds", Some("second")).await?;
        request.interval("invalid_unit", Some("sec")).await?;
        let values = [interval, explicit].map(|value| {
            value.map(|span| {
                json!([
                    span.years,
                    span.months,
                    span.days,
                    span.hours,
                    span.minutes,
                    span.seconds,
                    span.microseconds,
                    span.invert,
                    span.total_days()
                ])
            })
        });
        Response::json(StatusCode::OK, values)
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let (parts, body) = kernel
        .handle(&mut Request::from_parts(parts, body))
        .await
        .into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn start(label: &'static str) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withProviders(vec![Box::new(Provider(label))])
        .withRouting(Routing::default().web(|| {
            Route::r#match(
                &[Method::GET, Method::POST],
                "/input",
                ControllerDispatcher::new(EchoController::show),
            )?;
            Route::r#match(&[Method::GET, Method::POST], "/scalars", scalar_input)?;
            Route::r#match(&[Method::POST], "/typed-values", typed_values)?;
            Route::r#match(&[Method::POST], "/dated-values", dated_values)?;
            Route::r#match(&[Method::POST], "/intervals", interval_values)?;
            Route::get("/stream", stream)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

async fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    media: &str,
    body: &[u8],
    status: u16,
) -> (String, String) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let header = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(body).await.unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(
        headers.starts_with(&format!("HTTP/1.1 {status} ")),
        "{response}"
    );
    (headers.to_owned(), body.to_owned())
}

#[tokio::test]
async fn real_http_dispatches_controllers_and_reads_json_form_uploads_and_streams() {
    let ((first, one), (second, two)) = tokio::join!(start("first"), start("second"));
    let json_body = br#"{"team":{"name":"Ada"},"present":null}"#;
    let query = "/input?team[name]=query&present=query&keep=1";
    let ((headers, body), (_, second_body)) = tokio::join!(
        send(
            first,
            "POST",
            query,
            "application/vnd.example+json",
            json_body,
            200
        ),
        send(second, "POST", query, "application/json", json_body, 200),
    );
    assert!(headers.contains("\r\ncontent-type: application/json"));
    assert!(headers.contains("\r\ncache-control: no-store"));
    let first_json: Value = serde_json::from_str(&body).unwrap();
    let second_json: Value = serde_json::from_str(&second_body).unwrap();
    assert_eq!(first_json["app"], "first");
    assert_eq!(second_json["app"], "second");
    assert_eq!(
        first_json["input"],
        json!({"team":{"name":"Ada"},"present":null,"keep":"1"})
    );
    assert_eq!(first_json["nested"], "Ada");
    assert!(first_json["query_exact"].is_null());
    assert_eq!(first_json["raw_size"], json_body.len());

    let form = b"team[name]=form&tag[]=a&tag[]=b&key=old&key=new";
    let (_, body) = send(
        first,
        "POST",
        query,
        "application/x-www-form-urlencoded",
        form,
        200,
    )
    .await;
    let form_json: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(form_json["input"]["tag"], json!(["a", "b"]));
    assert_eq!(form_json["input"]["key"], "new");
    assert_eq!(form_json["nested"], "form");
    let (_, body) = send(
        first,
        "GET",
        query,
        "application/x-www-form-urlencoded",
        form,
        200,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["nested"],
        "query"
    );

    let multipart = b"--probe\r\nContent-Disposition: form-data; name=\"team[name]\"\r\n\r\nupload\r\n--probe\r\nContent-Disposition: form-data; name=\"attachments[]\"; filename=\"sub/example.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n\x00\xff\r\n--probe--\r\n";
    let (_, body) = send(
        first,
        "POST",
        query,
        "multipart/form-data; boundary=probe",
        multipart,
        200,
    )
    .await;
    let upload: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(upload["nested"], "upload");
    assert_eq!(upload["has_file"], true);
    assert_eq!(
        upload["file"],
        json!({"name":"example.bin","path":"sub/example.bin","mime":"application/octet-stream","bytes":[0,255],"size":2})
    );
    assert_eq!(upload["raw_size"], multipart.len());

    for (media, invalid) in [
        ("application/json", b"{broken".as_slice()),
        ("application/x-www-form-urlencoded", b"name=%FF".as_slice()),
        ("multipart/form-data", b"no-boundary".as_slice()),
    ] {
        let (_, body) = send(first, "POST", "/input", media, invalid, 400).await;
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["message"],
            "Bad Request"
        );
    }
    let mut socket = tokio::net::TcpStream::connect(first).await.unwrap();
    socket.write_all(
        format!("POST /input HTTP/1.1\r\nHost: {first}\r\nContent-Type: application/json\r\nContent-Length: 8388609\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    ).await.unwrap();
    let mut rejected = String::new();
    socket.read_to_string(&mut rejected).await.unwrap();
    assert!(rejected.starts_with("HTTP/1.1 413 "), "{rejected}");
    let (headers, body) = send(first, "GET", "/stream", "", b"", 200).await;
    assert!(headers.contains("\r\ntransfer-encoding: chunked"));
    assert_eq!(body, "5\r\nfirst\r\n6\r\nsecond\r\n0\r\n\r\n");
    one.abort();
    two.abort();
}

#[tokio::test]
async fn real_http_reads_strict_and_permissive_scalar_input() {
    let (address, server) = start("scalars").await;
    for (payload, permissive, strict) in [
        (json!({}), (42, 1.25, true), (42, 1.25, true)),
        (json!({"value": null}), (0, 0.0, false), (42, 1.25, true)),
        (json!({"value": false}), (0, 0.0, false), (42, 1.25, false)),
        (json!({"value": [1]}), (1, 1.0, false), (42, 1.25, true)),
        (json!({"value": "  01 "}), (1, 1.0, false), (42, 1.0, true)),
        (
            json!({"value": " -1.25e2tail "}),
            (-125, -125.0, false),
            (42, 1.25, true),
        ),
        (
            json!({"value": "1e-400"}),
            (0, 0.0, false),
            (42, 1.25, true),
        ),
        (
            json!({"value": "0e-400"}),
            (0, 0.0, false),
            (42, 1.25, true),
        ),
        (
            json!({"value": "  YeS  "}),
            (0, 0.0, true),
            (42, 1.25, true),
        ),
        (
            json!({"value": "  OFF  "}),
            (0, 0.0, false),
            (42, 1.25, false),
        ),
        (json!({"value": 1.0}), (1, 1.0, true), (1, 1.0, true)),
        (
            json!({"value": 1e14}),
            (100_000_000_000_000, 1e14, false),
            (42, 1e14, true),
        ),
        (
            json!({"value": "9223372036854775808"}),
            (i64::MAX, 9_223_372_036_854_775_808.0, false),
            (42, 9_223_372_036_854_775_808.0, true),
        ),
    ] {
        let bytes = serde_json::to_vec(&payload).unwrap();
        let (_, body) = send(address, "POST", "/scalars", "application/json", &bytes, 200).await;
        let response: Value = serde_json::from_str(&body).unwrap();
        let expected = [permissive, strict]
            .map(|(integer, float, boolean)| json!({"integer": integer, "float": float, "boolean": boolean}));
        assert_eq!(response, json!(expected), "{payload}");
    }
    let (_, body) = send(address, "GET", "/scalars?value=19", "", b"", 200).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()[1]["integer"],
        19
    );
    let (_, body) = send(
        address,
        "POST",
        "/scalars?value=19",
        "application/x-www-form-urlencoded",
        b"value=2.5",
        200,
    )
    .await;
    let response: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(response[0]["integer"], 2);
    assert_eq!(response[1]["integer"], 42);
    assert_eq!(response[1]["float"], 2.5);
    send(
        address,
        "POST",
        "/scalars?value=19",
        "application/json",
        b"{broken",
        400,
    )
    .await;
    server.abort();
}

#[tokio::test]
async fn real_http_reads_fluent_strings_arrays_and_backed_enums() {
    let (address, server) = start("typed-values").await;
    let path = "/typed-values?label=query&stage=draft";
    let (_, body) = send(address, "POST", path, "application/json", br#"{"label":"  published  ","items":{"z":9,"0":1,"a":2},"stage":"ready","toggle":"1.0","list":["ready","unknown","draft","ready"]}"#, 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"text":"published", "loose":"  published  ", "items":{"z":9,"0":1,"a":2}, "collected":[["z",9],[0,1],["a",2]], "stage":"Ready", "toggle":"On", "cases":[[0,"Ready"],[2,"Draft"],[3,"Ready"]]})
    );

    let (_, body) = send(address, "POST", path, "application/json", br#"{"label":null,"items":false,"stage":"none","toggle":false,"list":{"first":"draft","skip":"unknown","again":"draft"}}"#, 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"text":"fallback", "loose":"", "items":[false], "collected":[[0,false]], "stage":"Draft", "toggle":"Off", "cases":[["first","Draft"],["again","Draft"]]})
    );

    let (_, body) = send(
        address,
        "POST",
        "/typed-values",
        "application/json",
        b"{}",
        200,
    )
    .await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value,
        json!({"text":"fallback", "loose":"fallback", "items":[], "collected":[], "stage":"Draft", "toggle":null, "cases":[]})
    );

    for invalid in [
        br#"{"stage":[]}"#.as_slice(),
        br#"{"toggle":"1tail"}"#,
        br#"{"list":["ready",{}]}"#,
    ] {
        send(address, "POST", path, "application/json", invalid, 400).await;
    }
    server.abort();
}

#[tokio::test]
async fn real_http_reads_dates_and_selects_named_input_paths() {
    let (address, server) = start("dated-values").await;
    let (_, body) = send(address, "POST", "/dated-values?query=keep", "application/json", br#"{"when":"2026-01-02T03:04:05.123456+08:00","formatted":"03/01/2026 04:05:06.765432","user":{"name":"Ada","note":null,"secret":"remove"}}"#, 200).await;
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        value["dates"][0],
        json!([
            "2026-01-02 03:04:05.123456 +08:00",
            "+08:00",
            123_456,
            1_767_294_245_i64
        ])
    );
    assert_eq!(
        value["dates"][1],
        json!([
            "2026-01-03 04:05:06.765432 +09:00",
            "Asia/Tokyo",
            765_432,
            1_767_380_706_i64
        ])
    );
    let selected = json!({"user":{"name":"Ada","note":null},"query":"keep"});
    assert_eq!(value["selected"], selected);
    assert_eq!(value["array"], selected);
    assert_eq!(value["remainder"], selected);
    assert_eq!(
        value["collection"],
        json!([["user",{"name":"Ada","note":null}],["query","keep"]])
    );

    for (payload, expected) in [
        (
            br#"{"when":"2026-01-02 +1 day"}"#.as_slice(),
            json!([
                "2026-01-03 00:00:00.000000 +00:00",
                "UTC",
                0,
                1_767_398_400_i64
            ]),
        ),
        (
            br#"{"when":-0.25}"#,
            json!(["1969-12-31 23:59:59.750000 +00:00", "+00:00", 750_000, -1]),
        ),
        (br#"{"when":"   ","formatted":null}"#, Value::Null),
    ] {
        let (_, body) = send(
            address,
            "POST",
            "/dated-values",
            "application/json",
            payload,
            200,
        )
        .await;
        let date: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(date["dates"], json!([expected, null]));
    }
    for invalid in [
        br#"{"when":"not a date"}"#.as_slice(),
        br#"{"formatted":"wrong format"}"#,
    ] {
        send(
            address,
            "POST",
            "/dated-values",
            "application/json",
            invalid,
            400,
        )
        .await;
    }
    server.abort();
}

#[tokio::test]
async fn real_http_reads_calendar_intervals_and_explicit_units() {
    let (address, server) = start("intervals").await;
    for (body, expected) in [
        (
            br#"{"duration":"P1Y2M3DT4H5M6.123456S","seconds":"90.125suffix"}"#.as_slice(),
            json!([
                [1, 2, 3, 4, 5, 6, 123456, false, null],
                [0, 0, 0, 0, 0, 90, 125000, false, null]
            ]),
        ),
        (
            br#"{"duration":"2 hours 30 minutes","seconds":-1.25}"#,
            json!([
                [0, 0, 0, 2, 30, 0, 0, false, null],
                [0, 0, 0, 0, 0, -1, -250000, false, null]
            ]),
        ),
        (
            br#"{"duration":"2 days ago","seconds":" "}"#,
            json!([[0, 0, -2, 0, 0, 0, 0, false, null], null]),
        ),
        (br#"{"duration":3,"seconds":null}"#, json!([null, null])),
        (b"{}", json!([null, null])),
    ] {
        let path = if body == b"{}" {
            "/intervals"
        } else {
            "/intervals?duration=4%20hours"
        };
        let (_, actual) = send(address, "POST", path, "application/json", body, 200).await;
        assert_eq!(serde_json::from_str::<Value>(&actual).unwrap(), expected);
    }
    for body in [
        br#"{"duration":"P1Q"}"#.as_slice(),
        br#"{"invalid_unit":10}"#,
    ] {
        send(address, "POST", "/intervals", "application/json", body, 400).await;
    }
    server.abort();
}
