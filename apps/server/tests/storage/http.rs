//! 验证聚合宿主中的存储 HTTP 流和授权边界。

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use figment::providers::Format;
use openrust::core::Error;
use openrust::http::{AppBuilderHttpExt, HttpPlugin, Inject, Path, build_router};
use openrust::prelude::{AppBuilder, Application, NoConfig};
use openrust::storage::{Storage, StoragePath, StoragePlugin, UploadOptions};
use openrust::{
    config::{FrameworkConfig, HttpStatusMode},
    lifecycle,
};
use std::time::Duration;

struct StorageHttpApp;

impl Application for StorageHttpApp {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin)
            .plugin(StoragePlugin)
            .routes(|routes| {
                routes
                    .put("/objects/{key}", upload)
                    .get("/objects/{key}", download)
            });
    }
}

async fn upload(
    Inject(storage): Inject<Storage>,
    Path(key): Path<String>,
    request: Request,
) -> Result<StatusCode, Error> {
    authorize(&request)?;
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    storage
        .default_disk()?
        .put_body(
            StoragePath::new(key)?,
            content_type,
            request.into_body(),
            UploadOptions::default(),
            None,
        )
        .await?;
    Ok(StatusCode::CREATED)
}

async fn download(
    Inject(storage): Inject<Storage>,
    Path(key): Path<String>,
    request: Request,
) -> Result<Response, Error> {
    authorize(&request)?;
    let disk = storage.default_disk()?;
    let reader = disk.open(&StoragePath::new(key)?, None).await?;
    let mut response = Response::new(Body::from_stream(reader.body));
    if let Some(content_type) = reader.metadata.content_type {
        let value = HeaderValue::from_str(&content_type)
            .map_err(|error| Error::custom("storage_invalid_content_type", error.to_string()))?;
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    Ok(response)
}

fn authorize(request: &Request) -> Result<(), Error> {
    if request.headers().get("x-storage-test-token") == Some(&HeaderValue::from_static("allowed")) {
        Ok(())
    } else {
        Err(Error::Forbidden("需要对象访问授权".into()))
    }
}

#[tokio::test]
async fn assembled_server_streams_upload_and_download_over_tcp_with_authorization()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!(
        "openrust-storage-http-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root)?;
    let encoded_root = serde_json::to_string(&root.join("objects").to_string_lossy().to_string())?;
    let figment = figment::Figment::new().merge(figment::providers::Toml::string(&format!(
        "[storage]\nmax_upload_bytes = 64\nmax_chunk_bytes = 4\n[storage.disks.local]\ndriver = 'fs'\nroot = {encoded_root}\n"
    )));
    let mut framework = FrameworkConfig::default();
    framework.api.http_status = HttpStatusMode::Semantic;
    let mut builder = AppBuilder::new(framework, figment);
    StorageHttpApp::build(&mut builder);
    let app = lifecycle::finalize(builder).await?;
    let router = build_router(&app)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = format!("http://{address}/objects/avatar.bin");
    let denied = client.put(&url).body("secret").send().await?;
    let denied_status = denied.status();
    let denied_body = denied.text().await?;
    assert_eq!(denied_status, StatusCode::FORBIDDEN, "{denied_body}");

    let uploaded = client
        .put(&url)
        .header("x-storage-test-token", "allowed")
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body("a streaming object body")
        .send()
        .await?;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let downloaded = client
        .get(&url)
        .header("x-storage-test-token", "allowed")
        .send()
        .await?;
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(
        downloaded.bytes().await?.as_ref(),
        b"a streaming object body"
    );
    let denied_download = client.get(&url).send().await?;
    assert_eq!(denied_download.status(), StatusCode::FORBIDDEN);

    server.abort();
    tokio::time::sleep(Duration::from_millis(10)).await;
    std::fs::remove_dir_all(&root)?;
    Ok(())
}
