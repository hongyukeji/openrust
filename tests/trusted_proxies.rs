use std::{net::SocketAddr, sync::Arc};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    contracts::http::Kernel as _,
    foundation::{
        Application,
        configuration::{ApplicationBuilder, Routing},
        http::Kernel,
    },
    http::{
        HttpError, HttpFuture, Request, RequestSecurity, Response, StatusCode, TrustedProxyHeaders,
    },
    routing::Router,
    support::facades::URL,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn proxy_info(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let current = URL::current()?;
        Response::json(
            StatusCode::OK,
            json!({
                "secure": request.secure(),
                "ips": request.ips(),
                "url": current,
                "path": request.uri().path(),
                "base": request.getBaseUrl(),
                "original_host": request.headers().get("host").and_then(|value| value.to_str().ok()),
            }),
        )
    })
}

fn request_identity(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let full = request.fullUrl()?;
        Response::json(
            StatusCode::OK,
            json!({
                "path": request.path(),
                "decoded": request.decodedPath(),
                "segments": request.segments(),
                "segment": request.segment(2),
                "get": request.isMethod("get"),
                "named": request.routeIs(&["missing", "identity.*"]),
                "host": request.host()?,
                "http_host": request.httpHost()?,
                "origin": request.schemeAndHttpHost()?,
                "root": request.root()?,
                "url": request.url()?,
                "full": full,
                "matches": request.fullUrlIs(&["https://edge.test:8443/portal/*"])?,
                "with": request.fullUrlWithQuery(&json!({"z":"changed","extra":"new value"}))?,
                "without": request.fullUrlWithoutQuery(&["z", "items.0"])?,
                "header": request.header("X-Probe")?,
                "has": request.hasHeader("x-probe"),
                "bearer": request.bearerToken()?,
            }),
        )
    })
}

async fn entry(
    State(kernel): State<Arc<Kernel>>,
    mut request: axum::extract::Request,
) -> axum::response::Response {
    request.extensions_mut().insert(RequestSecurity(false));
    kernel.handleTransport(request).await
}

async fn start(
    at: &'static [&'static str],
    headers: u8,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    start_with(|builder| {
        builder.with_middleware(move |middleware| {
            middleware.trustProxies(at, headers).unwrap();
        })
    })
    .await
}

async fn start_with(
    configure: impl FnOnce(ApplicationBuilder) -> ApplicationBuilder,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = configure(Application::configure(std::env::temp_dir()))
        .with_routing(Routing::new(|_| {
            let mut router = Router::new();
            router.get("/probe", proxy_info)?;
            router.get("/", request_identity)?;
            router
                .get("/identity/{name}", request_identity)?
                .name("identity.show");
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = entry
        .with_state(Arc::new(kernel))
        .into_make_service_with_connect_info::<SocketAddr>();
    (
        address,
        tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        }),
    )
}

#[tokio::test]
async fn request_identity_reads_named_paths_headers_and_trusted_origins() {
    let (address, server) = start(&["127.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let data = request(
        address,
        "/identity/caf%C3%A9?z=old&items[]=one&items[]=two&b=hello+world&a.b=keep",
        concat!(
            "X-Forwarded-Proto: https\r\nX-Forwarded-Host: EDGE.TEST\r\n",
            "X-Forwarded-Port: 8443\r\nX-Forwarded-Prefix: /portal/\r\n",
            "X-Probe: first\r\nX-Probe: second\r\n",
            "Authorization: Bearer ignored, bEaReR fixture-token, next\r\n",
        ),
        200,
    )
    .await;
    assert_eq!(
        data,
        json!({
            "path": "identity/caf%C3%A9", "decoded": "identity/café",
            "segments": ["identity", "café"], "segment": "café",
            "get": true, "named": true,
            "host": "edge.test", "http_host": "edge.test:8443",
            "origin": "https://edge.test:8443", "root": "https://edge.test:8443/portal",
            "url": "https://edge.test:8443/portal/identity/caf%C3%A9",
            "full": "https://edge.test:8443/portal/identity/caf%C3%A9?a.b=keep&b=hello%20world&items%5B0%5D=one&items%5B1%5D=two&z=old",
            "matches": true,
            "with": "https://edge.test:8443/portal/identity/caf%C3%A9?z=changed&items%5B0%5D=one&items%5B1%5D=two&b=hello%20world&a_b=keep&extra=new%20value",
            "without": "https://edge.test:8443/portal/identity/caf%C3%A9?items%5B1%5D=two&b=hello%20world&a_b=keep",
            "header": "first", "has": true, "bearer": "fixture-token",
        })
    );
    let root = request_with_host(address, "/?z=remove&a=keep", "[2001:db8::1]:80", "", 200).await;
    assert_eq!(root["path"], "/");
    assert_eq!(root["segments"], json!([]));
    assert!(root["segment"].is_null());
    assert_eq!(root["named"], false);
    assert_eq!(root["has"], false);
    assert!(root["bearer"].is_null());
    assert_eq!(root["http_host"], "[2001:db8::1]");
    assert_eq!(root["full"], "http://[2001:db8::1]/?a=keep&z=remove");
    assert_eq!(root["without"], "http://[2001:db8::1]/?a=keep");
    server.abort();
}

async fn request(address: SocketAddr, uri: &str, fields: &str, status: u16) -> Value {
    request_with_host(address, uri, "backend.test", fields, status).await
}

async fn request_with_host(
    address: SocketAddr,
    uri: &str,
    host: &str,
    fields: &str,
    status: u16,
) -> Value {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!(
        "GET {uri} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n{fields}\r\n",
    ).as_bytes()).await.unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.unwrap();
    let (headers, body) = reply.split_once("\r\n\r\n").unwrap();
    assert!(
        headers.starts_with(&format!("HTTP/1.1 {status} ")),
        "{reply}"
    );
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
async fn uses_trusted_proxy_metadata_without_changing_the_route_path() {
    let (address, server) =
        start(&["127.0.0.0/8", "10.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let data = request(
        address,
        "/probe?keep=1",
        concat!(
            "X-Forwarded-For: 203.0.113.7\r\nX-Forwarded-For: 10.0.0.8\r\n",
            "X-Forwarded-Proto: https\r\nX-Forwarded-Host: edge.test\r\n",
            "X-Forwarded-Port: 8443\r\nX-Forwarded-Prefix: /portal/\r\n",
        ),
        200,
    )
    .await;
    assert_eq!(
        data,
        json!({
            "secure": true, "ips": ["203.0.113.7"], "url": "https://edge.test:8443/portal/probe",
            "path": "/probe", "base": "/portal", "original_host": "backend.test",
        })
    );
    server.abort();
}

#[tokio::test]
async fn requires_a_trusted_peer_and_rejects_conflicting_forwarding_fields() {
    let (untrusted, first) = start(&["10.0.0.0/8"], TrustedProxyHeaders::DEFAULT).await;
    let data = request(
        untrusted,
        "https://backend.test/probe",
        concat!(
            "X-Forwarded-Proto: https\r\nX-Forwarded-For: 203.0.113.7\r\n",
            "X-Forwarded-Host: edge.test\r\n",
        ),
        200,
    )
    .await;
    assert_eq!(data["secure"], false);
    assert_eq!(data["ips"], json!(["127.0.0.1"]));
    assert_eq!(data["url"], "http://backend.test/probe");
    first.abort();

    let (trusted, second) = start(
        &["REMOTE_ADDR"],
        TrustedProxyHeaders::FORWARDED | TrustedProxyHeaders::DEFAULT,
    )
    .await;
    let data = request(
        trusted,
        "/probe",
        "Forwarded: for=\"[2001:db8::7]:1234\";proto=https;host=\"edge.test:444\"\r\n",
        200,
    )
    .await;
    assert_eq!(data["url"], "https://edge.test:444/probe");
    assert_eq!(data["ips"], json!(["2001:db8::7"]));
    request(
        trusted,
        "/probe",
        "Forwarded: proto=https;host=edge.test\r\nX-Forwarded-Proto: http\r\n",
        400,
    )
    .await;
    second.abort();
}

#[tokio::test]
async fn validates_incoming_and_effective_hosts_against_pcre_and_application_subdomains() {
    let (address, server) = start_with(|builder| {
        builder
            .with_config(|_, _| {
                Ok([(
                    "app".into(),
                    json!({"env":"production","url":"https://example.test"}),
                )]
                .into())
            })
            .with_middleware(|middleware| {
                middleware
                    .trustHosts(&[r"^(?=backend\.)backend\.test$"], true)
                    .unwrap();
                middleware
                    .trustProxies(&["127.0.0.1"], TrustedProxyHeaders::DEFAULT)
                    .unwrap();
            })
    })
    .await;
    let data = request(
        address,
        "/probe",
        "X-Forwarded-Host: Api.Example.Test\r\nX-Forwarded-Proto: https\r\n",
        200,
    )
    .await;
    assert_eq!(data["url"], "https://api.example.test/probe");
    request(address, "/probe", "X-Forwarded-Host: other.test\r\n", 400).await;
    request_with_host(
        address,
        "/probe",
        "other.test",
        "X-Forwarded-Host: api.example.test\r\n",
        400,
    )
    .await;
    request_with_host(address, "/probe", "deep.api.example.test:8080", "", 200).await;
    server.abort();
}

#[tokio::test]
async fn resolves_dynamic_host_patterns_and_skips_them_in_local_environment() {
    use openrust::support::facades::Config;

    let (production, first) = start_with(|builder| {
        builder
            .with_config(|_, _| {
                Ok([
                    (
                        "app".into(),
                        json!({"env":"production","url":"https://example.test"}),
                    ),
                    ("probe".into(), json!({"host":r"^backend\.test$"})),
                ]
                .into())
            })
            .with_middleware(|middleware| {
                middleware.trustHostsWith(
                    || async {
                        let value = Config::get("probe.host")?.expect("host pattern");
                        Ok(vec![value.as_str().expect("string pattern").to_owned()])
                    },
                    false,
                );
            })
    })
    .await;
    request(production, "/probe", "", 200).await;
    request_with_host(production, "/probe", "example.test", "", 400).await;
    first.abort();

    let (local, second) = start_with(|builder| {
        builder
            .with_config(|_, _| Ok([("app".into(), json!({"env":"local"}))].into()))
            .with_middleware(|middleware| {
                middleware.trustHostsWith(|| async { panic!("disabled callback ran") }, false);
            })
    })
    .await;
    request_with_host(local, "/probe", "other.test", "", 200).await;
    second.abort();
}

#[tokio::test]
async fn honors_proxy_deployment_presets_and_explicit_configuration_precedence() {
    fn config(
        _: &std::path::Path,
        _: &openrust::config::Environment,
    ) -> Result<std::collections::BTreeMap<String, Value>, openrust::config::ConfigError> {
        Ok([(
            "trustedproxy".into(),
            json!({
                "proxies":"127.0.0.0/8, 10.0.0.0/8",
                "headers":"HEADER_X_FORWARDED_AWS_ELB",
            }),
        )]
        .into())
    }
    let fields = concat!(
        "X-Forwarded-For: 203.0.113.7, 10.0.0.8\r\n",
        "X-Forwarded-Proto: https\r\nX-Forwarded-Host: ignored.test\r\nX-Forwarded-Port: 8443\r\n",
    );
    let (configured, first) = start_with(|builder| builder.with_config(config)).await;
    let data = request(configured, "/probe", fields, 200).await;
    assert_eq!(data["url"], "https://backend.test:8443/probe");
    assert_eq!(data["ips"], json!(["203.0.113.7"]));
    first.abort();

    let (disabled, second) = start_with(|builder| {
        builder.with_config(config).with_middleware(|middleware| {
            middleware
                .trustProxies(&[], TrustedProxyHeaders::DEFAULT)
                .unwrap();
        })
    })
    .await;
    let data = request(disabled, "/probe", fields, 200).await;
    assert_eq!(data["url"], "http://backend.test/probe");
    assert_eq!(data["ips"], json!(["127.0.0.1"]));
    second.abort();
}
