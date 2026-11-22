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
            factory.withUserAgent(input)
        } else {
            factory.pendingRequest().withUserAgent(input)
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
            *captured.lock().unwrap() = Some(Http::withUserAgent(" Facade/1.0 \t")?);
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
            .withUserAgent("good\r\nInjected: bad")
            .get(&url)
            .await,
        Err(Error::InvalidRequest)
    ));
    let replaced = factory
        .pendingRequest()
        .withHeader(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("old"),
        )
        .withUserAgent("new");
    assert_eq!(
        replaced.get(&url).await.unwrap().json::<Value>().unwrap()["user_agent"],
        reference["override"]
    );
    let appended = factory.withUserAgent("old").withHeader(
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
    let url = format!("https://{}/", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut successes = 0;
        for _ in 0..6 {
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
                    break;
                }
            }
        }
        successes
    });
    let factory = Factory::new().unwrap();
    assert!(matches!(
        factory.pendingRequest().get(&url).await,
        Err(Error::Connection)
    ));
    assert_eq!(
        factory
            .withoutVerifying()
            .get(&url)
            .await
            .unwrap()
            .body()
            .unwrap()
            .as_ref(),
        b"ok"
    );
    assert!(matches!(
        factory.pendingRequest().get(&url).await,
        Err(Error::Connection)
    ));
    assert_eq!(
        factory
            .pendingRequest()
            .withoutVerifying()
            .connectTimeout(Duration::from_secs(2))
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
            .pendingRequest()
            .connectTimeout(Duration::from_secs(2))
            .get(&url)
            .await,
        Err(Error::Connection)
    ));
    let pending = Arc::new(Mutex::new(None));
    let captured = pending.clone();
    let mut app = Application::configure(root.0.clone())
        .booted(move |_| {
            *captured.lock().unwrap() = Some(Http::withoutVerifying()?);
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
    assert_eq!(server.join().unwrap(), 3);
}
