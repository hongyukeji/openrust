#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use openrust::{
    auth::{AuthError, AuthFuture, AuthIdentifier, AuthManager, AuthResult, Credentials},
    config::{ConfigError, Environment, Value},
    contracts::auth::{Authenticatable, StatefulGuard as _, UserProvider},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::{Level, LogContext},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Auth, Log, Route},
    },
};
use openrust_pail::{File, PailServiceProvider};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("pail-auth-{:032x}", fastrand::u128(..)));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join(".env"), "PAIL_TESTS=true\n").unwrap();
        File::new(path.join("storage/pail/auth.pail"))
            .create()
            .unwrap();
        Self(path)
    }
    fn records(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.0.join("storage/pail/auth.pail"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

struct User(String);
impl Authenticatable for User {
    fn getAuthIdentifierName(&self) -> &str {
        "id"
    }
    fn getAuthIdentifier(&self) -> AuthIdentifier {
        AuthIdentifier::String(self.0.clone())
    }
    fn getAuthPasswordName(&self) -> &str {
        "password"
    }
    fn getAuthPassword(&self) -> Option<&str> {
        None
    }
    fn getRememberToken(&self) -> Option<&str> {
        None
    }
    fn setRememberToken(&mut self, _: Option<String>) {}
    fn getRememberTokenName(&self) -> &str {
        "remember_token"
    }
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn getAuthAttribute(
        &self,
        name: &str,
    ) -> openrust::auth::AuthResult<Option<openrust::database::Value>> {
        Ok((name == "email")
            .then(|| openrust::database::Value::Text(format!("{}@example.test", self.0))))
    }
}

struct EmptyProvider;
impl UserProvider for EmptyProvider {
    fn retrieveById<'a>(
        &'a self,
        _: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok(None) })
    }
    fn retrieveByToken<'a>(
        &'a self,
        _: &'a AuthIdentifier,
        _: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok(None) })
    }
    fn updateRememberToken<'a>(
        &'a self,
        _: &'a mut dyn Authenticatable,
        _: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async { panic!("unexpected provider write") })
    }
    fn retrieveByCredentials<'a>(
        &'a self,
        _: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async { Ok(None) })
    }
    fn validateCredentials<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async { Ok(false) })
    }
    fn rehashPasswordIfRequired<'a>(
        &'a self,
        _: &'a mut dyn Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct Counts {
    resolved: AtomicUsize,
    cancelled: AtomicUsize,
}
struct Probe {
    prefix: &'static str,
    counts: Arc<Counts>,
}
impl ServiceProvider for Probe {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let auth = app.make::<AuthManager>().await?;
            auth.provider("empty", |_, _| Ok(Arc::new(EmptyProvider)));
            let counts = self.counts.clone();
            let prefix = self.prefix;
            auth.viaRequest("timing", move |request| -> AuthFuture<'_, _> {
                let counts = counts.clone();
                let id = request.route("id").unwrap().to_owned();
                Box::pin(async move {
                    counts.resolved.fetch_add(1, Ordering::SeqCst);
                    // Both kinds of nested logging must avoid recursive authentication.
                    Log::info("resolver sync", &LogContext::new()).unwrap();
                    Log::logWithRequest(request, Level::Info, "resolver async", &LogContext::new())
                        .await
                        .unwrap();
                    tokio::task::yield_now().await;
                    if id == "error" {
                        return Err(AuthError::InvalidIdentity);
                    }
                    if id == "cancel" && counts.cancelled.fetch_add(1, Ordering::SeqCst) == 0 {
                        std::future::pending::<()>().await;
                    }
                    Ok(Some(
                        Box::new(User(format!("{prefix}-{id}"))) as Box<dyn Authenticatable>
                    ))
                })
            });
            Ok(())
        })
    }
}
fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"testing","host":"127.0.0.1","port":0,"key":"a".repeat(32)}),
        ),
        (
            "session".into(),
            json!({"driver":"array","cookie":"pail_auth","lottery":[0,100]}),
        ),
        (
            "logging".into(),
            json!({"default":"file","channels":{"file":{"driver":"single","path":"written.log"},"filtered":{"driver":"null","level":"error"}}}),
        ),
        (
            "auth".into(),
            json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"timing"},"other":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"empty"}}}),
        ),
    ]))
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/timing/{id}", timing)?;
    Route::get("/failure/{id}", failure)
}
fn commands() -> Result<(), openrust::console::ConsoleError> {
    Artisan::command("timing:log", |_| {
        Log::info("artisan", &LogContext::new()).unwrap();
    })?;
    Ok(())
}
fn failure(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(std::io::Error::other("report probe")),
        })
    })
}
fn timing(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let id = request.route("id").unwrap().to_owned();
        if id == "quiet" {
            return Ok(Response::new(StatusCode::OK, Body::from("quiet")));
        }
        if id == "filtered" {
            Log::channel(Some("filtered"))
                .unwrap()
                .logWithRequest(request, Level::Info, "filtered", &LogContext::new())
                .await
                .unwrap();
            Log::logWithRequest(
                request,
                Level::Warning,
                "Deprecated probe",
                &LogContext::new(),
            )
            .await
            .unwrap();
            return Ok(Response::new(StatusCode::OK, Body::from("filtered")));
        }
        Log::info("before", &LogContext::new()).unwrap();
        if id == "cancel" {
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(10),
                    Log::logWithRequest(request, Level::Info, "cancelled", &LogContext::new())
                )
                .await
                .is_err()
            );
        }
        if id == "error" {
            assert!(
                Log::logWithRequest(request, Level::Info, "rejected", &LogContext::new())
                    .await
                    .is_err()
            );
            return Ok(Response::new(StatusCode::OK, Body::from("error handled")));
        }
        Log::logWithRequest(request, Level::Info, "first", &LogContext::new())
            .await
            .unwrap();
        Log::logWithRequest(request, Level::Info, "cached", &LogContext::new())
            .await
            .unwrap();
        if id == "sequence" {
            Auth::manager()
                .unwrap()
                .shouldUse(request, "other")
                .unwrap();
            Auth::sessionGuard(request, Some("other"))
                .unwrap()
                .login(Box::new(User("session".into())), false)
                .await
                .unwrap();
            Log::logWithRequest(request, Level::Info, "login", &LogContext::new())
                .await
                .unwrap();
            Auth::sessionGuard(request, Some("other"))
                .unwrap()
                .logout()
                .await
                .unwrap();
            Log::logWithRequest(request, Level::Info, "logout", &LogContext::new())
                .await
                .unwrap();
            Auth::manager().unwrap().shouldUse(request, "web").unwrap();
            Log::logWithRequest(request, Level::Info, "back", &LogContext::new())
                .await
                .unwrap();
            Artisan::callSilently("timing:log", &[]).await.unwrap();
            Log::logWithRequest(request, Level::Info, "after artisan", &LogContext::new())
                .await
                .unwrap();
        }
        Ok(Response::new(StatusCode::OK, Body::from("ok")))
    })
}
async fn server(
    root: &Root,
    prefix: &'static str,
    counts: Arc<Counts>,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(root.0.clone())
        .with_config(config)
        .with_providers(vec![
            Box::new(PailServiceProvider),
            Box::new(Probe { prefix, counts }),
        ])
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handleTransport(request));
    (
        address,
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }),
    )
}
async fn get(address: std::net::SocketAddr, path: &str, status: u16) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    socket.read_to_end(&mut bytes).await.unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).starts_with(&format!("HTTP/1.1 {status}")),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
}
fn origin(record: &serde_json::Value) -> &serde_json::Value {
    &record["context"]["__pail"]["origin"]
}
fn find<'a>(records: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    records
        .iter()
        .find(|record| record["message"] == name)
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn awaited_authentication_preserves_log_time_identity_and_application_request_scopes() {
    let left = Root::new();
    let right = Root::new();
    let lc = Arc::new(Counts::default());
    let rc = Arc::new(Counts::default());
    let (la, ls) = server(&left, "left", lc.clone()).await;
    let (ra, rs) = server(&right, "right", rc.clone()).await;
    get(la, "/timing/quiet", 200).await;
    get(la, "/timing/filtered", 200).await;
    assert_eq!(lc.resolved.load(Ordering::SeqCst), 0);
    tokio::join!(
        get(la, "/timing/sequence", 200),
        get(ra, "/timing/sequence", 200)
    );
    for (root, prefix, counts) in [(&left, "left", &lc), (&right, "right", &rc)] {
        let records = root.records();
        assert!(origin(find(&records, "before"))["auth_id"].is_null());
        assert!(origin(find(&records, "resolver sync"))["auth_id"].is_null());
        assert!(origin(find(&records, "resolver async"))["auth_id"].is_null());
        assert_eq!(
            origin(find(&records, "first"))["auth_id"],
            format!("{prefix}-sequence")
        );
        assert_eq!(
            origin(find(&records, "cached"))["auth_email"],
            format!("{prefix}-sequence@example.test")
        );
        assert_eq!(origin(find(&records, "login"))["auth_id"], "session");
        assert!(origin(find(&records, "logout"))["auth_id"].is_null());
        assert_eq!(
            origin(find(&records, "back"))["auth_id"],
            format!("{prefix}-sequence")
        );
        assert_eq!(origin(find(&records, "artisan"))["type"], "http");
        assert_eq!(origin(find(&records, "after artisan"))["type"], "http");
        assert_eq!(counts.resolved.load(Ordering::SeqCst), 1);
    }
    get(la, "/timing/cancel", 200).await;
    let records = left.records();
    assert!(
        !records
            .iter()
            .any(|record| record["message"] == "cancelled")
    );
    assert_eq!(origin(find(&records, "first"))["auth_id"], "left-sequence");
    assert!(
        records.iter().any(
            |record| record["message"] == "first" && origin(record)["auth_id"] == "left-cancel"
        )
    );
    assert_eq!(lc.cancelled.load(Ordering::SeqCst), 2);
    get(la, "/timing/error", 200).await;
    assert!(
        !left
            .records()
            .iter()
            .any(|record| record["message"] == "rejected")
    );
    get(la, "/failure/error", 500).await;
    get(la, "/failure/report", 500).await;
    let records = left.records();
    let report = records
        .iter()
        .find(|record| {
            record["context"]["status"] == 500 && origin(record)["auth_id"] == "left-report"
        })
        .unwrap();
    assert_eq!(origin(report)["auth_id"], "left-report");
    assert!(report["context"]["exception"].is_object());
    get(la, "/timing/fresh", 200).await;
    assert!(left.records().iter().any(|record| record["message"] == "first" && origin(record)["auth_id"] == "left-fresh"));
    std::fs::remove_file(right.0.join("storage/pail/auth.pail")).unwrap();
    get(ra, "/timing/no-session", 200).await;
    assert_eq!(rc.resolved.load(Ordering::SeqCst), 1);
    for task in [ls, rs] {
        task.abort();
        let _ = task.await;
    }
    for address in [la, ra] {
        drop(tokio::net::TcpListener::bind(address).await.unwrap());
    }
}
