use std::{
    hash::{DefaultHasher, Hash, Hasher},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::{
        container::Container as _, foundation::Application as _, http::Kernel as _,
        session::Session,
    },
    database::{Connection, DatabaseManager},
    foundation::{
        Application,
        configuration::Routing,
        http::{FormRequest, Kernel, ValidatedFormRequest},
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::facades::{Route, Validator},
    validation::{Data, RuleSet, ValidationException, ValidationResult},
    view::view,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn web() -> Result<(), RoutingError> {
    Route::get("/edit", inspect)?;
    Route::get("/display", || view("errors-probe"))?;
    Route::post("/submit", submit)?;
    Route::post("/explicit", explicit)?;
    Route::post("/manual", manual)?;
    Route::post("/custom", explicit)?;
    Ok(())
}

fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({
                "errors": request.session()?.get("errors"),
                "old": request.old(None),
                "nested": request.old(Some("profile.bio")),
            }),
        )
    })
}

struct Update {
    explicit: bool,
}

impl Default for Update {
    fn default() -> Self {
        Self { explicit: true }
    }
}

impl FormRequest for Update {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([("name", "required|string")])
    }

    fn errorBag(&self) -> &str {
        "profile"
    }

    fn redirectTo(&self) -> Option<&str> {
        self.explicit.then_some("/edit?from=explicit")
    }
}

fn submit(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Update { explicit: false }.validateResolved(request).await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

async fn explicit(_: ValidatedFormRequest<Update>) -> Result<Response, HttpError> {
    Ok(Response::empty(StatusCode::NO_CONTENT))
}

fn manual(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let data = Data::new(request.input(None).await?.unwrap_or_default())?;
        Validator::make(data, RuleSet::parse([("name", "required")])?)?
            .validateWithBag("manual")
            .await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

struct TestServer {
    http: tokio::task::JoinHandle<()>,
    mysql_session: Option<(Connection, String)>,
}

impl TestServer {
    async fn close(self) {
        self.http.abort();
        let _ = self.http.await;
        if let Some((db, table)) = self.mysql_session {
            db.getSchemaBuilder().drop(&table).await.unwrap();
        }
    }
}

fn session_table(root: &Path) -> String {
    let mut hash = DefaultHasher::new();
    root.hash(&mut hash);
    format!("validation_sessions_{:016x}", hash.finish())
}

async fn start(root: PathBuf) -> (SocketAddr, TestServer) {
    let using_mysql = std::env::var("OPENRUST_TEST_SESSION_MYSQL_URL").is_ok();
    let app = Application::configure(root)
        .with_config(|base, _| {
            let mysql = std::env::var("OPENRUST_TEST_SESSION_MYSQL_URL").ok();
            let driver = if mysql.is_some() { "database" } else { "array" };
            let mut config = std::collections::BTreeMap::from([
                ("app".into(), json!({"key":"a".repeat(32)})),
                ("session".into(), json!({"driver":driver,"table":session_table(base),"cookie":"form_session","lottery":[0,100]})),
            ]);
            if let Some(url) = &mysql {
                config.insert("database".into(), json!({
                    "default":"session_test","connections":{"session_test":{"url":url,"use_upsert_alias":true}}
                }));
            }
            Ok(config)
        })
        .with_exceptions(|exceptions| {
            exceptions.dontFlash(["profile.secret", "users.*.pin"]);
            exceptions.render::<ValidationException>(|_, request| {
                request
                    .is("custom")
                    .then(|| Response::empty(StatusCode::CONFLICT))
            });
            exceptions.respond(|response, _, _| {
                response.header(
                    ::http::HeaderName::from_static("x-handled"),
                    ::http::HeaderValue::from_static("yes"),
                )
            });
        })
        .with_routing(Routing::default().web(web))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let mysql_session = if using_mysql {
        let app = kernel.get_application();
        let table = session_table(app.base_path());
        let db = app
            .container()
            .make::<DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap();
        db.getSchemaBuilder()
            .create(&table, |table| {
                table.string("id", 255).primary();
                table.bigInteger("user_id").nullable();
                table.string("ip_address", 45).nullable();
                table.text("user_agent").nullable();
                table.longText("payload");
                table.integer("last_activity");
            })
            .await
            .unwrap();
        Some((db, table))
    } else {
        None
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (
        address,
        TestServer {
            http: server,
            mysql_session,
        },
    )
}

async fn send(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> String {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let wire = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",body.len());
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }).await.unwrap()
}

fn session_cookie(response: &str) -> String {
    let header = response
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: form_session="))
        .unwrap_or_else(|| panic!("Missing session cookie in {response}"));
    format!(
        "Cookie: form_session={}\r\n",
        header.split(';').next().unwrap()
    )
}

fn body(response: &str, status: u16) -> Value {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status} ")),
        "{response}"
    );
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn redirect(response: &str, target: &str) {
    assert!(response.starts_with("HTTP/1.1 302 "), "{response}");
    assert!(
        response.contains(&format!("\r\nlocation: {target}\r\n")),
        "{response}"
    );
    assert!(response.contains("\r\nx-handled: yes\r\n"));
}

const JSON: &str = "Content-Type: application/json\r\nSec-Fetch-Site: same-origin\r\n";

#[tokio::test]
async fn browser_validation_redirects_and_flashes_filtered_input_and_named_errors() {
    let root = tempfile::tempdir().unwrap();
    let (address, server) = start(root.path().to_path_buf()).await;
    let form = send(address, "GET", "/edit?step=1", "", "").await;
    let session = session_cookie(&form);
    let headers = session.clone() + JSON + "Referer: https://outside.example/steal\r\n";
    let submitted = json!({
        "_error_bag":"custom.form", "current_password":"private", "password":"private",
        "password_confirmation":"private", "profile.secret":"private",
        "profile":{"secret":"private","bio":null,"password":"private","title":"Ada"},
        "users":[{"pin":"private","id":1}]
    });
    let response = send(address, "POST", "/submit", &headers, &submitted.to_string()).await;
    redirect(&response, "/edit?step=1");

    let session = session_cookie(&response);
    let (first, unrelated) = tokio::join!(
        send(address, "GET", "/edit?step=1", &session, ""),
        send(address, "GET", "/edit", "", ""),
    );
    let first = body(&first, 200);
    assert_eq!(
        first["errors"],
        json!({"custom.form":{"name":["The name field is required."]}})
    );
    assert_eq!(
        first["old"],
        json!({
            "_error_bag":"custom.form","profile":{"bio":null,"title":"Ada"},"users":[{"id":1}]
        })
    );
    assert!(first["nested"].is_null());
    assert!(body(&unrelated, 200)["errors"].is_null());
    let next = send(address, "GET", "/edit", &session, "").await;
    assert_eq!(
        body(&next, 200),
        json!({"errors":null,"old":null,"nested":null})
    );
    let form = send(address, "GET", "/edit?step=2", "", "").await;
    let headers = session_cookie(&form) + JSON;
    let without_referer = send(address, "POST", "/submit", &headers, "{}").await;
    redirect(&without_referer, "/edit?step=2");
    server.close().await;
}

#[tokio::test]
async fn redirect_targets_custom_renderers_json_and_blade_keep_their_own_contracts() {
    let root = std::env::temp_dir().join(format!("openrust-form-{:016x}", fastrand::u64(..)));
    std::fs::create_dir_all(root.join("resources/views")).unwrap();
    std::fs::write(
        root.join("resources/views/errors-probe.blade.html"),
        "@foreach($errors->profile->name as $message)<p>{{ $message }}</p>@endforeach",
    )
    .unwrap();
    let (address, server) = start(root.clone()).await;
    let form = send(address, "GET", "/edit", "", "").await;
    let cookie = session_cookie(&form);
    let headers = cookie.clone() + JSON + &format!("Referer: http://{address}/display\r\n");
    let explicit = send(address, "POST", "/explicit", &headers, "{}").await;
    redirect(&explicit, "/edit?from=explicit");
    let display = send(address, "GET", "/display", &cookie, "").await;
    assert!(
        display.ends_with("<p>The name field is required.</p>"),
        "{display}"
    );

    let form_headers = headers.replace("application/json", "application/x-www-form-urlencoded");
    let referer = send(
        address,
        "POST",
        "/manual",
        &form_headers,
        "profile%5Bbio%5D=About+me",
    )
    .await;
    redirect(&referer, "/display");
    let form = send(address, "GET", "/edit", &cookie, "").await;
    let values = body(&form, 200);
    assert_eq!(values["old"], json!({"profile":{"bio":"About me"}}));
    assert_eq!(
        values["errors"],
        json!({"manual":{"name":["The name field is required."]}})
    );

    let json_error = send(
        address,
        "POST",
        "/submit",
        &(cookie.clone() + JSON + "Accept: application/json\r\n"),
        "{}",
    )
    .await;
    assert!(body(&json_error, 422)["errors"]["name"].is_array());
    let custom = send(address, "POST", "/custom", &(cookie.clone() + JSON), "{}").await;
    assert!(custom.starts_with("HTTP/1.1 409 "));
    assert!(custom.contains("\r\nx-handled: yes\r\n"));
    let clean = send(address, "GET", "/edit", &cookie, "").await;
    assert!(body(&clean, 200)["errors"].is_null());
    server.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
