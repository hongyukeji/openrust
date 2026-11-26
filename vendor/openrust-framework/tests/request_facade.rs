use std::{collections::BTreeMap, net::SocketAddr};

use axum::extract::ConnectInfo;
use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode, TrustedProxyHeaders},
    support::facades::{Request as RequestFacade, Route},
};
use serde_json::{Value, json};

fn inspect(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        tokio::task::yield_now().await;
        let ip = RequestFacade::ip()?;
        let ips = RequestFacade::ips()?;
        assert_eq!(ip, request.ip());
        assert_eq!(ips, request.ips());
        assert_eq!(ip, RequestFacade::get_client_ip()?);
        assert_eq!(ips, RequestFacade::get_client_ips()?);
        assert_eq!(ip, request.get_client_ip());
        assert_eq!(ips, request.get_client_ips());
        assert!(tokio::spawn(async { RequestFacade::ip().is_err() }).await?);
        Response::json(StatusCode::OK, json!({"ip":ip,"ips":ips}))
    })
}

fn slow(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        assert!(RequestFacade::ip()?.is_some());
        std::future::pending().await
    })
}

async fn kernel(trusted: &'static [&'static str], headers: u8) -> (tempfile::TempDir, Kernel) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().to_path_buf())
        .with_config(|_, _| {
            Ok(BTreeMap::from([
                (
                    "app".into(),
                    json!({"env":"testing","key":"a".repeat(32),"locale":"en"}),
                ),
                ("session".into(), json!({"driver":"array"})),
            ]))
        })
        .with_middleware(move |middleware| {
            middleware.trust_proxies(trusted, headers).unwrap();
        })
        .with_exceptions(|exceptions| {
            exceptions.render::<HttpError>(|_, request| {
                let ip = RequestFacade::ip().expect("异常渲染仍在请求作用域内");
                assert_eq!(ip, request.ip());
                Response::json(StatusCode::BAD_REQUEST, json!({"error_ip":ip})).ok()
            });
        })
        .with_routing(Routing::default().web(|| {
            Route::get("/", inspect)?;
            Route::get("/slow", slow)?;
            Ok(())
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    (root, kernel)
}

fn incoming(peer: Option<&str>, headers: &[(&str, &str)], uri: &str) -> Request {
    let mut builder = ::http::Request::builder()
        .uri(uri)
        .header("host", "example.test");
    if let Some(peer) = peer {
        builder = builder.extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    Request::from(builder.body(Body::empty()).unwrap())
}

async fn call(kernel: &Kernel, peer: Option<&str>, headers: &[(&str, &str)]) -> Value {
    let mut request = incoming(peer, headers, "/");
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_parts().1, 4096)
        .await
        .unwrap();
    assert!(RequestFacade::ip().is_err());
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn request_facade_does_not_invent_an_address_outside_http() {
    assert!(RequestFacade::ip().is_err());
    assert!(RequestFacade::ips().is_err());
    assert!(RequestFacade::get_client_ip().is_err());
    assert!(RequestFacade::get_client_ips().is_err());
}

#[tokio::test]
async fn direct_ipv4_ipv6_and_mapped_addresses_match_the_request() {
    let (_root, kernel) = kernel(&[], TrustedProxyHeaders::DEFAULT).await;
    for (peer, ip) in [
        ("198.51.100.2:1200", "198.51.100.2"),
        ("[2001:db8::2]:1200", "2001:db8::2"),
        ("[::ffff:192.0.2.3]:1200", "192.0.2.3"),
    ] {
        assert_eq!(
            call(&kernel, Some(peer), &[]).await,
            json!({"ip":ip,"ips":[ip]})
        );
    }
}

#[tokio::test]
async fn missing_transport_address_is_none_inside_http_and_an_error_afterwards() {
    let (_root, kernel) = kernel(&[], TrustedProxyHeaders::DEFAULT).await;
    assert_eq!(call(&kernel, None, &[]).await, json!({"ip":null,"ips":[]}));
}

#[tokio::test]
async fn untrusted_peers_cannot_supply_the_facade_ip_from_headers() {
    let (_root, kernel) = kernel(&["10.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let value = call(
        &kernel,
        Some("198.51.100.2:1200"),
        &[("x-forwarded-for", "203.0.113.7")],
    )
    .await;
    assert_eq!(value, json!({"ip":"198.51.100.2","ips":["198.51.100.2"]}));
}

#[tokio::test]
async fn trusted_proxy_chains_share_the_existing_address_order() {
    let (_root, kernel) = kernel(&["10.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let value = call(
        &kernel,
        Some("10.0.0.1:1200"),
        &[("x-forwarded-for", "203.0.113.7, 198.51.100.9, 10.0.0.2")],
    )
    .await;
    assert_eq!(
        value,
        json!({"ip":"198.51.100.9","ips":["198.51.100.9","203.0.113.7"]})
    );
}

#[tokio::test]
async fn forwarded_ipv6_uses_the_same_trusted_proxy_policy() {
    let (_root, kernel) = kernel(&["10.0.0.0/8"], TrustedProxyHeaders::FORWARDED).await;
    let value = call(
        &kernel,
        Some("10.0.0.1:1200"),
        &[("forwarded", "for=\"[2001:db8::7]\"")],
    )
    .await;
    assert_eq!(value, json!({"ip":"2001:db8::7","ips":["2001:db8::7"]}));
}

#[tokio::test]
async fn concurrent_requests_and_applications_keep_separate_addresses() {
    let (_one, one) = kernel(&[], TrustedProxyHeaders::DEFAULT).await;
    let (_two, two) = kernel(&["10.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let requests = (1..=24).map(|index| {
        let one = &one;
        let two = &two;
        async move {
            let peer = format!("198.51.100.{index}:1200");
            let client = format!("203.0.113.{index}");
            let forwarded = [("x-forwarded-for", client.as_str())];
            let (direct, proxied) = tokio::join!(
                call(one, Some(&peer), &[]),
                call(two, Some("10.0.0.1:1200"), &forwarded)
            );
            assert_eq!(direct["ip"], format!("198.51.100.{index}"));
            assert_eq!(proxied["ip"], client);
        }
    });
    futures_util::future::join_all(requests).await;
}

#[tokio::test]
async fn cancellation_drops_the_request_address_scope() {
    let (_root, kernel) = kernel(&[], TrustedProxyHeaders::DEFAULT).await;
    let mut request = incoming(Some("198.51.100.8:1200"), &[], "/slow");
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            kernel.handle(&mut request)
        )
        .await
        .is_err()
    );
    assert!(RequestFacade::ip().is_err());
    assert_eq!(
        call(&kernel, Some("198.51.100.9:1200"), &[]).await["ip"],
        "198.51.100.9"
    );
}

#[tokio::test]
async fn conflicting_forwarded_headers_return_400_without_retaining_an_address() {
    let (_root, kernel) = kernel(
        &["10.0.0.0/8"],
        TrustedProxyHeaders::DEFAULT | TrustedProxyHeaders::FORWARDED,
    )
    .await;
    let mut request = incoming(
        Some("10.0.0.1:1200"),
        &[
            ("x-forwarded-for", "203.0.113.7"),
            ("forwarded", "for=198.51.100.9"),
        ],
        "/",
    );
    assert_eq!(
        kernel.handle(&mut request).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert!(RequestFacade::ip().is_err());
}

async fn transport(
    axum::extract::State(kernel): axum::extract::State<std::sync::Arc<Kernel>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    kernel.handle_transport(request).await
}

#[tokio::test]
async fn real_tcp_requests_use_the_transport_peer_and_configured_proxy_policy() {
    for (trusted, expected) in [(&[][..], "127.0.0.1"), (&["127.0.0.1"][..], "203.0.113.23")] {
        let (_root, kernel) = kernel(trusted, TrustedProxyHeaders::DEFAULT).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new()
            .fallback(transport)
            .with_state(std::sync::Arc::new(kernel));
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client
                .get(format!("http://{address}/"))
                .header("x-forwarded-for", "203.0.113.23")
                .send(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value = response.json().await.unwrap();
        assert_eq!(value, json!({"ip":expected,"ips":[expected]}));
        server.abort();
        let _ = server.await;
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }
}

#[tokio::test]
async fn early_path_validation_errors_keep_the_request_facade_available() {
    let (_root, kernel) = kernel(&[], TrustedProxyHeaders::DEFAULT).await;
    let mut request = incoming(Some("198.51.100.17:1200"), &[], "/%ZZ");
    let response = kernel.handle(&mut request).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_parts().1, 4096)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["error_ip"],
        "198.51.100.17"
    );
    assert!(RequestFacade::ip().is_err());
}
