use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::{ArrayStore, Repository},
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade as _, Route},
    },
};
use openrust_fortify::{
    TwoFactorAuthenticationProvider, contracts::TwoFactorAuthenticationProvider as _,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};
struct Engine;
impl openrust::support::facades::Facade for Engine {
    type Service = TwoFactorAuthenticationProvider;
    fn getFacadeAccessor() -> openrust::container::ServiceKey {
        openrust::container::ServiceKey::typed::<TwoFactorAuthenticationProvider>()
    }
}
struct Provider;
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singleton::<TwoFactorAuthenticationProvider, _>(|_| {
                    Ok(Arc::new(
                        TwoFactorAuthenticationProvider::new(Some(Arc::new(Repository::new(
                            Arc::new(ArrayStore::new()),
                        ))))
                        .withClock(|| Ok(1_234_567_890)),
                    ))
                })?;
            Ok(())
        })
    }
}
fn verify(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let provider = Engine::getFacadeRoot()?;
        let secret = request
            .query(Some("secret"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let code = request
            .query(Some("code"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        match provider.verify(&secret, &code).await {
            Ok(true) => Ok(Response::noContent()),
            Ok(false) => Response::json(
                StatusCode::UNPROCESSABLE_ENTITY,
                serde_json::json!({"valid":false}),
            ),
            Err(error) => Response::json(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error":error.to_string()}),
            ),
        }
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/verify", verify)?;
    Ok(())
}
async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}
async fn start() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withProviders(vec![Box::new(Provider)])
        .withRouting(Routing::default().api(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        addr,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}
async fn send(addr: SocketAddr, secret: &str, code: &str, status: u16) {
    timeout(Duration::from_secs(10),async {
 let mut socket=TcpStream::connect(addr).await.unwrap();let wire=format!("GET /api/verify?secret={secret}&code={code} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
 socket.write_all(wire.as_bytes()).await.unwrap();let mut response=String::new();socket.read_to_string(&mut response).await.unwrap();assert!(response.starts_with(&format!("HTTP/1.1 {status} ")),"{response}");
 }).await.unwrap();
}
#[tokio::test]
async fn real_http_verification_replay_errors_and_application_isolation() {
    let (one, s1) = start().await;
    let (two, s2) = start().await;
    send(one, "JBSWY3DPEHPK3PXP", "742275", 204).await;
    send(one, "JBSWY3DPEHPK3PXP", "742275", 422).await;
    send(two, "JBSWY3DPEHPK3PXP", "742275", 204).await;
    send(one, "JBSWY3DPEHPK3PXP", "bad", 422).await;
    send(one, "bad", "bad", 500).await;
    s1.abort();
    s2.abort();
    let _ = s1.await;
    let _ = s2.await;
}
