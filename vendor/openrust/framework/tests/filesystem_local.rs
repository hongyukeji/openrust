use std::{io, path::PathBuf, sync::Arc, task::Poll};

#[cfg(unix)]
use openrust::filesystem::WriteOptions;
use openrust::{
    config::Repository,
    filesystem::{FilesystemError, FilesystemManager, LocalDiskConfig, Visibility},
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
    assert_eq!(manager.getDefaultCloudDriver().unwrap(), "s3");
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
        http::{HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
        support::facades::{Route, Storage},
    };

    fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
        Ok(BTreeMap::from([(
            "filesystems".into(),
            json!({"default": "local", "disks": {
                "local": {"driver":"local", "root":"private", "throw":true},
                "public": {"driver":"local", "root":"public-files", "visibility":"public",
                    "url":"/storage", "throw":true,
                    "permissions":{
                        "file":{"public":0o664,"private":0o604},
                        "dir":{"public":0o775,"private":0o701},
                    }},
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
            assert!(Storage::makeDirectory("notes/empty").await.unwrap());
            assert!(Storage::put("notes/example.txt", b"Hello").await.unwrap());
            assert_eq!(Storage::size("notes/example.txt").await.unwrap(), 5);
            assert_eq!(
                Storage::mime_type("notes/example.txt")
                    .await
                    .unwrap()
                    .as_deref(),
                Some("text/plain")
            );
            assert!(Storage::lastModified("notes/example.txt").await.is_ok());
            assert_eq!(
                Storage::checksum("notes/example.txt")
                    .await
                    .unwrap()
                    .as_deref(),
                Some("8b1a9953c4611296a827abf8c47804d7")
            );

            assert_eq!(
                Storage::files(Some("notes")).await.unwrap(),
                ["notes/example.txt"]
            );
            assert_eq!(
                Storage::all_files(None).await.unwrap(),
                ["notes/example.txt"]
            );
            assert_eq!(
                Storage::directories(Some("notes")).await.unwrap(),
                ["notes/empty"]
            );
            assert!(
                Storage::allDirectories(None)
                    .await
                    .unwrap()
                    .contains(&"notes/empty".to_owned())
            );
            assert!(Storage::deleteDirectory("notes/empty").await.unwrap());

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
            assert_eq!(
                disk.getVisibility("nested/private.txt").await?,
                Visibility::Private
            );
            disk.put("private-folder/readable.txt", b"private directory")
                .await?;
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
            let name = file.hash_name()?;
            assert_eq!(file.clone().hash_name()?, name);
            assert_eq!(file.extension(), Some("png"));
            let path = file
                .store_publicly("incoming", Some("public"))
                .await?
                .unwrap();
            assert_eq!(path, format!("incoming/{name}"));
            file.store_as("incoming", "private.png", None)
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
            .with_config(config)
            .with_routing(Routing::default().web(|| {
                Route::static_disk("public")?;
                Route::get("/write", write)?;
                Route::post("/upload", upload)?;
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
    {
        use std::os::unix::fs::PermissionsExt as _;

        for (path, mode) in [
            ("public-files", 0o775),
            ("public-files/nested", 0o775),
            ("public-files/nested/public.txt", 0o664),
            ("public-files/nested/private.txt", 0o604),
        ] {
            assert_eq!(
                std::fs::metadata(first.0.join(path))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                mode
            );
        }
        std::fs::set_permissions(
            first.0.join("public-files/private-folder"),
            std::fs::Permissions::from_mode(0o701),
        )
        .unwrap();
    }
    assert!(
        get(address, "/storage/private-folder/readable.txt")
            .await
            .starts_with("HTTP/1.1 404")
    );

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

#[tokio::test]
async fn local_directories_list_paths_create_parents_and_delete_recursively() {
    let root = Sandbox::new();
    let disk = root.manager(true).disk(None).unwrap();
    assert!(disk.files(None).await.unwrap().is_empty());
    assert!(
        disk.allDirectories(Some("absent"))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(disk.makeDirectory("records/january/empty").await.unwrap());
    assert!(disk.makeDirectory("records/january/empty").await.unwrap());
    assert!(disk.put("records/z.txt", b"last").await.unwrap());
    assert!(disk.put("records/january/a.txt", b"first").await.unwrap());
    assert!(disk.put("outside.txt", b"other").await.unwrap());
    assert_eq!(disk.files(Some("")).await.unwrap(), ["outside.txt"]);
    assert_eq!(
        disk.files(Some("records")).await.unwrap(),
        ["records/z.txt"]
    );
    assert_eq!(
        disk.all_files(Some("records")).await.unwrap(),
        ["records/january/a.txt", "records/z.txt"]
    );
    assert_eq!(
        disk.directories(Some("records")).await.unwrap(),
        ["records/january"]
    );
    assert_eq!(
        disk.allDirectories(Some("records")).await.unwrap(),
        ["records/january", "records/january/empty"]
    );

    assert!(disk.makeDirectory("records/z.txt/child").await.is_err());
    let quiet = root.manager(false).disk(None).unwrap();
    assert!(!quiet.makeDirectory("records/z.txt/child").await.unwrap());
    assert!(!quiet.deleteDirectory("records/z.txt").await.unwrap());
    assert!(matches!(
        quiet.deleteDirectory("").await,
        Err(FilesystemError::InvalidPath)
    ));
    assert!(matches!(
        quiet.all_files(Some("../")).await,
        Err(FilesystemError::InvalidPath)
    ));
    assert!(disk.deleteDirectory("records").await.unwrap());
    assert!(disk.deleteDirectory("records").await.unwrap());
    assert!(!disk.directoryExists("records").await.unwrap());
    assert_eq!(disk.get("outside.txt").await.unwrap().unwrap(), b"other");
}

#[cfg(unix)]
#[tokio::test]
async fn local_directory_operations_preserve_visibility_and_link_boundaries() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let root = Sandbox::new();
    let manager = root.manager(true);
    let private = manager.disk(None).unwrap();
    let public = manager.disk(Some("public")).unwrap();
    assert!(private.makeDirectory("folder/nested").await.unwrap());
    assert!(public.makeDirectory("folder/nested").await.unwrap());
    for (path, mode) in [
        ("private/folder", 0o700),
        ("private/folder/nested", 0o700),
        ("public/folder", 0o755),
        ("public/folder/nested", 0o755),
    ] {
        assert_eq!(
            std::fs::metadata(root.0.join(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            mode
        );
    }
    let outside = root.0.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("retained.txt"), b"retain").unwrap();
    symlink(&outside, root.0.join("public/folder/link")).unwrap();
    symlink(
        outside.join("retained.txt"),
        root.0.join("public/folder/linked.txt"),
    )
    .unwrap();
    std::fs::write(
        root.0.join("public/folder/.storage-write-pending"),
        b"stage",
    )
    .unwrap();
    assert!(public.all_files(Some("folder")).await.unwrap().is_empty());
    assert_eq!(
        public.allDirectories(Some("folder")).await.unwrap(),
        ["folder/nested"]
    );
    assert!(public.files(Some("folder/link")).await.is_err());
    assert!(public.makeDirectory("folder/link/other").await.is_err());
    assert!(public.deleteDirectory("folder/link").await.is_err());
    assert!(public.deleteDirectory("folder").await.unwrap());
    assert_eq!(
        std::fs::read(outside.join("retained.txt")).unwrap(),
        b"retain"
    );
}

#[tokio::test]
async fn local_metadata_uses_the_open_file_and_preserves_error_policy() {
    let root = Sandbox::new();
    let disk = root.manager(true).disk(None).unwrap();
    let content = vec![b'x'; 600_000];
    assert!(disk.put("reports/large.bin", &content).await.unwrap());
    assert_eq!(disk.size("reports/large.bin").await.unwrap(), 600_000);
    assert_eq!(
        disk.lastModified("reports/large.bin").await.unwrap(),
        std::fs::metadata(root.0.join("private/reports/large.bin"))
            .unwrap()
            .modified()
            .unwrap()
    );
    assert_eq!(
        disk.checksumWith::<md5::Md5>("reports/large.bin")
            .await
            .unwrap()
            .as_deref(),
        Some("bbef3d09d1fd68cfb48e7ae1acc56a17")
    );
    assert_eq!(
        disk.mime_type("reports/large.bin")
            .await
            .unwrap()
            .as_deref(),
        Some("application/octet-stream")
    );
    assert!(
        disk.put("reports/image.txt", b"\x89PNG\r\n\x1a\n")
            .await
            .unwrap()
    );
    assert_eq!(
        disk.mime_type("reports/image.txt")
            .await
            .unwrap()
            .as_deref(),
        Some("image/png")
    );

    let quiet = root.manager(false).disk(None).unwrap();
    assert!(matches!(
        quiet.size("missing.txt").await,
        Err(FilesystemError::NotFound)
    ));
    assert!(matches!(
        quiet.lastModified("missing.txt").await,
        Err(FilesystemError::NotFound)
    ));
    assert!(matches!(
        disk.mime_type("missing.txt").await,
        Err(FilesystemError::NotFound)
    ));
    assert!(matches!(
        disk.checksum("missing.txt").await,
        Err(FilesystemError::NotFound)
    ));
    assert_eq!(quiet.mime_type("missing.txt").await.unwrap(), None);
    assert_eq!(quiet.checksum("missing.txt").await.unwrap(), None);
    assert!(matches!(
        quiet.mime_type("../outside.txt").await,
        Err(FilesystemError::InvalidPath)
    ));
    assert!(disk.size("reports").await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn local_metadata_rejects_symbolic_and_hard_links() {
    let root = Sandbox::new();
    let disk = root.manager(true).disk(None).unwrap();
    assert!(disk.put("inside.txt", b"inside").await.unwrap());
    let outside = root.0.join("outside.txt");
    std::fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, root.0.join("private/soft.txt")).unwrap();
    std::fs::hard_link(&outside, root.0.join("private/hard.txt")).unwrap();
    for name in ["soft.txt", "hard.txt"] {
        assert!(disk.size(name).await.is_err());
        assert!(disk.lastModified(name).await.is_err());
        assert!(disk.mime_type(name).await.is_err());
        assert!(disk.checksum(name).await.is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn local_permission_maps_apply_to_writes_directories_and_visibility_reads() {
    use std::os::unix::fs::PermissionsExt as _;

    use openrust::filesystem::LocalPermissions;

    let root = Sandbox::new();
    let config = LocalDiskConfig {
        root: "on-demand".into(),
        visibility: Visibility::Private,
        directory_visibility: Some(Visibility::Public),
        throw: true,
        report: false,
    };
    let permissions = LocalPermissions::new(0o640, 0o620, 0o775, 0o750).unwrap();
    let disk = root
        .manager(true)
        .buildWithPermissions(config, permissions)
        .unwrap();
    assert!(disk.put("reports/example.txt", b"report").await.unwrap());
    let mode = |path: &str| {
        std::fs::metadata(root.0.join(path))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode("on-demand"), 0o775);
    assert_eq!(mode("on-demand/reports"), 0o775);
    assert_eq!(mode("on-demand/reports/example.txt"), 0o620);
    assert_eq!(
        disk.getVisibility("reports/example.txt").await.unwrap(),
        Visibility::Private
    );
    assert!(
        disk.setVisibility("reports/example.txt", Visibility::Public)
            .await
            .unwrap()
    );
    assert_eq!(mode("on-demand/reports/example.txt"), 0o640);
    assert_eq!(
        disk.getVisibility("reports/example.txt").await.unwrap(),
        Visibility::Public
    );
    assert!(matches!(
        LocalPermissions::new(0o4644, 0o600, 0o755, 0o700),
        Err(FilesystemError::InvalidConfiguration)
    ));
}

#[tokio::test]
async fn repeated_separators_share_local_files_and_preserve_s3_path_prefix_text() {
    let root = Sandbox::new();
    let disk = root.manager(true).disk(None).unwrap();
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("reference/filesystem-paths.json")).unwrap();
    let source = reference["cases"][0]["input"].as_str().unwrap();
    let normalized = reference["cases"][0]["normalized"].as_str().unwrap();
    assert!(disk.put(source, b"image").await.unwrap());
    assert_eq!(disk.get(normalized).await.unwrap(), Some(b"image".to_vec()));
    assert_eq!(
        disk.get("avatars/nested/image.png").await.unwrap(),
        Some(b"image".to_vec())
    );
    assert_eq!(
        disk.get("avatars//nested///image.png").await.unwrap(),
        Some(b"image".to_vec())
    );
    assert_eq!(
        disk.path("avatars//nested///image.png").unwrap(),
        disk.path("avatars/nested/image.png").unwrap()
    );
    assert!(
        disk.copy("avatars//nested/image.png", "copies///image.png")
            .await
            .unwrap()
    );
    assert!(
        disk.r#move("copies///image.png", "moved///image.png")
            .await
            .unwrap()
    );
    assert!(disk.delete("moved///image.png").await.unwrap());
    for path in [
        "/absolute",
        "logs/../outside",
        "logs\\outside",
        "",
        "logs/.storage-write-test",
    ] {
        assert!(
            matches!(
                disk.put(path, b"invalid").await,
                Err(FilesystemError::InvalidPath)
            ),
            "{path}"
        );
    }
    disk.buildTemporaryUrlsUsing(|path, _, _| Box::pin(async move { Ok(path.to_owned()) }));
    disk.buildTemporaryUploadUrlsUsing(|path, _, _| {
        Box::pin(async move {
            Ok(openrust::filesystem::TemporaryUploadUrl {
                url: path.to_owned(),
                headers: Default::default(),
            })
        })
    });
    let expires = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    assert_eq!(
        disk.temporaryUrl("avatars///image.png", expires)
            .await
            .unwrap(),
        "avatars///image.png"
    );
    assert_eq!(
        disk.temporaryUploadUrl("avatars///image.png", expires)
            .await
            .unwrap()
            .url,
        "avatars///image.png"
    );
    let config = Repository::new(std::collections::BTreeMap::from([(
        "filesystems".into(),
        json!({"default":"s3","disks":{"s3":{"driver":"s3","bucket":"test-bucket","region":"us-east-1","key":"test","secret":"test","root":"tenant","url":"https://cdn.example.test/files"}}}),
    )]));
    let s3 = FilesystemManager::new(Arc::new(config), root.0.clone())
        .disk(None)
        .unwrap();
    assert_eq!(
        s3.path("avatars///image.png").unwrap(),
        PathBuf::from("tenant/avatars///image.png")
    );
    assert_eq!(
        s3.path("avatars///image.png").unwrap().to_string_lossy(),
        reference["prefix_case"].as_str().unwrap()
    );
    assert_eq!(
        s3.url("avatars///image.png").unwrap(),
        "https://cdn.example.test/files/tenant/avatars///image.png"
    );
}
