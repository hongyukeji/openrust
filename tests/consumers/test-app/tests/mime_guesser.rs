//! 独立 crate 验证公开 MIME 检测器能读取受管上传文件，并随应用注册状态变化。

use figment::{Figment, providers::Serialized};
use openrust::config::{FrameworkConfig, NoConfig};
use openrust::core::{
    AppState, Application, BootContext, BoxFuture, RegisterContext, Result, ServiceProvider,
};
use openrust::http::{
    HttpServiceProvider, MimeFile, MimeFileReader, MimeGuess, MimeTypeGuesser, MimeTypes,
    Request as InputRequest, UploadedFile,
    axum::{
        Json,
        body::Body,
        extract::State,
        http::{Request, StatusCode},
    },
};
use openrust::testing::TestApp;
use openrust::tokio::io::AsyncReadExt;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

const CONTENT: &[u8] = b"consumer mime payload";

struct ContentGuesser(&'static str);

impl MimeTypeGuesser for ContentGuesser {
    fn guess(
        &self,
        file: MimeFile,
    ) -> Pin<Box<dyn Future<Output = Result<MimeGuess>> + Send + '_>> {
        Box::pin(async move {
            // 两个独立游标同时读取同一受管文件，可发现外部检测器拿到失效暂存句柄。
            let mut first: MimeFileReader = file.clone().open().await?;
            let mut second: MimeFileReader = file.open().await?;
            let mut first_bytes = Vec::new();
            let mut second_bytes = Vec::new();
            first.read_to_end(&mut first_bytes).await.map_err(|_| {
                openrust::Error::custom("consumer_mime_read_failed", "检测器读取失败")
            })?;
            second.read_to_end(&mut second_bytes).await.map_err(|_| {
                openrust::Error::custom("consumer_mime_read_failed", "检测器读取失败")
            })?;
            Ok(if first_bytes == CONTENT && second_bytes == CONTENT {
                MimeGuess::Detected(self.0.to_owned())
            } else {
                MimeGuess::Unknown
            })
        })
    }
}

struct MimeRouteProvider;

impl ServiceProvider for MimeRouteProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // factory 延迟实例化，首个 multipart 必须通过 Container resolve 取得覆盖。
            app.singleton::<MimeTypes, _>(|_| {
                Box::pin(async {
                    let mime = MimeTypes::new();
                    mime.register_guesser(Arc::new(ContentGuesser("application/pdf")));
                    Ok(Arc::new(mime))
                })
            })
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes.post(
                    "/api/mime-consumer",
                    |State(state): State<AppState>, request: InputRequest| async move {
                        let value = request.file("document")?.ok_or_else(|| {
                            openrust::Error::custom("consumer_file_missing", "文件缺失")
                        })?;
                        let file = value.downcast_ref::<UploadedFile>().ok_or_else(|| {
                            openrust::Error::custom("consumer_file_invalid", "文件无效")
                        })?;
                        let before_mime = file.mime_type().await?;
                        let before_name = file.hash_name().await?;
                        let mut after_mime = before_mime.clone();
                        let mut after_name = before_name.clone();
                        if request
                            .input("late")?
                            .as_ref()
                            .and_then(|value| value.as_str())
                            == Some("yes")
                        {
                            // 请求已经提取文件后再登记，验证 UploadedFile 与 App 共享注册表。
                            state
                                .mime_types()
                                .await?
                                .register_guesser(Arc::new(ContentGuesser("image/png")));
                            after_mime = file.mime_type().await?;
                            after_name = file.hash_name().await?;
                        }
                        Ok::<_, openrust::Error>(Json(serde_json::json!({
                            "before_mime": before_mime,
                            "after_mime": after_mime,
                            "before_name": before_name,
                            "after_name": after_name,
                            "client_extension": file.client_extension(),
                            "client_name": file.client_original_name(),
                        })))
                    },
                )
            })?;
            Ok(())
        })
    }
}

struct ConsumerApplication;

impl Application for ConsumerApplication {
    type Config = NoConfig;

        fn default_providers() -> openrust::core::DefaultProviders {
            openrust::core::DefaultProviders::empty()
        }

    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(MimeRouteProvider)]
    }
}

fn source() -> Figment {
    Figment::from(Serialized::defaults(FrameworkConfig::default()))
}

fn upload(late: bool) -> std::result::Result<Request<Body>, openrust::http::axum::http::Error> {
    let late_field = if late {
        "--consumer-mime\r\nContent-Disposition: form-data; name=\"late\"\r\n\r\nyes\r\n"
    } else {
        ""
    };
    Request::builder()
        .method("POST")
        .uri("/api/mime-consumer")
        .header("content-type", "multipart/form-data; boundary=consumer-mime")
        .body(Body::from(format!(
            "{late_field}--consumer-mime\r\nContent-Disposition: form-data; name=\"document\"; filename=\"claimed.gif\"\r\nContent-Type: image/gif\r\n\r\nconsumer mime payload\r\n--consumer-mime--\r\n"
        )))
}

#[tokio::test]
async fn external_guesser_reads_managed_file_and_registration_stays_with_its_app()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut first = TestApp::<ConsumerApplication>::builder(source())
        .build()
        .await?;
    let mut second = TestApp::<ConsumerApplication>::builder(source())
        .build()
        .await?;
    second
        .app()?
        .mime_types()
        .await?
        .register_guesser(Arc::new(ContentGuesser("application/json")));

    let first_body = first.request(upload(true)?).await?;
    first_body.assert_status(StatusCode::OK)?;
    let first_json: serde_json::Value = serde_json::from_slice(first_body.body())?;
    assert_eq!(first_json["before_mime"], "application/pdf");
    assert_eq!(first_json["after_mime"], "image/png");
    assert_eq!(first_json["client_extension"], "gif");
    assert_eq!(first_json["client_name"], "claimed.gif");
    let before = first_json["before_name"]
        .as_str()
        .ok_or("缺少原始 hash 名")?;
    let after = first_json["after_name"]
        .as_str()
        .ok_or("缺少更新后 hash 名")?;
    assert_eq!(before.strip_suffix(".pdf"), after.strip_suffix(".png"));
    assert_eq!(before.len(), 44);

    let second_body = second.request(upload(false)?).await?;
    second_body.assert_status(StatusCode::OK)?;
    let second_json: serde_json::Value = serde_json::from_slice(second_body.body())?;
    assert_eq!(second_json["before_mime"], "application/json");
    assert_eq!(second_json["after_mime"], "application/json");
    assert!(
        second_json["before_name"]
            .as_str()
            .is_some_and(|name| name.ends_with(".json"))
    );

    // 晚重绑应影响下一次提取，且另一 App 的 MIME 单例保持独立。
    first
        .app()?
        .state()
        .container()
        .singleton::<MimeTypes, _>(|_| {
            Box::pin(async {
                let mime = MimeTypes::new();
                mime.register_guesser(Arc::new(ContentGuesser("text/plain")));
                Ok(Arc::new(mime))
            })
        })
        .await?;
    let rebound_body = first.request(upload(false)?).await?;
    rebound_body.assert_status(StatusCode::OK)?;
    let rebound_json: serde_json::Value = serde_json::from_slice(rebound_body.body())?;
    assert_eq!(rebound_json["before_mime"], "text/plain");
    assert!(
        rebound_json["before_name"]
            .as_str()
            .is_some_and(|name| name.ends_with(".txt"))
    );
    let isolated_body = second.request(upload(false)?).await?;
    isolated_body.assert_status(StatusCode::OK)?;
    let isolated_json: serde_json::Value = serde_json::from_slice(isolated_body.body())?;
    assert_eq!(isolated_json["before_mime"], "application/json");

    let deadline = openrust::tokio::time::Instant::now() + Duration::from_secs(5);
    first.shutdown(deadline).await?;
    second.shutdown(deadline).await?;
    Ok(())
}
