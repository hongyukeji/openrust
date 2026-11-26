use openrust::console::view::components::{
    TerminalElement, TerminalElementKind, TerminalElementResult, TerminalStyles,
};
use openrust::console::{ConsoleError, Output, OutputFormat};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
#[ignore = "由父消费者隔离终端列宽"]
fn dynamic_element_vectors_driver() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/terminal-element-dispatch-oracle.json"
    ))
    .unwrap();
    for row in rows {
        let style = TerminalStyles::new().with_default_styles(["uppercase"]);
        let mut element = TerminalElement::with_styles(TerminalElementKind::Span, "hello", style);
        let method = row["name"].as_str().unwrap();
        let args = row["args"].as_array().unwrap();
        let identity = std::ptr::from_mut(&mut element);
        let result = element.call(method, args);
        if row["error"].is_null() {
            match result.unwrap() {
                TerminalElementResult::Element(current) => {
                    assert_eq!(std::ptr::from_mut(current), identity, "{row}");
                    assert_eq!(row["result"], "identity", "{row}");
                }
                TerminalElementResult::Value(value) => {
                    assert_ne!(row["result"], "identity", "{row}");
                    assert_eq!(value, row["value"], "{row}");
                }
            }
            assert_eq!(element.to_markup().unwrap(), row["markup"], "{row}");
        } else {
            assert!(result.is_err(), "{row}");
        }
        assert_eq!(
            json!(element.get_properties().unwrap()),
            row["properties"],
            "{row}"
        );
    }
}

#[test]
fn programmatic_style_calls_match_fixed_element_forwarding() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "dynamic_element_vectors_driver",
            "--ignored",
            "--nocapture",
        ])
        .env("COLUMNS", "40")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}

#[test]
fn native_names_preserve_receiver_identity_and_named_query_shape() {
    let mut value = TerminalElement::new(TerminalElementKind::Span, "工具");
    assert!(matches!(
        value.call("font_bold", &[json!(true)]).unwrap(),
        TerminalElementResult::Element(_)
    ));
    let TerminalElementResult::Value(properties) = value.call("get_properties", &[]).unwrap()
    else {
        panic!("属性查询必须提供值");
    };
    assert_eq!(properties["options"]["bold"], true);
    let TerminalElementResult::Value(length) = value.call("get_length", &[]).unwrap() else {
        panic!("列宽查询必须提供值");
    };
    assert_eq!(length, 4);
    assert!(matches!(
        value.call("Get_Length", &[]).unwrap(),
        TerminalElementResult::Element(_)
    ));
    assert!(matches!(
        value.call("get__length", &[]).unwrap(),
        TerminalElementResult::Element(_)
    ));
    assert!(value.call("get_paddings", &[]).is_err());
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:element-dispatch {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let value = if mode == "two" { "second" } else { "first" };
                let mut element = TerminalElement::with_styles(
                    TerminalElementKind::Span,
                    value,
                    TerminalStyles::new(),
                );
                match mode.as_str() {
                    "invalid" => {
                        element.call("text", &[json!(false)])?;
                    }
                    "width" => {
                        element.call("get_margins", &[])?;
                    }
                    _ => {
                        element.call("prepend", &[json!("verified ")])?;
                    }
                }
                let TerminalElementResult::Value(length) =
                    element.call("get_length", &[json!(value)])?
                else {
                    return Err(ConsoleError::new("元素未返回长度"));
                };
                assert_eq!(length, json!(value.len()));
                element.render(output, OutputFormat::Normal)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn terminal_dispatch_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:element-dispatch".into(),
        std::env::var("TERMINAL_DISPATCH_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_dispatch_values_and_errors() {
    for (mode, code, message) in [
        ("one", 0, "verified first"),
        ("two", 0, "verified second"),
        ("invalid", 1, "参数类型不匹配"),
        ("width", 1, "不能调用私有终端样式方法"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_dispatch_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TERMINAL_DISPATCH_MODE", mode)
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(child.status.code(), Some(code), "{text}");
        assert!(text.contains(message), "{text}");
    }
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/element-dispatch", response)?;
    Ok(())
}

fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let mode = request
            .query(Some("mode"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "one".into());
        let mut output = Output::default();
        let result = openrust::support::facades::Artisan::call(
            "components:element-dispatch",
            &[mode.into()],
            &mut output,
        )
        .await;
        openrust::http::Response::json(
            if matches!(result, Ok(0)) {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            json!({"output":output.stdout}),
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_dispatch_isolates_errors_and_values() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
    };
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }

    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    for mode in ["invalid", "width"] {
        let response = client
            .get(format!("http://{address}/element-dispatch?mode={mode}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
    }
    let mut calls = tokio::task::JoinSet::new();
    for index in 0..12 {
        let client = client.clone();
        let (mode, expected) = if index % 2 == 0 {
            ("one", "verified first")
        } else {
            ("two", "verified second")
        };
        calls.spawn(async move {
            let response = client
                .get(format!("http://{address}/element-dispatch?mode={mode}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let content: Value = response.json().await.unwrap();
            assert_eq!(content["output"].as_str().unwrap().trim(), expected);
        });
    }
    while let Some(result) = calls.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
