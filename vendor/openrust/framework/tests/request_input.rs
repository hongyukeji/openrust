use std::{net::SocketAddr, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::ControllerDispatcher,
    support::{ProviderResult, ServiceProvider, facades::Route},
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
