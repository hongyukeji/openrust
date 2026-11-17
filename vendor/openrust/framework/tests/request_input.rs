use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{
        Body, HeaderMap, HttpError, HttpFuture, Method, Request, Response, StatusCode,
        StreamedEvent, StreamedJson,
    },
    routing::{
        ControllerDispatcher, ResourceAction, ResourceController, RouteGroup, Router, RoutingError,
    },
    support::{BackedEnum, ProviderResult, ServiceProvider, facades::Route},
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct EchoController(&'static str);

macro_rules! resource_methods {
    ($($method:ident),+) => {$ (
        fn $method<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
            Box::pin(async move {
                Response::json(StatusCode::OK, json!([self.0, stringify!($method), request.route("photo"), request.route("album"), request.route("image")]))
            })
        }
    )+};
}

impl EchoController {
    resource_methods!(index, create, store, edit, update, destroy, member);
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

fn photo_resource() -> ResourceController<EchoController> {
    use ResourceAction::*;
    ResourceController::new()
        .action(Index, EchoController::index)
        .action(Create, EchoController::create)
        .action(Store, EchoController::store)
        .action(Show, EchoController::member)
        .action(Edit, EchoController::edit)
        .action(Update, EchoController::update)
        .action(Destroy, EchoController::destroy)
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

fn stream_json(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let users = futures_util::stream::iter([Ok::<_, std::io::Error>(1), Ok(2)]);
        let empty = futures_util::stream::empty::<Result<i32, std::io::Error>>();
        let counts = futures_util::stream::iter([Ok::<_, std::io::Error>(("active", 2))]);
        Ok(Response::streamJson(
            StatusCode::OK,
            StreamedJson::object([
                ("users", StreamedJson::items(users)),
                ("counts", StreamedJson::entries(counts)),
                (
                    "meta",
                    StreamedJson::array([
                        StreamedJson::value("ready")?,
                        StreamedJson::items(empty),
                    ]),
                ),
            ]),
        ))
    })
}

fn event_stream(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let events = futures_util::stream::iter([
            Ok::<_, HttpError>(StreamedEvent::message("line\r\nnext\rfinal")),
            StreamedEvent::json("snapshot", json!({"ready":true})),
        ]);
        Ok(Response::eventStream(events))
    })
}

fn stream_download(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let content = futures_util::stream::iter([Ok::<_, std::io::Error>("first"), Ok("second")]);
        Response::streamDownload(content, Some("résumé.txt"), HeaderMap::new(), "attachment")
    })
}

fn response_factory(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use axum::http::{HeaderName, HeaderValue};
    use openrust::support::facades::View;

    Box::pin(async move {
        match request.uri().path() {
            "/response.jsonp" => {
                let callback = request.query(Some("callback"))?;
                let callback = callback
                    .as_ref()
                    .map(|value| value.as_str().ok_or(HttpError::InvalidRequest))
                    .transpose()?;
                Response::jsonp(StatusCode::OK, callback, "café / 😀 </script>")
            }
            "/response.md" => {
                let status = if request.query(Some("empty"))?.is_some() {
                    StatusCode::NOT_MODIFIED
                } else {
                    StatusCode::ACCEPTED
                };
                Ok(Response::markdown(status, "# Heading\n"))
            }
            "/response-empty" => Ok(Response::noContent().header(
                HeaderName::from_static("x-probe"),
                HeaderValue::from_static("empty"),
            )),
            _ => {
                let view = View::first(&["missing", "answer"], Default::default())
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?
                    .with("name", "<Ada>");
                let mut headers = HeaderMap::new();
                headers.insert(
                    HeaderName::from_static("x-probe"),
                    HeaderValue::from_static("view"),
                );
                Response::view(StatusCode::CREATED, view, headers, request).await
            }
        }
    })
}

fn cache_response(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    use axum::http::{HeaderName, HeaderValue};

    Box::pin(async move {
        Ok(match request.uri().path() {
            "/cacheable" => {
                let debug = HeaderName::from_static("x-debug");
                let remove = HeaderName::from_static("x-remove");
                Response::new(StatusCode::OK, Body::from("cache me"))
                    .header(debug.clone(), HeaderValue::from_static("one"))
                    .header(remove.clone(), HeaderValue::from_static("two"))
                    .withoutHeaders([debug, remove])
            }
            "/cache-date" => Response::new(StatusCode::OK, Body::from("date")),
            "/cache-manual" => Response::new(StatusCode::OK, Body::from("manual")).header(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-transform, ext=\"a,b\""),
            ),
            _ => Response::empty(StatusCode::NOT_FOUND),
        })
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
    start_at(label, std::env::temp_dir()).await
}

async fn start_at(label: &'static str, root: PathBuf) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root)
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
            Route::resource("photos", photo_resource())?;
            Route::apiResource(
                "api/albums.photos",
                photo_resource()
                    .shallow()
                    .except(&[ResourceAction::Destroy])
                    .parameters(&[("photos", "image")])
                    .names(&[(ResourceAction::Show, "pictures.read")])
                    .scoped(&[("image", "slug")])
                    .configure(|action, route| {
                        if matches!(action, ResourceAction::Show | ResourceAction::Update) {
                            route.where_number("image")?;
                        }
                        Ok(())
                    }),
            )?;
            Route::singleton("profile", photo_resource())?;
            Route::apiSingleton(
                "api/albums.thumbnail",
                photo_resource()
                    .creatable()
                    .shallow()
                    .parameters(&[("albums", "photo")])
                    .scoped(&[("photo", "slug")]),
            )?;
            Route::singleton("preferences", photo_resource().destroyable())?;
            Route::get("/stream.json", stream_json)?;
            Route::get("/stream.events", event_stream)?;
            Route::get("/stream.download", stream_download)?;
            for path in [
                "/response.jsonp",
                "/response.md",
                "/response-empty",
                "/response-view",
            ] {
                Route::get(path, response_factory)?;
            }
            Route::group(
                RouteGroup::default().middleware_name(
                    "cache.headers:public;max_age=2628000;etag;last_modified=1704067200",
                ),
                |router| {
                    router.r#match(
                        &[Method::GET, Method::HEAD, Method::POST],
                        "/cacheable",
                        cache_response,
                    )?;
                    Ok(())
                },
            )?;
            Route::getWith("/cache-date", cache_response, |route| {
                route.middleware_name("cache.headers:last_modified=2024-01-01 00:00:00 UTC");
                Ok(())
            })?;
            Route::getWith("/cache-stream", stream, |route| {
                route.middleware_name("cache.headers:s_maxage=600;etag");
                Ok(())
            })?;
            Route::getWith("/cache-manual", cache_response, |route| {
                use openrust::http::middleware::SetCacheHeaders;
                route.middleware_name(SetCacheHeaders::using(
                    "s_maxage=30;private;no_transform=0;etag=manual",
                ));
                Ok(())
            })?;
            Route::getWith("/cache-invalid-status", cache_response, |route| {
                route.middleware_name("cache.headers:public;etag");
                Ok(())
            })?;
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
    send_with_headers(address, method, path, media, body, status, "").await
}

async fn send_with_headers(
    address: SocketAddr,
    method: &str,
    path: &str,
    media: &str,
    body: &[u8],
    status: u16,
    additional: &str,
) -> (String, String) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let header = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n{additional}\r\n",
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

#[tokio::test]
async fn real_http_dispatches_resource_and_api_resource_actions() {
    let (address, server) = start("resource").await;
    for (method, path, expected) in [
        (
            "GET",
            "/photos",
            json!(["resource", "index", null, null, null]),
        ),
        (
            "GET",
            "/photos/create",
            json!(["resource", "create", null, null, null]),
        ),
        (
            "POST",
            "/photos",
            json!(["resource", "store", null, null, null]),
        ),
        (
            "GET",
            "/photos/2",
            json!(["resource", "member", "2", null, null]),
        ),
        (
            "GET",
            "/photos/2/edit",
            json!(["resource", "edit", "2", null, null]),
        ),
        (
            "PUT",
            "/photos/2",
            json!(["resource", "update", "2", null, null]),
        ),
        (
            "PATCH",
            "/photos/2",
            json!(["resource", "update", "2", null, null]),
        ),
        (
            "DELETE",
            "/photos/2",
            json!(["resource", "destroy", "2", null, null]),
        ),
        (
            "GET",
            "/api/albums/7/photos",
            json!(["resource", "index", null, "7", null]),
        ),
        (
            "POST",
            "/api/albums/7/photos",
            json!(["resource", "store", null, "7", null]),
        ),
        (
            "GET",
            "/api/photos/3",
            json!(["resource", "member", null, null, "3"]),
        ),
        (
            "PATCH",
            "/api/photos/3",
            json!(["resource", "update", null, null, "3"]),
        ),
    ] {
        let (_, body) = send(address, method, path, "", b"", 200).await;
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), expected);
    }
    for path in [
        "/api/albums/7/photos/create",
        "/api/photos/3/edit",
        "/api/albums/7/photos/3",
        "/api/photos/create",
    ] {
        send(address, "GET", path, "", b"", 404).await;
    }
    send(address, "DELETE", "/api/photos/3", "", b"", 405).await;
    server.abort();
}

#[test]
fn resource_registration_preserves_names_binding_fields_and_atomic_failure() {
    let mut router = Router::new();
    router.resource("photos", photo_resource()).unwrap();
    let original = router.getRoutes();
    assert_eq!(original.len(), 7);
    assert!(
        original
            .iter()
            .any(|route| route.name.as_deref() == Some("photos.show")
                && route.action.ends_with("EchoController@show"))
    );
    assert!(matches!(
        router.resource(
            "photos",
            ResourceController::new().action(ResourceAction::Index, EchoController::index),
        ),
        Err(RoutingError::MissingResourceAction {
            action: "create",
            ..
        })
    ));
    assert_eq!(router.getRoutes(), original);
    assert!(
        router
            .resource(
                "photos",
                photo_resource().configure(|action, _| {
                    if action == ResourceAction::Store {
                        return Err(RoutingError::InvalidAction);
                    }
                    Ok(())
                }),
            )
            .is_err()
    );
    assert_eq!(router.getRoutes(), original);

    router
        .apiResource(
            "albums.photos",
            photo_resource()
                .only(&[ResourceAction::Index, ResourceAction::Show])
                .shallow()
                .parameters(&[("photos", "image")])
                .scoped(&[("image", "slug")])
                .names(&[(ResourceAction::Show, "pictures.read")]),
        )
        .unwrap();
    let routes = router.getRoutes();
    assert!(
        routes
            .iter()
            .any(|route| route.name.as_deref() == Some("albums.photos.index")
                && route.uri == "albums/{album}/photos")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.name.as_deref() == Some("pictures.read")
                && route.uri == "photos/{image:slug}")
    );
    router
        .apiResource(
            "api/forms",
            photo_resource().only(&[ResourceAction::Create]),
        )
        .unwrap();
    assert!(
        router
            .getRoutes()
            .iter()
            .any(|route| route.method == "GET|HEAD"
                && route.uri == "api/forms/create"
                && route.name.as_deref() == Some("forms.create"))
    );
}

#[tokio::test]
async fn real_http_dispatches_singletons_with_lifecycle_and_shallow_paths() {
    let (address, server) = start("singleton").await;
    for (method, path, action, parent) in [
        ("GET", "/profile", "member", None),
        ("GET", "/profile/edit", "edit", None),
        ("PUT", "/profile", "update", None),
        ("PATCH", "/profile", "update", None),
        ("DELETE", "/preferences", "destroy", None),
        ("GET", "/api/albums/7/thumbnail", "member", Some("7")),
        ("POST", "/api/albums/7/thumbnail", "store", Some("7")),
        ("PATCH", "/api/thumbnail", "update", None),
        ("DELETE", "/api/thumbnail", "destroy", None),
    ] {
        let (_, body) = send(address, method, path, "", b"", 200).await;
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!(["singleton", action, parent, null, null])
        );
    }
    for (method, path, status) in [
        ("POST", "/profile", 405),
        ("DELETE", "/profile", 405),
        ("POST", "/preferences", 405),
        ("GET", "/profile/3", 404),
        ("GET", "/profile/create", 404),
        ("GET", "/api/albums/7/thumbnail/create", 404),
        ("GET", "/api/thumbnail/edit", 404),
        ("GET", "/api/thumbnail", 405),
        ("PATCH", "/api/albums/7/thumbnail", 405),
    ] {
        send(address, method, path, "", b"", status).await;
    }
    server.abort();
}

#[test]
fn singleton_routes_apply_explicit_selection_and_keep_binding_metadata() {
    let mut router = Router::new();
    router
        .apiSingleton(
            "pages.summary",
            photo_resource()
                .only(&[ResourceAction::Edit])
                .names(&[(ResourceAction::Edit, "summary.change")]),
        )
        .unwrap();
    let routes = router.getRoutes();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].method, "GET|HEAD");
    assert_eq!(routes[0].uri, "pages/{page}/summary/edit");
    assert_eq!(routes[0].name.as_deref(), Some("summary.change"));
    router
        .singleton(
            "profiles.card",
            photo_resource()
                .creatable()
                .destroyable()
                .scoped(&[("profile", "slug")]),
        )
        .unwrap();
    let routes = router.getRoutes();
    assert_eq!(routes.len(), 7);
    for (method, path) in [
        ("GET|HEAD", "profiles/{profile:slug}/card/create"),
        ("POST", "profiles/{profile:slug}/card"),
        ("DELETE", "profiles/{profile:slug}/card"),
    ] {
        assert!(
            routes
                .iter()
                .any(|route| route.method == method && route.uri == path),
            "{method} {path}"
        );
    }
}

fn chunked_content(mut body: &str) -> String {
    let mut result = String::new();
    loop {
        let (length, next) = body.split_once("\r\n").unwrap();
        let length = usize::from_str_radix(length, 16).unwrap();
        if length == 0 {
            return result;
        }
        result.push_str(&next[..length]);
        body = next[length..].strip_prefix("\r\n").unwrap();
    }
}

#[tokio::test]
async fn real_http_streams_json_events_and_downloads() {
    let (address, server) = start("streams").await;
    let (headers, body) = send(address, "GET", "/stream.json", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-type: application/json"));
    assert!(headers.contains("\r\ntransfer-encoding: chunked"));
    assert_eq!(
        serde_json::from_str::<Value>(&chunked_content(&body)).unwrap(),
        json!({"users":[1,2],"counts":{"active":2},"meta":["ready",[]]})
    );
    let (_, body) = send(address, "HEAD", "/stream.json", "", b"", 200).await;
    assert!(body.is_empty());

    let (headers, body) = send(address, "GET", "/stream.events", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-type: text/event-stream"));
    assert!(headers.contains("\r\ncache-control: no-cache"));
    assert!(headers.contains("\r\nx-accel-buffering: no"));
    assert_eq!(
        chunked_content(&body),
        "event: update\ndata: line\ndata: next\ndata: final\n\nevent: snapshot\ndata: {\"ready\":true}\n\nevent: update\ndata: </stream>\n\n"
    );

    let (headers, body) = send(address, "GET", "/stream.download", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-disposition: attachment; filename=resume.txt; filename*=utf-8''r%C3%A9sum%C3%A9.txt"));
    assert_eq!(chunked_content(&body), "firstsecond");
    server.abort();
}

#[tokio::test]
async fn response_streams_preserve_failure_and_lazy_sources() {
    use futures_util::{StreamExt as _, stream};

    let (release, ready) = tokio::sync::oneshot::channel::<()>();
    let source = stream::once(async {
        ready.await.unwrap();
        Ok::<_, std::io::Error>(1)
    });
    let mut response = Response::streamJson(StatusCode::OK, StreamedJson::items(source));
    let mut body = response.take_body().into_data_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(1), body.next()).await;
    assert_eq!(first.unwrap().unwrap().unwrap().as_ref(), b"[");
    release.send(()).unwrap();
    assert_eq!(body.next().await.unwrap().unwrap().as_ref(), b"1");
    assert_eq!(body.next().await.unwrap().unwrap().as_ref(), b"]");

    let source = stream::iter([Err::<StreamedEvent, _>(std::io::Error::other(
        "ended early",
    ))]);
    let mut response = Response::eventStream(source);
    let mut body = response.take_body().into_data_stream();
    assert!(body.next().await.unwrap().is_err());
    assert!(body.next().await.is_none());

    let empty = || stream::empty::<Result<StreamedEvent, std::io::Error>>();
    let mut response = Response::eventStreamWithEnd(empty(), None);
    assert!(
        response
            .take_body()
            .into_data_stream()
            .next()
            .await
            .is_none()
    );
    let end = StreamedEvent::new("done", "ok").unwrap();
    let mut response = Response::eventStreamWithEnd(empty(), Some(end));
    let mut body = response.take_body().into_data_stream();
    assert_eq!(
        body.next().await.unwrap().unwrap().as_ref(),
        b"event: done\ndata: ok\n\n"
    );
    assert!(body.next().await.is_none());

    assert!(StreamedEvent::new("injected\nevent", "data").is_err());
    assert!(
        Response::streamDownload(
            stream::empty::<Result<String, std::io::Error>>(),
            Some("../unsafe"),
            HeaderMap::new(),
            "attachment"
        )
        .is_err()
    );
}

#[tokio::test]
async fn real_http_removes_headers_and_applies_cache_validators() {
    let (address, server) = start("cache-headers").await;
    let (headers, body) = send(address, "GET", "/cacheable", "", b"", 200).await;
    // Fixed Laravel / PHP 8.5 hash('xxh128', 'cache me').
    let tag = "\"a9c4c77c2e3f060abb5bb0773cfe00bf\"";
    assert_eq!(body, "cache me");
    assert!(headers.contains("\r\ncache-control: max-age=2628000, public"));
    assert!(headers.contains(&format!("\r\netag: {tag}")));
    assert!(headers.contains("\r\nlast-modified: Mon, 01 Jan 2024 00:00:00 GMT"));
    assert!(!headers.contains("\r\nx-debug:") && !headers.contains("\r\nx-remove:"));

    let fields = format!("If-None-Match: \"other,tag\"\r\nIf-None-Match: W/{tag}\r\n");
    let (headers, body) =
        send_with_headers(address, "HEAD", "/cacheable", "", b"", 304, &fields).await;
    assert!(body.is_empty());
    assert!(headers.contains(&format!("\r\netag: {tag}")));
    assert!(!headers.contains("\r\nlast-modified:") && !headers.contains("\r\ncontent-type:"));
    let (_, body) = send_with_headers(
        address,
        "GET",
        "/cacheable",
        "",
        b"",
        200,
        "If-None-Match: \"different\"\r\nIf-Modified-Since: Mon, 01 Jan 2024 00:00:00 GMT\r\n",
    )
    .await;
    assert_eq!(body, "cache me");

    let (headers, body) = send_with_headers(
        address,
        "GET",
        "/cache-date",
        "",
        b"",
        304,
        "If-Modified-Since: Mon, 01 Jan 2024 00:00:00 GMT\r\n",
    )
    .await;
    assert!(body.is_empty());
    assert!(headers.contains("\r\ncache-control: private, must-revalidate"));

    let (headers, body) = send(address, "GET", "/cache-stream", "", b"", 200).await;
    assert!(headers.contains("\r\ncache-control: public, s-maxage=600"));
    assert!(!headers.contains("\r\netag:"));
    assert_eq!(chunked_content(&body), "firstsecond");

    let (headers, body) = send_with_headers(
        address,
        "GET",
        "/cache-manual",
        "",
        b"",
        304,
        "If-None-Match: *\r\n",
    )
    .await;
    assert!(body.is_empty());
    assert!(headers.contains("\r\netag: \"manual\""));
    assert!(headers.contains("\r\ncache-control: ext=\"a,b\", private, s-maxage=30"));

    for (method, path, status) in [
        ("POST", "/cacheable", 200),
        ("GET", "/cache-invalid-status", 404),
    ] {
        let (headers, _) = send(address, method, path, "", b"", status).await;
        assert!(!headers.contains("\r\ncache-control:") && !headers.contains("\r\netag:"));
    }
    server.abort();
}

#[tokio::test]
async fn real_http_serves_jsonp_markdown_empty_and_view_responses() {
    let root = tempfile::tempdir().unwrap();
    let views = root.path().join("resources/views");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(views.join("answer.blade.html"), "Hello {{ $name }}!").unwrap();
    let (address, server) = start_at("response-factory", root.path().into()).await;

    let (headers, body) = send(
        address,
        "GET",
        "/response.jsonp?callback=app.handlers%5B2%5D",
        "",
        b"",
        200,
    )
    .await;
    assert!(headers.contains("\r\ncontent-type: text/javascript; charset=utf-8"));
    assert_eq!(
        body,
        r#"/**/app.handlers[2]("caf\u00e9 \/ \ud83d\ude00 <\/script>");"#
    );
    let (_, body) = send(
        address,
        "GET",
        "/response.jsonp?callback=%E5%BA%94%E7%AD%94%5B%22done%22%5D",
        "",
        b"",
        200,
    )
    .await;
    assert!(body.starts_with("/**/应答[\"done\"]("));
    let (headers, body) = send(address, "GET", "/response.jsonp", "", b"", 200).await;
    assert!(headers.contains("\r\ncontent-type: application/json"));
    assert_eq!(
        serde_json::from_str::<String>(&body).unwrap(),
        "café / 😀 </script>"
    );
    for path in [
        "/response.jsonp?callback=alert%281%29",
        "/response.jsonp?callback=class",
        "/response.jsonp?callback=valid%0a",
        "/response.jsonp?callback%5B%5D=valid",
    ] {
        send(address, "GET", path, "", b"", 400).await;
    }

    let (headers, body) = send(address, "GET", "/response.md", "", b"", 202).await;
    assert!(headers.contains("\r\ncontent-type: text/markdown; charset=utf-8"));
    assert_eq!(body, "# Heading\n");
    let (_, body) = send(address, "HEAD", "/response.md", "", b"", 202).await;
    assert!(body.is_empty());
    let (headers, body) = send(address, "GET", "/response.md?empty=1", "", b"", 304).await;
    assert!(!headers.contains("\r\ncontent-type:") && body.is_empty());

    let (headers, body) = send(address, "GET", "/response-empty", "", b"", 204).await;
    assert!(headers.contains("\r\nx-probe: empty"));
    assert!(!headers.contains("\r\ncontent-type:") && body.is_empty());

    let (headers, body) = send(address, "GET", "/response-view", "", b"", 201).await;
    assert!(headers.contains("\r\nx-probe: view"));
    assert!(headers.contains("\r\ncontent-type: text/html; charset=utf-8"));
    assert_eq!(body, "Hello &lt;Ada&gt;!");
    server.abort();
}
