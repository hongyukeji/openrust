use openrust::console::view::components::{TerminalElement, TerminalElementKind, TerminalStyles};
use openrust::console::{ConsoleError, Output, OutputFormat, view::components::StyleArgument};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
#[ignore = "由父测试隔离终端列宽"]
fn terminal_style_inheritance_driver() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/terminal-style-inheritance-oracle.json"
    ))
    .unwrap();
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    for row in rows.into_iter().filter(|row| row["width"] == width) {
        if row["query_only"] == true {
            let mut style = TerminalStyles::new();
            style
                .set_properties(row["properties"].as_object().unwrap().clone())
                .unwrap();
            assert_eq!(style.get_inner_width(), row["inner"], "{row}");
            continue;
        }
        let mut parent = TerminalStyles::new();
        if row["case"] == "nested-layout" {
            let mut ancestor = TerminalStyles::default();
            ancestor
                .with(serde_json::Map::from_iter([
                    ("styles".into(), json!({"width":38,"pl":1,"pr":2})),
                    ("colors".into(), json!({"bg":"red"})),
                ]))
                .unwrap();
            parent.inherit_from_styles(&ancestor).unwrap();
        }
        parent
            .with(row["parent"].as_object().cloned().unwrap_or_default())
            .unwrap();
        assert_eq!(
            json!(parent.get_properties()),
            row["applied_parent"],
            "{row}"
        );
        let mut style = TerminalStyles::new();
        style
            .with(row["own"].as_object().cloned().unwrap_or_default())
            .unwrap();
        if row["attached"] == true {
            let mut element =
                TerminalElement::with_styles(TerminalElementKind::Span, "payload", style);
            element.inherit_from_styles(&parent).unwrap();
            assert_eq!(
                json!(element.get_properties().unwrap()),
                row["properties"],
                "{row}"
            );
            let output = element.to_markup();
            if row["error"].is_null() {
                assert_eq!(output.unwrap(), row["markup"], "{row}");
                assert_eq!(element.get_length(None).unwrap(), row["length"], "{row}");
                assert_eq!(element.get_inner_width().unwrap(), row["inner"], "{row}");
            } else {
                assert!(output.is_err(), "{row}");
            }
        } else {
            style.inherit_from_styles(&parent).unwrap();
            assert_eq!(json!(style.get_properties()), row["properties"], "{row}");
            let output = style.format("payload");
            if row["error"].is_null() {
                assert_eq!(output.unwrap(), row["markup"], "{row}");
                assert_eq!(style.get_length(None), row["length"], "{row}");
                assert_eq!(style.get_inner_width(), row["inner"], "{row}");
            } else {
                assert!(output.is_err(), "{row}");
            }
        }
    }
}

#[test]
fn attached_and_standalone_styles_inherit_fixed_color_spacing_and_width() {
    for width in [20, 40] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_style_inheritance_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("COLUMNS", width.to_string())
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{width}: {}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:inherited-style {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let mut parent = TerminalStyles::new();
                parent.text("cyan")?.space_x(1)?;
                let text = match mode.as_str() {
                    "invalid" => "invalid",
                    "width" => "invalid width",
                    "two" => "verified second",
                    _ => "verified first",
                };
                let mut element = TerminalElement::new(TerminalElementKind::Span, text);
                match mode.as_str() {
                    "invalid" => {
                        element.set_properties(serde_json::Map::new())?;
                    }
                    "width" => {
                        parent.w(StyleArgument::Text("1/0"))?;
                        element
                            .configure(|style| style.w(StyleArgument::Text("1/2")).map(|_| ()))?;
                    }
                    "two" => {
                        parent.bg("black")?;
                    }
                    _ => {}
                }
                element.inherit_from_styles(&parent)?;
                element.render(output, OutputFormat::Normal)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn inherited_style_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:inherited-style".into(),
        std::env::var("INHERITED_STYLE_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_explicit_inheritance_keeps_cli_output_and_errors() {
    for (mode, code, message) in [
        ("one", 0, "verified first"),
        ("two", 0, "verified second"),
        ("invalid", 1, "isFirstChild"),
        ("width", 1, "分母不能为零"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "inherited_style_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("INHERITED_STYLE_MODE", mode)
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
    openrust::support::facades::Route::get("/inherited-style", response)?;
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
            "components:inherited-style",
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
async fn actual_http_inherited_styles_keep_request_layout_local() {
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
            .get(format!("http://{address}/inherited-style?mode={mode}"))
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
                .get(format!("http://{address}/inherited-style?mode={mode}"))
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
