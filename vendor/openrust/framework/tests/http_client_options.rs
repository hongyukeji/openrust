use openrust::{
    foundation::Application,
    http::client::{Error, Factory, Http},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read as _, Write as _},
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("http-options-{:016x}", fastrand::u64(..)));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn user_agent_matches_fixed_php_trimming_and_real_wire_headers() {
    let reference: Value =
        serde_json::from_str(include_str!("reference/http-client-options.json")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().route("/",axum::routing::get(|headers: http::HeaderMap|async move {
            ([("content-type", "application/json")], json!({"user_agent":headers.get(http::header::USER_AGENT).map(|value|String::from_utf8_lossy(value.as_bytes()).into_owned()), "user_agents":headers.get_all(http::header::USER_AGENT).iter().map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned()).collect::<Vec<_>>()}).to_string())
        }))).await.unwrap();
    });
    let factory = Factory::new().unwrap();
    for (index, row) in reference["trim"].as_array().unwrap().iter().enumerate() {
        let input = row["input"].as_str().unwrap();
        let request = if index % 2 == 0 {
            factory.with_user_agent(input)
        } else {
            factory.pending_request().with_user_agent(input)
        };
        assert_eq!(
            request.get(&url).await.unwrap().json::<Value>().unwrap()["user_agent"],
            row["expected"]
        );
    }
    let pending = Arc::new(Mutex::new(None));
    let captured = pending.clone();
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .booted(move |_| {
            *captured.lock().unwrap() = Some(Http::with_user_agent(" Facade/1.0 \t")?);
            Ok(())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let request = pending.lock().unwrap().take().unwrap();
    assert_eq!(
        request.get(&url).await.unwrap().json::<Value>().unwrap()["user_agent"],
        "Facade/1.0"
    );
    assert!(matches!(
        factory
            .with_user_agent("good\r\nInjected: bad")
            .get(&url)
            .await,
        Err(Error::InvalidRequest)
    ));
    let replaced = factory
        .pending_request()
        .with_header(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("old"),
        )
        .with_user_agent("new");
    assert_eq!(
        replaced.get(&url).await.unwrap().json::<Value>().unwrap()["user_agent"],
        reference["override"]
    );
    let appended = factory.with_user_agent("old").with_header(
        http::header::USER_AGENT,
        http::HeaderValue::from_static("new"),
    );
    assert_eq!(
        appended.get(&url).await.unwrap().json::<Value>().unwrap()["user_agents"],
        reference["append"]
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn certificate_verification_is_disabled_only_for_the_selected_request() {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject as _};
    let root = Root::new();
    let certificate = root.0.join("certificate.pem");
    let key = root.0.join("key.pem");
    let output = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=untrusted.example",
            "-addext",
            "subjectAltName=DNS:untrusted.example",
            "-keyout",
        ])
        .arg(&key)
        .arg("-out")
        .arg(&certificate)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let certificates = CertificateDer::pem_file_iter(&certificate)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_file(&key).unwrap();
    let configuration = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certificates, key)
    .unwrap();
    let configuration = Arc::new(configuration);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("https://{address}/");
    let pinned_url = format!("https://untrusted.example:{}/", address.port());
    let server = std::thread::spawn(move || {
        let mut successes = 0;
        let mut verified_sni = 0;
        for _ in 0..8 {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            tcp.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
            let connection = rustls::ServerConnection::new(configuration.clone()).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, tcp);
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while request.len() < 8192 {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(length) => request.extend_from_slice(&chunk[..length]),
                }
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                        )
                        .unwrap();
                    stream.flush().unwrap();
                    successes += 1;
                    verified_sni +=
                        usize::from(stream.conn.server_name() == Some("untrusted.example"));
                    break;
                }
            }
        }
        (successes, verified_sni)
    });
    let factory = Factory::new().unwrap();
    assert!(matches!(
        factory.pending_request().get(&url).await,
        Err(Error::Connection)
    ));
    assert_eq!(
        factory
            .without_verifying()
            .get(&url)
            .await
            .unwrap()
            .body()
            .unwrap()
            .as_ref(),
        b"ok"
    );
    assert!(matches!(
        factory.pending_request().get(&url).await,
        Err(Error::Connection)
    ));
    assert_eq!(
        factory
            .pending_request()
            .without_verifying()
            .connect_timeout(Duration::from_secs(2))
            .get(&url)
            .await
            .unwrap()
            .body()
            .unwrap()
            .as_ref(),
        b"ok"
    );
    assert!(matches!(
        factory
            .pending_request()
            .connect_timeout(Duration::from_secs(2))
            .get(&url)
            .await,
        Err(Error::Connection)
    ));
    let pending = Arc::new(Mutex::new(None));
    let captured = pending.clone();
    let mut app = Application::configure(root.0.clone())
        .booted(move |_| {
            *captured.lock().unwrap() = Some(Http::without_verifying()?);
            Ok(())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let request = pending.lock().unwrap().take().unwrap();
    assert_eq!(
        request.get(&url).await.unwrap().body().unwrap().as_ref(),
        b"ok"
    );
    let pinned = || {
        factory
            .pending_request()
            .pin_destination(&pinned_url, vec![address])
            .unwrap()
    };
    assert!(matches!(
        pinned().get(&pinned_url).await,
        Err(Error::Connection)
    ));
    assert!(matches!(
        pinned().with_tls_ca(Vec::new()).get(&pinned_url).await,
        Err(Error::Configuration)
    ));
    assert_eq!(
        pinned()
            .without_verifying()
            .get(&pinned_url)
            .await
            .unwrap()
            .body()
            .unwrap()
            .as_ref(),
        b"ok"
    );
    assert_eq!(server.join().unwrap(), (4, 1));
}

#[tokio::test]
async fn pinned_destination_uses_only_checked_addresses_and_preserves_host() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let blocked = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other = format!("http://{}/", blocked.local_addr().unwrap());
    let redirect = other.clone();
    let server = tokio::spawn(async move {
        let routes = axum::Router::new()
            .route(
                "/",
                axum::routing::get(|headers: http::HeaderMap| async move {
                    headers[http::header::HOST].to_str().unwrap().to_owned()
                }),
            )
            .route(
                "/redirect",
                axum::routing::get(move || async move {
                    axum::response::Redirect::temporary(&redirect)
                }),
            );
        axum::serve(listener, routes).await.unwrap();
    });
    let url = format!("http://checked-address.invalid:{}/", address.port());
    let factory = Factory::new().unwrap();
    let request = || {
        factory
            .pending_request()
            .pin_destination(&url, vec![address])
            .unwrap()
    };
    let response = request().get(&url).await.unwrap();
    assert_eq!(
        response.body().unwrap().as_ref(),
        format!("checked-address.invalid:{}", address.port()).as_bytes()
    );
    let response = request()
        .max_redirects(10)
        .get(&format!("{url}redirect"))
        .await
        .unwrap();
    assert_eq!(response.status(), http::StatusCode::TEMPORARY_REDIRECT);

    assert!(matches!(
        request().max_response_bytes(1).get(&url).await,
        Err(Error::BodyTooLarge)
    ));
    assert!(matches!(
        request().get(&other).await,
        Err(Error::DestinationMismatch)
    ));
    assert!(matches!(
        request()
            .with_request_middleware({
                let other = other.clone();
                move |request| {
                    *request.url_mut() = other.parse().unwrap();
                    Ok(())
                }
            })
            .get(&url)
            .await,
        Err(Error::DestinationMismatch)
    ));
    assert!(matches!(
        request()
            .with_header(http::header::HOST, "other-target.invalid".parse().unwrap())
            .get(&url)
            .await,
        Err(Error::DestinationMismatch)
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), blocked.accept())
            .await
            .is_err()
    );
    server.abort();
}
