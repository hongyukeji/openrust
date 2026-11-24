use openrust::{
    contracts::{foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogContext,
    support::facades::{Log, Route},
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, sync::Arc};
fn config(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"production","key":"a".repeat(32)}),
        ),
        (
            "session".into(),
            json!({"driver":"array","lottery":[0,100]}),
        ),
        (
            "logging".into(),
            json!({"default":"browser","channels":{"browser":{"driver":"monolog","handler":"browser_console"}}}),
        ),
    ]))
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/{kind}/{id}", page)
}
fn page(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let kind = request.route("kind").unwrap().to_owned();
        let id = request.route("id").unwrap().to_owned();
        Log::info(
            &format!("request-{id} </script><script>globalThis.injected=true</script>"),
            &LogContext::from_iter([("id".into(), id.clone().into())]),
        )?;
        if kind == "default" {
            Log::build(
                &json!({"driver":"monolog","handler":"browser_console","formatter":"default"}),
            )?
            .warning("handler-own-style", &LogContext::new())?;
        }
        if kind == "htmlformat" {
            Log::build(&json!({"driver":"monolog","handler":"browser_console","formatter":"html","formatter_with":{"dateFormat":"Y-m-d"}}))?.info("html formatter record",&LogContext::new())?;
        }
        if kind == "typed" {
            Log::build(
                &json!({"driver":"monolog","handler":"typed_browser","formatter":"browser"}),
            )?
            .error("typed public handler", &LogContext::new())?;
        }
        let logger = Log::channel(None)?;
        match kind.as_str() {
            "trailers" => {
                let mut trailers = http::HeaderMap::new();
                trailers.insert("x-log-end", http::HeaderValue::from_static("yes"));
                let frames = vec![
                    Ok::<_, std::io::Error>(http_body::Frame::data(
                        axum::body::Bytes::from_static(b"<p>trailers</p>"),
                    )),
                    Ok(http_body::Frame::trailers(trailers)),
                ];
                Ok(
                    Response::new(StatusCode::OK, Body::new(FrameBody(frames.into()))).header(
                        http::header::CONTENT_TYPE,
                        http::HeaderValue::from_static("text/html"),
                    ),
                )
            }
            "js" => Ok(
                Response::new(StatusCode::OK, Body::from("globalThis.original=true;")).header(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("application/javascript"),
                ),
            ),
            "encoded" => Ok(Response::new(StatusCode::OK, Body::from("encoded")).header(
                http::header::CONTENT_ENCODING,
                http::HeaderValue::from_static("gzip"),
            )),
            "partial" => Ok(
                Response::new(StatusCode::PARTIAL_CONTENT, Body::from("partial")).header(
                    http::header::CONTENT_RANGE,
                    http::HeaderValue::from_static("bytes 0-6/10"),
                ),
            ),
            "json" => Response::json(StatusCode::OK, json!({"id":id})),
            "head" => Ok(Response::no_content()),
            "stream" => Ok(Response::new(
                StatusCode::OK,
                Body::from_stream(futures_util::stream::once(async move {
                    logger
                        .warning(&format!("stream-{id}"), &LogContext::new())
                        .unwrap();
                    Ok::<_, std::io::Error>(format!("<p>{id}</p>"))
                })),
            )
            .header(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/html"),
            )),
            _ => Ok(Response::from(format!("<html><body>{id}</body></html>"))),
        }
    })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_http_console_is_request_isolated_and_skips_json_head_and_cli() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::container::Container as _,
        foundation::Application as App,
        support::{ProviderResult, ServiceProvider},
    };
    struct Register;
    impl ServiceProvider for Register {
        fn boot<'a>(&'a mut self, app: &'a App) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                app.container()
                    .make::<openrust::log::LogManager>()?
                    .register_handler("typed_browser", |_| {
                        let handler = openrust::log::BrowserConsoleHandler::new()?;
                        Ok(Arc::new(move |record, formatted| {
                            handler.handle(record, formatted)
                        }))
                    });
                app.container()
                    .make::<openrust::log::LogManager>()?
                    .register_formatter("browser", |_| {
                        Ok(Arc::new(|record| {
                            Ok(format!(
                                "[[{}]]{{color:blue}} {}",
                                format!("{:?}", record.level).to_uppercase(),
                                record.message()
                            ))
                        }))
                    });
                Ok(())
            })
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_config(config)
            .with_providers(vec![Box::new(Register)])
            .with_routing(Routing::default().web(routes))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, transport.with_state(kernel).into_make_service())
            .await
            .unwrap()
    });
    let other_root = tempfile::tempdir().unwrap();
    let mut other_kernel = Kernel::new(
        Application::configure(other_root.path().into())
            .with_config(config)
            .with_providers(vec![Box::new(Register)])
            .with_routing(Routing::default().web(routes))
            .create()
            .unwrap(),
    );
    other_kernel.bootstrap().await.unwrap();
    let other_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_address = other_listener.local_addr().unwrap();
    let other_kernel = Arc::new(other_kernel);
    let other_server = tokio::spawn(async move {
        axum::serve(
            other_listener,
            transport.with_state(other_kernel).into_make_service(),
        )
        .await
        .unwrap()
    });
    let client = reqwest::Client::new();
    let mut tasks = Vec::new();
    for id in 0..16 {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            let text = client
                .get(format!("http://{address}/stream/{id}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            assert!(
                text.contains(&format!("request-{id} ")) && text.contains(&format!("stream-{id}")),
                "{text}"
            );
            assert!(!text.contains("</script><script>globalThis.injected"));
            assert_eq!(text.matches("<script>").count(), 1);
            text
        }));
    }
    let mut html = String::new();
    for task in tasks {
        html = task.await.unwrap();
    }
    let typed = client
        .get(format!("http://{address}/typed/public"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(typed.contains("typed public handler"));
    if let Ok(path) = std::env::var("BROWSER_LOG_TYPED_FIXTURE") {
        std::fs::write(path, &typed).unwrap();
    }
    let own = client
        .get(format!("http://{address}/default/own"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(own.contains("[[production]]{macro: autolabel}"));
    let html_formatted = client
        .get(format!("http://{address}/htmlformat/literal"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        html_formatted.contains("html formatter record")
            && html_formatted.contains("monolog-output")
    );

    let foreign = client
        .get(format!("http://{other_address}/stream/foreign"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        foreign.contains("request-foreign")
            && foreign.contains("stream-foreign")
            && !foreign.contains("request-15")
    );
    let restored = client
        .get(format!("http://{address}/html/restored"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(restored.contains("request-restored") && !restored.contains("request-foreign"));
    for kind in ["json", "head"] {
        let response = client
            .get(format!("http://{address}/{kind}/clean"))
            .send()
            .await
            .unwrap();
        assert!(!response.text().await.unwrap().contains("<script>"));
    }
    let response = client
        .head(format!("http://{address}/html/head-request"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "");
    // 保存实际响应，供真实浏览器执行与安全反例验证；路径由验收入口显式指定。
    if let Ok(path) = std::env::var("BROWSER_LOG_FIXTURE") {
        std::fs::write(path, html).unwrap();
    }
    server.abort();
    other_server.abort();
    assert!(!root.path().join("storage/logs/app.log").exists());
    assert!(!other_root.path().join("storage/logs/app.log").exists());
}
fn commands() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::command("logs:browser", |_| {
        Log::warning("CLI browser record", &LogContext::new()).unwrap();
    })?
    .purpose("验收浏览器日志的CLI边界")
}
#[tokio::test]
async fn actual_artisan_does_not_print_browser_script_or_fall_back_to_emergency() {
    if let Ok(root) = std::env::var("BROWSER_LOG_CLI_ROOT") {
        let app = Application::configure(root.into())
            .with_config(config)
            .with_routing(Routing::default().commands(commands))
            .create()
            .unwrap();
        assert_eq!(
            app.handle_command(&["logs:browser".into()]).await.unwrap(),
            0
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "actual_artisan_does_not_print_browser_script_or_fall_back_to_emergency",
            "--nocapture",
        ])
        .env("BROWSER_LOG_CLI_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("<script>"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("<script>"));
    assert!(!root.path().join("storage/logs/app.log").exists());
}
#[tokio::test]
async fn response_frames_preserve_trailers_and_special_content_protocols() {
    use http_body::Body as _;
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_config(config)
            .with_routing(Routing::default().web(routes))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    let response = kernel
        .clone()
        .handle_transport(
            http::Request::builder()
                .uri("/trailers/direct")
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let mut body = response.into_body();
    let mut data = String::new();
    let mut trailer_seen = false;
    while let Some(frame) =
        futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)).await
    {
        let frame = frame.unwrap();
        if let Some(bytes) = frame.data_ref() {
            assert!(!trailer_seen);
            data.push_str(std::str::from_utf8(bytes).unwrap());
        } else if let Some(trailers) = frame.trailers_ref() {
            assert_eq!(trailers["x-log-end"], "yes");
            assert!(data.contains("<script>"));
            trailer_seen = true;
        }
    }
    assert!(
        trailer_seen && data.starts_with("<p>trailers</p>"),
        "trailer={trailer_seen}, body={data}"
    );
    for (kind, expected) in [
        ("js", "globalThis.original=true;"),
        ("encoded", "encoded"),
        ("partial", "partial"),
    ] {
        let response = kernel
            .clone()
            .handle_transport(
                http::Request::builder()
                    .uri(format!("/{kind}/direct"))
                    .header("host", "localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) =
            futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx)).await
        {
            if let Some(data) = frame.unwrap().data_ref() {
                bytes.extend_from_slice(data);
            }
        }
        let text = std::str::from_utf8(&bytes).unwrap();
        if kind == "js" {
            assert!(
                text.starts_with(expected)
                    && text.contains("globalThis.console")
                    && !text.contains("<script>")
            );
        } else {
            assert_eq!(text, expected);
        }
    }
}

struct FrameBody(
    std::collections::VecDeque<Result<http_body::Frame<axum::body::Bytes>, std::io::Error>>,
);
impl http_body::Body for FrameBody {
    type Data = axum::body::Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        std::task::Poll::Ready(self.get_mut().0.pop_front())
    }
}
