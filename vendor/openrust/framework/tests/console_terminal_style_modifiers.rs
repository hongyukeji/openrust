use openrust::console::{Output, OutputFormat};

use openrust::console::{
    ConsoleError,
    view::components::{
        StyleArgument, TerminalElement, TerminalElementKind, TerminalStyleModifiers, TerminalStyles,
    },
};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};

#[test]
#[ignore = "由父消费者隔离终端列宽"]
fn terminal_style_modifiers_vectors_driver() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/terminal-style-modifiers-oracle.json"
    ))
    .unwrap();
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    for row in rows.into_iter().filter(|row| row["width"] == width) {
        let kind = row["kind"].as_str().unwrap();
        let events = Arc::new(Mutex::new(Vec::<Value>::new()));
        let mut modifiers = TerminalStyleModifiers::new();
        let reaction = events.clone();
        let fails = kind == "body-error";
        modifiers.set_text("prefix", move |value, properties, parent| {
            reaction
                .lock()
                .unwrap()
                .push(json!(["prefix", value, properties, parent]));
            if fails {
                return Err(ConsoleError::new("text stop"));
            }
            Ok(format!("x:{value}"))
        });
        let reaction = events.clone();
        modifiers.set_text("suffix", move |value, properties, parent| {
            reaction
                .lock()
                .unwrap()
                .push(json!(["suffix", value, properties, parent]));
            Ok(format!("{value}:y"))
        });
        let reaction = events.clone();
        let fails = kind == "style-error";
        modifiers.set_style("outer", move |value, properties| {
            reaction
                .lock()
                .unwrap()
                .push(json!(["outer", value, properties]));
            if fails {
                return Err(ConsoleError::new("style stop"));
            }
            Ok(format!("[{value}]"))
        });
        let reaction = events.clone();
        modifiers.set_style("count", move |value, properties| {
            reaction
                .lock()
                .unwrap()
                .push(json!(["count", value, properties]));
            Ok(format!("{value}!"))
        });

        let mut style = TerminalStyles::new().with_modifiers(modifiers);
        style
            .with(Map::from_iter([
                ("isFirstChild".into(), Value::Bool(true)),
                ("colors".into(), json!({"fg":"green"})),
                ("styles".into(), json!({"width":12,"pl":2})),
            ]))
            .unwrap();
        match kind {
            "native" => {
                style
                    .uppercase()
                    .underline()
                    .unwrap()
                    .font_bold()
                    .prepend("A-")
                    .unwrap()
                    .prepend("B-")
                    .unwrap();
            }
            "inherit" => {
                let mut parent = TerminalStyles::new();
                parent.w(StyleArgument::Integer(16)).unwrap().pr(2).unwrap();
                style
                    .inherit_from_styles(&parent)
                    .unwrap()
                    .w(StyleArgument::Text("1/2"))
                    .unwrap();
            }
            "replace" => {
                style
                    .set_properties(Map::from_iter([
                        ("isFirstChild".into(), Value::Bool(false)),
                        ("options".into(), json!({"bold":true})),
                    ]))
                    .unwrap();
            }
            _ => {}
        }
        assert_eq!(json!(style.get_properties()), row["properties"], "{row}");
        let result = if row["attached"] == true {
            TerminalElement::with_styles(TerminalElementKind::Span, "data", style).to_markup()
        } else {
            style.format("data")
        };
        if row["error"].is_null() {
            assert_eq!(result.unwrap(), row["output"], "{row}");
        } else {
            let expected = if kind == "body-error" {
                "text stop"
            } else {
                "style stop"
            };
            assert_eq!(result.unwrap_err().to_string(), expected);
        }
        assert_eq!(json!(*events.lock().unwrap()), row["events"], "{row}");
    }
}

#[test]
fn terminal_modifiers_use_source_order_context_and_error_short_circuit() {
    for width in [20, 40] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_style_modifiers_vectors_driver",
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

#[test]
fn replacing_a_named_modifier_retains_its_position() {
    let mut collection = TerminalStyleModifiers::new();
    collection
        .set_text("head", |value, _, _| Ok(format!("old-{value}")))
        .set_text("tail", |value, _, _| Ok(format!("{value}!")))
        .set_text("head", |value, _, _| Ok(format!("new-{value}")));
    let styled = TerminalStyles::new().with_modifiers(collection);
    assert_eq!(styled.format("ready").unwrap(), "new-ready!");
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:style-modifiers {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let mut hooks = TerminalStyleModifiers::new();
                let text_error = mode == "invalid";
                hooks.set_text("text", move |value, _, _| {
                    if text_error {
                        return Err(ConsoleError::new("text stop"));
                    }
                    Ok(format!("verified {value}"))
                });
                let style_error = mode == "width";
                hooks.set_style("style", move |value, _| {
                    if style_error {
                        return Err(ConsoleError::new("style stop"));
                    }
                    Ok(value.to_owned())
                });
                let style = TerminalStyles::new().with_modifiers(hooks);
                let value = if mode == "two" { "second" } else { "first" };
                let mut element =
                    TerminalElement::with_styles(TerminalElementKind::Span, value, style);
                element.render(output, OutputFormat::Normal)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn terminal_modifiers_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:style-modifiers".into(),
        std::env::var("TERMINAL_MODIFIERS_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_modifier_errors_preserve_both_stages() {
    for (mode, code, message) in [
        ("one", 0, "verified first"),
        ("two", 0, "verified second"),
        ("invalid", 1, "text stop"),
        ("width", 1, "style stop"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_modifiers_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TERMINAL_MODIFIERS_MODE", mode)
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
    openrust::support::facades::Route::get("/style-modifiers", response)?;
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
            "components:style-modifiers",
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
async fn actual_http_modifiers_isolate_error_paths_and_outputs() {
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
            .get(format!("http://{address}/style-modifiers?mode={mode}"))
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
                .get(format!("http://{address}/style-modifiers?mode={mode}"))
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
