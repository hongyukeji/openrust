use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{cookie::Factory as _, http::Kernel as _},
    cookie::{
        CookieJar, CookieOptions,
        middleware::{AddQueuedCookiesToResponse, EncryptCookies},
    },
    encryption::{Cipher, Encrypter},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{MiddlewareEntry, RoutingError},
    support::facades::{Cookie, Route},
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn set(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let value = request.route("value").unwrap_or("sample");
        let defaults = CookieOptions::default();
        Cookie::queue(Cookie::make("queued", "overwritten", 0, &defaults)?)?;
        Cookie::queue(Cookie::make("queued", value, 15, &defaults)?)?;
        Cookie::queue(Cookie::make("ignored", "discard", 0, &defaults)?)?;
        Cookie::unqueue("ignored", None)?;
        Cookie::queue(Cookie::make("plain", "a+b c", 0, &defaults)?)?;
        assert_eq!(
            Cookie::queued("queued", None)?.unwrap().value(),
            Some(value)
        );
        Response::empty(StatusCode::NO_CONTENT).cookie(
            &Cookie::make(
                "direct",
                "a%20b",
                0,
                &CookieOptions {
                    raw: true,
                    ..defaults
                },
            )?,
            request,
        )
    })
}

fn read(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move { Response::json(StatusCode::OK, request.cookie(None)?) })
}

fn expire(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Cookie::expire("queued", None, None)?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn register() -> Result<(), RoutingError> {
    Route::get("/set/{value}", set)?;
    Route::get("/read", read)?;
    Route::get("/expire", expire)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn start(key: u8, previous: Option<u8>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "session".into(),
                json!({
                    "path": "/", "domain": "example.test", "secure": true, "same_site": "strict"
                }),
            )]
            .into())
        })
        .withMiddleware(move |middleware| {
            let encrypter = Encrypter::new([key; 32], Cipher::Aes256Cbc)
                .unwrap()
                .previousKeys(previous.into_iter().map(|byte| vec![byte; 32]).collect())
                .unwrap();
            let mut encrypt = EncryptCookies::new(Arc::new(encrypter));
            encrypt.disableFor(["plain"]);
            middleware.group(
                "web",
                [
                    MiddlewareEntry::Instance(Arc::new(encrypt)),
                    MiddlewareEntry::Instance(Arc::new(AddQueuedCookiesToResponse)),
                ],
            );
        })
        .withRouting(Routing::default().web(register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server)
}

async fn send(address: SocketAddr, path: &str, cookies: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: example.test\r\nCookie: {cookies}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut wire = String::new();
        stream.read_to_string(&mut wire).await.unwrap();
        wire
    }).await.unwrap()
}

fn cookie_header(response: &str, name: &str) -> String {
    response
        .lines()
        .find_map(|line| {
            let header = line.strip_prefix("set-cookie: ")?;
            header
                .starts_with(&format!("{name}="))
                .then(|| header.to_owned())
        })
        .unwrap_or_else(|| panic!("cookie {name} missing"))
}

fn pair(header: &str) -> &str {
    header.split_once(';').unwrap().0
}

fn json_body(response: &str) -> Value {
    assert!(response.starts_with("HTTP/1.1 200 "));
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[tokio::test]
async fn cookies_round_trip_and_reject_tampering_across_real_http_requests() {
    let (original, first_server) = start(b'a', None).await;
    let (rotated, rotated_server) = start(b'b', Some(b'a')).await;
    let (unrelated, unrelated_server) = start(b'c', None).await;
    let (first, second) = tokio::join!(
        send(original, "/set/first", ""),
        send(original, "/set/second", ""),
    );
    assert!(first.starts_with("HTTP/1.1 204 "));
    assert_eq!(first.matches("set-cookie: ").count(), 3);
    assert!(!first.contains("overwritten"));
    assert!(!first.contains("ignored="));
    let queued = cookie_header(&first, "queued");
    let direct = cookie_header(&first, "direct");
    let plain = cookie_header(&first, "plain");
    assert!(queued.contains("; domain=example.test; secure; httponly; samesite=strict"));
    assert!(queued.contains("Max-Age="));
    assert!(plain.starts_with("plain=a%2Bb%20c;"));
    let wire = format!("{}; {}; {}", pair(&queued), pair(&direct), pair(&plain));
    let response = send(rotated, "/read", &wire).await;
    assert_eq!(
        json_body(&response),
        json!({"queued":"first","direct":"a%20b","plain":"a+b c"})
    );
    assert!(!response.contains("set-cookie:"));
    let second_cookie = cookie_header(&second, "queued");
    let response = send(original, "/read", pair(&second_cookie)).await;
    assert_eq!(json_body(&response)["queued"], "second");
    let replay = format!("direct={}", pair(&queued).split_once('=').unwrap().1);
    let response = send(original, "/read", &replay).await;
    assert_eq!(json_body(&response), json!({"direct":null}));
    let response = send(unrelated, "/read", pair(&queued)).await;
    assert_eq!(json_body(&response), json!({"queued":null}));
    let mut tampered = pair(&queued).to_owned();
    let index = tampered.find('=').unwrap() + 1;
    let replacement = if tampered.as_bytes()[index] == b'A' {
        "B"
    } else {
        "A"
    };
    tampered.replace_range(index..index + 1, replacement);
    let response = send(original, "/read", &tampered).await;
    assert_eq!(json_body(&response), json!({"queued":null}));
    let response = send(original, "/read", "plain=first; plain=second").await;
    assert_eq!(json_body(&response), json!({"plain":"first"}));
    let expired = send(original, "/expire", "").await;
    assert!(cookie_header(&expired, "queued").contains("Max-Age=0"));

    for server in [first_server, rotated_server, unrelated_server] {
        server.abort();
    }
}

#[test]
fn cookie_expiry_and_raw_attribute_validation_match_factory_contract() {
    let jar = CookieJar::new();
    let options = CookieOptions::default();
    let forever = jar.forever("sample", "value", &options).unwrap();
    assert!(
        forever
            .expires()
            .unwrap()
            .duration_since(std::time::SystemTime::now())
            .unwrap()
            .as_secs()
            >= 34_559_998
    );
    let deleted = jar.forget("sample", Some("/custom"), None).unwrap();
    let header = deleted.to_header(false).unwrap();
    assert!(header.to_str().unwrap().starts_with("sample=deleted;"));
    assert!(
        header
            .to_str()
            .unwrap()
            .contains("; Max-Age=0; path=/custom;")
    );
    assert!(jar.make("sample", "value", i64::MAX, &options).is_err());
    assert!(
        jar.make(
            "sample",
            "bad; path=/",
            0,
            &CookieOptions {
                raw: true,
                ..options
            }
        )
        .is_err()
    );
}
