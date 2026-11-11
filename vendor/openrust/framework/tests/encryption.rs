use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::encryption::{Cipher, DecryptException, EncryptException, Encrypter};
use serde_json::Value;

const CIPHERS: [Cipher; 4] = [
    Cipher::Aes128Cbc,
    Cipher::Aes256Cbc,
    Cipher::Aes128Gcm,
    Cipher::Aes256Gcm,
];

#[test]
fn laravel_payloads_rotate_and_authenticate_for_every_cipher() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/laravel_encryption.json")).unwrap();
    for (cipher, fixture) in CIPHERS.into_iter().zip(fixtures) {
        assert_eq!(
            fixture["cipher"]
                .as_str()
                .unwrap()
                .to_uppercase()
                .parse::<Cipher>()
                .unwrap(),
            cipher
        );
        let size = cipher.key_length();
        let current = Encrypter::new(vec![b'n'; size], cipher).unwrap();
        let previous = Encrypter::new(vec![b'o'; size], cipher).unwrap();
        let old = fixture["wire"].as_str().unwrap();
        assert!(matches!(
            current.decryptString(old),
            Err(DecryptException::AuthenticationFailed)
        ));
        let rotated = Encrypter::new(vec![b'n'; size], cipher)
            .unwrap()
            .previousKeys(vec![vec![b'x'; size], vec![b'o'; size]])
            .unwrap();
        assert_eq!(rotated.decryptString(old).unwrap(), b"persisted\0\xff");
        assert_eq!(rotated.getAllKeys().len(), 3);

        let raw = b"binary\0\xff\"data";
        let serialized = rotated.encrypt(raw, true).unwrap();
        assert_eq!(rotated.decrypt(&serialized, true).unwrap(), raw);
        assert_eq!(
            rotated.decryptString(&serialized).unwrap(),
            b"s:13:\"binary\0\xff\"data\";"
        );
        let value = rotated.encryptString(raw).unwrap();
        assert_ne!(value, rotated.encryptString(raw).unwrap());
        assert_eq!(current.decryptString(&value).unwrap(), raw);
        assert!(previous.decryptString(&value).is_err());
        assert_eq!(
            rotated
                .decryptString(&rotated.encryptString(b"").unwrap())
                .unwrap(),
            b""
        );
        assert!(matches!(
            rotated.decrypt(&value, true),
            Err(DecryptException::Deserialization)
        ));
        assert!(Encrypter::appearsEncrypted(&value));
        assert!(!Encrypter::appearsEncrypted("arbitrary text"));

        let mut envelope: Value = serde_json::from_slice(&STANDARD.decode(value).unwrap()).unwrap();
        if matches!(cipher, Cipher::Aes128Gcm | Cipher::Aes256Gcm) {
            envelope["tag"] = serde_json::json!(STANDARD.encode([0; 15]));
        } else {
            envelope["mac"] = serde_json::json!("0".repeat(64));
        }
        let tampered = STANDARD.encode(serde_json::to_vec(&envelope).unwrap());
        assert!(matches!(
            rotated.decryptString(&tampered),
            Err(DecryptException::AuthenticationFailed)
        ));
        envelope["iv"] = serde_json::json!(STANDARD.encode([0; 17]));
        assert!(matches!(
            rotated.decryptString(&STANDARD.encode(serde_json::to_vec(&envelope).unwrap())),
            Err(DecryptException::InvalidPayload)
        ));
        assert!(matches!(
            rotated.previousKeys(vec![vec![0; size + 1]]),
            Err(EncryptException::InvalidKey)
        ));
    }
}

#[tokio::test]
async fn crypt_uses_each_apps_current_and_previous_keys_over_http() {
    use std::{collections::BTreeMap, path::Path, sync::Arc};

    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        config::{ConfigError, Environment},
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
        support::facades::{Crypt, Route},
    };
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    fn config(root: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
        let name = root.file_name().unwrap().to_str().unwrap();
        let (key, previous) = match name {
            "first" => (Some("o".repeat(32)), Vec::new()),
            "rotated" => (
                Some(format!("base64:{}", STANDARD.encode([b'n'; 32]))),
                vec!["o".repeat(32)],
            ),
            "missing" => (None, Vec::new()),
            _ => unreachable!(),
        };
        Ok(BTreeMap::from([(
            "app".into(),
            serde_json::json!({
                "cipher": "AES-256-CBC", "key": key, "previous_keys": previous,
            }),
        )]))
    }

    fn encrypt(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            match Crypt::encryptString("hello") {
                Ok(value) => Ok(Response::new(StatusCode::OK, Body::from(value))),
                Err(EncryptException::MissingKey) => {
                    Ok(Response::empty(StatusCode::SERVICE_UNAVAILABLE))
                }
                Err(error) => Err(HttpError::Internal {
                    source: Box::new(error),
                }),
            }
        })
    }

    fn decrypt(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        let payload = request
            .headers()
            .get("x-encrypted")
            .unwrap()
            .to_str()
            .unwrap();
        Box::pin(async move {
            match Crypt::decryptString(payload) {
                Ok(value) => Ok(Response::new(StatusCode::OK, Body::from(value))),
                Err(_) => Ok(Response::empty(StatusCode::UNPROCESSABLE_ENTITY)),
            }
        })
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        let (parts, body) = incoming.into_parts();
        let (parts, body) = kernel
            .handle(&mut Request::from_parts(parts, body))
            .await
            .into_parts();
        axum::response::Response::from_parts(parts, body)
    }

    async fn start(root: &Path) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let application = Application::configure(root.to_owned())
            .withConfig(config)
            .withRouting(Routing::default().web(|| {
                Route::get("/encrypt", encrypt)?;
                Route::get("/decrypt", decrypt)
            }))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let task = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        (address, task)
    }

    async fn get(address: std::net::SocketAddr, path: &str, payload: &str, status: u16) -> String {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nx-encrypted: {payload}\r\nConnection: close\r\n\r\n"
        );
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(
            headers.starts_with(&format!("HTTP/1.1 {status} ")),
            "{headers}"
        );
        body.to_owned()
    }

    struct Root(std::path::PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let root =
        Root(std::env::temp_dir().join(format!("openrust-crypt-{:016x}", fastrand::u64(..))));
    for name in ["first", "rotated", "missing"] {
        std::fs::create_dir_all(root.0.join(name)).unwrap();
    }
    let (first, first_task) = start(&root.0.join("first")).await;
    let (rotated, rotated_task) = start(&root.0.join("rotated")).await;
    let (missing, missing_task) = start(&root.0.join("missing")).await;
    let (old, new) = tokio::join!(
        get(first, "/encrypt", "", 200),
        get(rotated, "/encrypt", "", 200)
    );
    assert_eq!(get(rotated, "/decrypt", &old, 200).await, "hello");
    assert_eq!(get(rotated, "/decrypt", &new, 200).await, "hello");
    assert!(get(first, "/decrypt", &new, 422).await.is_empty());
    assert!(get(rotated, "/decrypt", "invalid", 422).await.is_empty());
    assert!(get(missing, "/encrypt", "", 503).await.is_empty());
    for task in [first_task, rotated_task, missing_task] {
        task.abort();
        let _ = task.await;
    }
}
