use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    cache::rate_limiting::{Limit, LimiterResponse},
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{RateLimiter, Route},
    },
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    time::timeout,
};

struct Limits(u64);

impl ServiceProvider for Limits {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        let max = self.0;
        Box::pin(async move {
            RateLimiter::r#for("uploads", move |request| {
                let key = request.route("tenant").unwrap();
                vec![
                    Limit::perMinute(max)
                        .by(key)
                        .after(|response| response.status().is_success()),
                    Limit::perDay(max + 2).by(key).response(|_, headers| {
                        Response::empty(StatusCode::TOO_MANY_REQUESTS).withHeaders(headers)
                    }),
                ]
            })?;
            Ok(())
        })
    }
}

fn show(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let callback = RateLimiter::limiter("uploads").unwrap().unwrap();
        let LimiterResponse::Limits(limits) = callback(request) else {
            panic!("expected multiple limits");
        };
        let allowed = limits[0].afterCallback.as_ref().unwrap()(&Response::empty(StatusCode::OK));
        let denied = limits[1].responseCallback.as_ref().unwrap()(request, Default::default());
        Response::json(
            StatusCode::OK,
            json!({
                "keys": limits.iter().map(|limit| &limit.key).collect::<Vec<_>>(),
                "allowed": allowed,
                "denied": denied.status().as_u16(),
            }),
        )
    })
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/uploads/{tenant}", show)
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handleTransport(request).await
}

async fn start(max: u64) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Application::configure(std::env::temp_dir())
        .withProviders(vec![Box::new(Limits(max))])
        .withRouting(Routing::default().web(routes))
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
        .unwrap();
    });
    (address, server)
}

async fn request(address: SocketAddr) -> Value {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!(
                    "GET /uploads/demo HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut wire = String::new();
        stream.read_to_string(&mut wire).await.unwrap();
        assert!(wire.starts_with("HTTP/1.1 200 "), "{wire}");
        serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap()
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn named_limits_resolve_duplicate_keys_and_keep_two_applications_isolated() {
    let (a, first) = start(5).await;
    let (b, second) = start(10).await;
    let (a, b) = tokio::join!(request(a), request(b));
    assert_eq!(
        a,
        json!({"keys":["demo:attempts:5:decay:60","demo:attempts:7:decay:86400"],"allowed":true,"denied":429})
    );
    assert_eq!(
        b,
        json!({"keys":["demo:attempts:10:decay:60","demo:attempts:12:decay:86400"],"allowed":true,"denied":429})
    );
    first.abort();
    second.abort();
}
