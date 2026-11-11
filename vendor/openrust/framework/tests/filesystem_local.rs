use std::{io, path::PathBuf, sync::Arc, task::Poll};

use openrust::{
    config::Repository,
    filesystem::{FilesystemError, FilesystemManager, LocalDiskConfig, Visibility, WriteOptions},
};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _, ReadBuf};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("openrust-fs-{:032x}", fastrand::u128(..)));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn manager(&self, throw: bool) -> FilesystemManager {
        let repository = Repository::new(std::collections::BTreeMap::from([(
            "filesystems".into(),
            json!({
                "default": "local",
                "disks": {
                    "local": {"driver":"local", "root":"private", "throw":throw},
                    "public": {"driver":"local", "root":"public", "visibility":"public", "throw":throw},
                    "s3": {"driver":"s3"},
                }
            }),
        )]));
        FilesystemManager::new(Arc::new(repository), self.0.clone())
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn named_and_on_demand_disks_isolate_files_and_honor_error_policy() {
    let root = Sandbox::new();
    let other = Sandbox::new();
    let manager = root.manager(false);
    let private = manager.disk(None).unwrap();
    let public = manager.disk(Some("public")).unwrap();
    assert!(Arc::ptr_eq(&private, &manager.disk(Some("local")).unwrap()));
    assert!(matches!(
        manager.disk(Some("unknown")),
        Err(FilesystemError::UnknownDisk { .. })
    ));
    assert!(matches!(
        manager.disk(Some("s3")),
        Err(FilesystemError::InvalidConfiguration)
    ));
    assert!(private.put("logs/data", b"first").await.unwrap());
    assert!(public.put("logs/data", b"second").await.unwrap());
    assert_eq!(private.get("logs/data").await.unwrap().unwrap(), b"first");
    assert_eq!(public.get("logs/data").await.unwrap().unwrap(), b"second");
    assert!(
        other
            .manager(false)
            .disk(None)
            .unwrap()
            .missing("logs/data")
            .await
            .unwrap()
    );
    assert!(private.directoryExists("logs").await.unwrap());
    assert!(!private.fileExists("logs").await.unwrap());
    assert!(private.delete("logs/data").await.unwrap());
    assert!(private.delete("logs/data").await.unwrap());
    assert!(private.get("logs/data").await.unwrap().is_none());
    assert!(matches!(
        root.manager(true).disk(None).unwrap().get("absent").await,
        Err(FilesystemError::NotFound)
    ));
    for path in [
        "/absolute",
        "logs/../outside",
        "logs\\outside",
        "",
        "logs/.storage-write-test",
    ] {
        assert!(
            matches!(
                private.put(path, b"invalid").await,
                Err(FilesystemError::InvalidPath)
            ),
            "{path}"
        );
    }

    manager.forgetDisk("local");
    assert!(!Arc::ptr_eq(&private, &manager.disk(None).unwrap()));
    let on_demand = manager
        .build(LocalDiskConfig {
            root: PathBuf::from("other"),
            visibility: Visibility::Private,
            directory_visibility: None,
            throw: true,
            report: false,
        })
        .unwrap();
    assert!(on_demand.put("fresh", b"on demand").await.unwrap());
    assert_eq!(
        std::fs::read(root.0.join("other/fresh")).unwrap(),
        b"on demand"
    );
}

struct Interrupted(bool);

impl AsyncRead for Interrupted {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.0 {
            return Poll::Ready(Err(io::Error::other("reader failed")));
        }
        buffer.put_slice(b"partial");
        self.0 = true;
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn stream_failures_and_cancellation_leave_existing_file_intact() {
    let root = Sandbox::new();
    let disk = root.manager(true).disk(None).unwrap();
    disk.put("file", b"before").await.unwrap();
    assert!(matches!(
        disk.writeStream("file", &mut Interrupted(false)).await,
        Err(FilesystemError::Backend { .. })
    ));
    assert_eq!(disk.get("file").await.unwrap().unwrap(), b"before");

    let (mut writer, mut reader) = tokio::io::duplex(16);
    let worker = {
        let disk = Arc::clone(&disk);
        tokio::spawn(async move { disk.writeStream("file", &mut reader).await })
    };
    writer.write_all(b"uncommitted").await.unwrap();
    let start = std::time::Instant::now();
    while std::fs::read_dir(root.0.join("private")).unwrap().count() == 1 {
        assert!(start.elapsed().as_secs() < 5);
        tokio::task::yield_now().await;
    }
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert_eq!(disk.get("file").await.unwrap().unwrap(), b"before");
    assert_eq!(
        std::fs::read_dir(root.0.join("private")).unwrap().count(),
        1
    );

    let mut data = &vec![b'x'; 512 * 1024][..];
    assert!(disk.writeStream("file", &mut data).await.unwrap());
    let mut stream = disk.readStream("file").await.unwrap().unwrap();
    let mut first = [0; 17];
    stream.read_exact(&mut first).await.unwrap();
    assert_eq!(first, [b'x'; 17]);
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).await.unwrap();
    assert_eq!(first.len() + rest.len(), 512 * 1024);
}

#[cfg(unix)]
#[tokio::test]
async fn visibility_and_links_are_confined_to_the_disk() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let root = Sandbox::new();
    let disk = root.manager(true).disk(Some("public")).unwrap();
    disk.put("folder/plain", b"public").await.unwrap();
    let private = WriteOptions {
        visibility: Some(Visibility::Private),
    };
    disk.putWithOptions("folder/hidden", b"private", private)
        .await
        .unwrap();
    assert_eq!(
        disk.getVisibility("folder/plain").await.unwrap(),
        Visibility::Public
    );
    assert_eq!(
        disk.getVisibility("folder/hidden").await.unwrap(),
        Visibility::Private
    );
    assert_eq!(
        std::fs::metadata(root.0.join("public/folder"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    disk.setVisibility("folder/hidden", Visibility::Public)
        .await
        .unwrap();
    assert_eq!(
        std::fs::metadata(root.0.join("public/folder/hidden"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );

    std::fs::write(root.0.join("outside"), b"outside").unwrap();
    symlink(&root.0, root.0.join("public/escape")).unwrap();
    symlink(root.0.join("outside"), root.0.join("public/soft")).unwrap();
    std::fs::hard_link(root.0.join("outside"), root.0.join("public/hard")).unwrap();
    for path in ["escape/outside", "soft", "hard"] {
        assert!(disk.get(path).await.is_err(), "{path}");
        assert!(
            disk.setVisibility(path, Visibility::Private).await.is_err(),
            "{path}"
        );
    }
    assert!(disk.put("escape/outside", b"replace").await.is_err());
    disk.put("soft", b"replace").await.unwrap();
    disk.put("hard", b"replace").await.unwrap();
    assert_eq!(std::fs::read(root.0.join("outside")).unwrap(), b"outside");
}

#[cfg(unix)]
#[tokio::test]
async fn application_scope_and_public_mount_work_over_http() {
    use std::{collections::BTreeMap, path::Path};

    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        config::{ConfigError, Environment, Value},
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{HeaderMap, HttpError, HttpFuture, Method, Request, Response, StatusCode},
        support::facades::{Route, Storage},
    };

    fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
        Ok(BTreeMap::from([(
            "filesystems".into(),
            json!({"default": "local", "disks": {
                "local": {"driver":"local", "root":"private", "throw":true},
                "public": {"driver":"local", "root":"public-files", "visibility":"public",
                    "url":"/storage", "throw":true},
            }}),
        )]))
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        let (parts, body) = incoming.into_parts();
        let mut request = Request::from_parts(parts, body);
        let (parts, body) = kernel.handle(&mut request).await.into_parts();
        axum::response::Response::from_parts(parts, body)
    }

    async fn send(address: std::net::SocketAddr, request: &str, body: &[u8]) -> Vec<u8> {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket
            .write_all(
                format!("{request}\r\nHost: {address}\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        socket.write_all(body).await.unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.unwrap();
        response
    }

    async fn get(address: std::net::SocketAddr, path: &str) -> String {
        String::from_utf8(send(address, &format!("GET {path} HTTP/1.1"), b"").await).unwrap()
    }

    fn write(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            assert!(Storage::put("notes/example.txt", b"Hello").await.unwrap());
            assert_eq!(
                Storage::get("notes/example.txt").await.unwrap().as_deref(),
                Some(&b"Hello"[..])
            );
            let disk = Storage::disk(Some("public")).unwrap();
            assert!(
                disk.put("nested/public.txt", b"public content")
                    .await
                    .unwrap()
            );
            assert!(
                disk.putWithOptions(
                    "nested/private.txt",
                    b"restricted content",
                    WriteOptions {
                        visibility: Some(Visibility::Private)
                    },
                )
                .await
                .unwrap()
            );
            disk.put("nested/large.txt", vec![b'x'; 256 * 1024]).await?;
            Ok(Response::empty(StatusCode::NO_CONTENT))
        })
    }

    fn upload(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let file = request
                .file("attachment")
                .await?
                .ok_or(HttpError::InvalidRequest)?;
            let name = file.hashName()?;
            assert_eq!(file.clone().hashName()?, name);
            assert_eq!(file.extension(), Some("png"));
            let path = file
                .storePublicly("incoming", Some("public"))
                .await?
                .unwrap();
            assert_eq!(path, format!("incoming/{name}"));
            file.storeAs("incoming", "private.png", None)
                .await?
                .unwrap();
            let disk = Storage::disk(Some("public"))?;
            assert!(matches!(
                disk.putFileAs("incoming", &file, "../escape.png", WriteOptions::default())
                    .await,
                Err(FilesystemError::InvalidPath)
            ));
            Response::json(StatusCode::CREATED, json!({"url": disk.url(&path)?}))
        })
    }

    fn download(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async move {
            let disk = Storage::disk(Some("public"))?;
            let (path, name) = match request.route("file") {
                Some("public") => ("nested/public.txt", "привет%.txt"),
                Some("large") => ("nested/large.txt", "large.txt"),
                Some("invalid") => ("nested/public.txt", "invalid\r\nname.txt"),
                _ => ("missing.txt", "missing.txt"),
            };
            Ok(disk.download(path, Some(name), HeaderMap::new()).await?)
        })
    }

    async fn start(root: &Sandbox) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let application = Application::configure(root.0.clone())
            .withConfig(config)
            .withRouting(Routing::default().web(|| {
                Route::staticDisk("public")?;
                Route::get("/write", write)?;
                Route::r#match(&[Method::POST], "/upload", upload)?;
                Route::get("/download/{file}", download)
            }))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let server = tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        });
        (address, server)
    }

    let first = Sandbox::new();
    let second = Sandbox::new();
    let (address, server) = start(&first).await;
    let (other, second_server) = start(&second).await;
    assert!(get(address, "/write").await.starts_with("HTTP/1.1 204"));
    let response = get(address, "/storage/nested/public.txt").await;
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.ends_with("public content"));
    assert!(
        get(other, "/storage/nested/public.txt")
            .await
            .starts_with("HTTP/1.1 404")
    );
    assert!(
        get(address, "/storage/nested/private.txt")
            .await
            .starts_with("HTTP/1.1 404")
    );
    assert!(
        get(address, "/storage/notes/example.txt")
            .await
            .starts_with("HTTP/1.1 404")
    );
    assert_eq!(
        std::fs::read(first.0.join("private/notes/example.txt")).unwrap(),
        b"Hello"
    );

    let downloaded = get(address, "/download/public").await;
    let (headers, body) = downloaded.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(headers.contains("\r\ncontent-length: 14\r\n"));
    assert!(headers.contains("\r\ncontent-type: text/plain\r\n"));
    assert!(headers.contains("content-disposition: attachment; filename=privet.txt; filename*=utf-8''%D0%BF%D1%80%D0%B8%D0%B2%D0%B5%D1%82%25.txt"));
    assert_eq!(body, "public content");
    let large = get(address, "/download/large").await;
    let (headers, body) = large.split_once("\r\n\r\n").unwrap();
    assert!(headers.contains("\r\ncontent-length: 262144\r\n"));
    assert_eq!(body.len(), 256 * 1024);
    assert!(body.bytes().all(|byte| byte == b'x'));
    assert!(
        get(address, "/download/invalid")
            .await
            .starts_with("HTTP/1.1 400")
    );
    assert!(
        get(address, "/download/missing")
            .await
            .starts_with("HTTP/1.1 404")
    );
    assert!(
        get(address, "/download/private")
            .await
            .starts_with("HTTP/1.1 404")
    );

    let png = b"\x89PNG\r\n\x1a\nfixture";
    let mut body = b"--upload\r\nContent-Disposition: form-data; name=\"attachment\"; filename=\"fake.txt\"\r\nContent-Type: text/plain\r\n\r\n".to_vec();
    body.extend_from_slice(png);
    body.extend_from_slice(b"\r\n--upload--\r\n");
    let request = format!(
        "POST /upload HTTP/1.1\r\nContent-Type: multipart/form-data; boundary=upload\r\nContent-Length: {}",
        body.len()
    );
    let uploaded = String::from_utf8(send(address, &request, &body).await).unwrap();
    assert!(uploaded.starts_with("HTTP/1.1 201"), "{uploaded}");
    let payload: serde_json::Value =
        serde_json::from_str(uploaded.split_once("\r\n\r\n").unwrap().1).unwrap();
    let url = payload["url"].as_str().unwrap();
    let name = url
        .strip_prefix("/storage/incoming/")
        .unwrap()
        .strip_suffix(".png")
        .unwrap();
    assert_eq!(name.len(), 40);
    assert!(name.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    let fetched = send(address, &format!("GET {url} HTTP/1.1"), b"").await;
    assert!(fetched.starts_with(b"HTTP/1.1 200"));
    assert!(fetched.ends_with(png));
    assert!(get(other, url).await.starts_with("HTTP/1.1 404"));
    assert!(
        get(address, "/storage/incoming/private.png")
            .await
            .starts_with("HTTP/1.1 404")
    );
    assert_eq!(
        std::fs::read(first.0.join("private/incoming/private.png")).unwrap(),
        png
    );

    server.abort();
    second_server.abort();
    let _ = server.await;
    let _ = second_server.await;
}
