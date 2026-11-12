use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::Environment,
    contracts::{http::Kernel as _, session::Session},
    foundation::{Application, BootstrapError, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Method, Request, Response, StatusCode},
    routing::{Router, RoutingError},
    support::facades::Route,
    view::view,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn web() -> Result<(), RoutingError> {
    Route::get("/token", token)?;
    Route::get("/errors", || view("errors-probe"))?;
    Route::get("/flash", flash)?;
    Route::r#match(&[Method::POST, Method::OPTIONS], "/update/{id}", update)?;
    Route::r#match(&[Method::POST], "/hooks/inbound", update)?;
    Ok(())
}

fn binding(_: &Application) -> Result<Router, RoutingError> {
    let mut router = Router::new();
    router.bind("id", |binding| {
        Box::pin(
            async move { Ok((binding.value() != "missing").then(|| binding.value().to_owned())) },
        )
    });
    Ok(router)
}

fn api() -> Result<(), RoutingError> {
    Route::r#match(&[Method::POST], "/probe", api_probe)?;
    Ok(())
}

fn api_probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Ok(Response::empty(StatusCode::NO_CONTENT)) })
}

fn token(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(
        async move { Response::json(StatusCode::OK, json!({"token":request.session()?.token()})) },
    )
}

fn flash(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request
            .session_mut()?
            .flash("errors", json!({"default":{"name":"Required"}}))?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

fn update(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let value = request.input(Some("value")).await?;
        Response::json(StatusCode::OK, json!({"value":value}))
    })
}

fn app(root: PathBuf, origin_only: bool, same_site: bool) -> Application {
    Application::configure(root)
        .withConfig(|_: &Path, _: &Environment| {
            Ok([
                ("app".into(), json!({"key":"a".repeat(32)})),
                (
                    "session".into(),
                    json!({"driver":"array","cookie":"csrf_session","lottery":[0,100]}),
                ),
            ]
            .into())
        })
        .withMiddleware(move |middleware| {
            middleware.preventRequestForgery(["hooks/*"], origin_only, same_site);
        })
        .withRouting(Routing::new(binding).web(web).api(api))
        .create()
        .unwrap()
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn start(
    root: PathBuf,
    origin_only: bool,
    same_site: bool,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = Kernel::new(app(root, origin_only, same_site));
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    (address, server)
}

async fn send(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut socket = TcpStream::connect(address).await.unwrap();
        let wire = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len());
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        response
    }).await.unwrap()
}

fn status(response: &str, expected: u16) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {expected} ")),
        "{response}"
    );
}

fn cookie<'a>(response: &'a str, name: &str) -> &'a str {
    let prefix = format!("set-cookie: {name}=");
    response
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap()
}

#[tokio::test]
async fn default_web_group_protects_writes_before_binding_and_preserves_request_input() {
    let (address, server) = start(std::env::temp_dir(), false, false).await;
    let initial = send(address, "GET", "/token", "", "").await;
    status(&initial, 200);
    let cookies = format!(
        "Cookie: csrf_session={}\r\n",
        cookie(&initial, "csrf_session").split(';').next().unwrap()
    );
    let xsrf = cookie(&initial, "XSRF-TOKEN");
    assert!(!xsrf.contains("httponly"));
    let token: Value = serde_json::from_str(initial.split_once("\r\n\r\n").unwrap().1).unwrap();
    let token = token["token"].as_str().unwrap();
    assert!(!xsrf.starts_with(token));
    let cipher = percent_encoding::percent_decode_str(xsrf.split(';').next().unwrap())
        .decode_utf8()
        .unwrap();

    for (request_headers, body, expected) in [
        (String::new(), "", 419),
        ("Sec-Fetch-Site: same-site\r\n".into(), "", 419),
        ("Sec-Fetch-Site: cross-site\r\n".into(), "", 419),
        ("Sec-Fetch-Site: same-origin\r\n".into(), "", 200),
        (
            "Sec-Fetch-Site: same-origin\r\nSec-Fetch-Site: cross-site\r\n".into(),
            "",
            419,
        ),
        (format!("X-CSRF-TOKEN: {token}\r\n"), "", 200),
        (format!("X-XSRF-TOKEN: {cipher}\r\n"), "", 200),
        ("X-XSRF-TOKEN: corrupt\r\n".into(), "", 419),
        (
            format!("X-CSRF-TOKEN: {token}\r\nContent-Type: application/x-www-form-urlencoded\r\n"),
            "_token=wrong",
            419,
        ),
    ] {
        let response = send(
            address,
            "POST",
            "/update/42",
            &(cookies.clone() + &request_headers),
            body,
        )
        .await;
        status(&response, expected);
        if expected == 419 {
            assert!(response.ends_with(r#"{"message":"CSRF token mismatch."}"#));
        }
    }
    let form = format!("_token={token}&value=retained");
    let headers = cookies.clone() + "Content-Type: application/x-www-form-urlencoded\r\n";
    let response = send(address, "POST", "/update/42", &headers, &form).await;
    status(&response, 200);
    assert!(response.ends_with(r#"{"value":"retained"}"#));

    status(
        &send(address, "POST", "/update/missing", &cookies, "").await,
        419,
    );
    status(
        &send(
            address,
            "POST",
            "/update/missing",
            &format!("{cookies}X-CSRF-TOKEN: {token}\r\n"),
            "",
        )
        .await,
        404,
    );
    status(
        &send(address, "OPTIONS", "/update/42", &cookies, "").await,
        200,
    );
    status(&send(address, "POST", "/hooks/inbound", "", "").await, 200);
    let api = send(address, "POST", "/api/probe", "", "").await;
    status(&api, 204);
    assert!(!api.contains("set-cookie:"));
    server.abort();
}

#[tokio::test]
async fn origin_only_and_session_errors_are_application_and_request_scoped() {
    let root = std::env::temp_dir().join(format!("openrust-csrf-{:016x}", fastrand::u64(..)));
    std::fs::create_dir_all(root.join("resources/views")).unwrap();
    std::fs::write(
        root.join("resources/views/errors-probe.blade.html"),
        "{{ $errors->default->name }}",
    )
    .unwrap();
    let (origin, origin_server) = start(root.clone(), true, true).await;
    let (other, other_server) = start(root.clone(), false, false).await;
    let flash = send(origin, "GET", "/flash", "", "").await;
    status(&flash, 204);
    assert!(!flash.contains("XSRF-TOKEN"));
    let cookie = format!(
        "Cookie: csrf_session={}\r\n",
        cookie(&flash, "csrf_session").split(';').next().unwrap()
    );
    let (errors, isolated) = tokio::join!(
        send(origin, "GET", "/errors", &cookie, ""),
        send(other, "GET", "/errors", &cookie, ""),
    );
    status(&errors, 200);
    status(&isolated, 200);
    assert!(errors.contains("Required"), "{errors}");
    assert!(!isolated.contains("Required"), "{isolated}");
    let aged = send(origin, "GET", "/errors", &cookie, "").await;
    assert!(!aged.contains("Required"));
    let denied = send(origin, "POST", "/update/42", "", "").await;
    status(&denied, 403);
    assert!(denied.ends_with(r#"{"message":"Origin mismatch."}"#));
    status(
        &send(
            origin,
            "POST",
            "/update/42",
            "Sec-Fetch-Site: same-site\r\n",
            "",
        )
        .await,
        200,
    );
    status(
        &send(
            other,
            "POST",
            "/update/42",
            "Sec-Fetch-Site: same-site\r\n",
            "",
        )
        .await,
        419,
    );
    origin_server.abort();
    other_server.abort();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn default_web_rejects_excluded_session_cookies_before_serving() {
    let app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok([(
                "session".into(),
                json!({"driver":"array","cookie":"csrf_session"}),
            )]
            .into())
        })
        .withMiddleware(|middleware| {
            middleware.encryptCookies(["csrf_session"]);
        })
        .create()
        .unwrap();
    let error = Kernel::new(app).bootstrap().await.unwrap_err();
    assert!(matches!(error, BootstrapError::ConfigurationFailed { .. }));
}
