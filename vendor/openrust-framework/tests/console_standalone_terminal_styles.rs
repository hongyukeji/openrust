use openrust::console::{AsOutput as _, ConsoleError, Output, OutputFormat};
use std::sync::Arc;

use openrust::console::view::components::{StyleRepository, TerminalStyles};
use serde_json::{Value, json};

#[test]
#[ignore = "由父测试隔离终端列宽"]
fn standalone_styles_vectors_driver() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/standalone-terminal-styles-oracle.json"
    ))
    .unwrap();
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    for row in rows.into_iter().filter(|row| row["width"] == width) {
        let mut styles = TerminalStyles::new().with_default_styles(
            row["defaults"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap()),
        );
        let content = row["content"].as_str().unwrap();
        let result = (|| {
            if let Some(classes) = row["classes"].as_str() {
                styles.add_style(classes)?;
            }
            styles.format(content)
        })();
        if row["error"].is_null() {
            assert_eq!(result.unwrap(), row["out"], "{row}");
        } else {
            assert!(result.is_err(), "{row}");
        }
        assert_eq!(
            json!(styles.default_styles().collect::<Vec<_>>()),
            row["defaults"]
        );
        assert_eq!(styles.get_length(Some(content)), row["length"], "{row}");
        assert_eq!(styles.get_length(None), row["none_length"], "{row}");
        assert_eq!(styles.get_inner_width(), row["inner_width"], "{row}");
    }
}

#[test]
fn independent_styles_match_fixed_defaults_width_format_and_text_lengths() {
    for width in [20, 40, 83] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "standalone_styles_vectors_driver",
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
fn detached_style_reuses_repository_nested_defaults_and_distinct_arguments() {
    let repository = StyleRepository::new();
    repository.create("brand").color("#a23").unwrap();
    repository
        .create_with("flag", |styles, _| {
            assert_eq!(styles.default_styles().collect::<Vec<_>>(), ["font-bold"]);
            styles.uppercase();
            Ok(())
        })
        .apply("text-brand");
    let mut styles = TerminalStyles::default()
        .with_default_styles(["font-bold"])
        .with_repository(repository);
    styles.apply("flag").unwrap();
    assert_eq!(
        styles.format("ready").unwrap(),
        "<fg=#a23;options=bold>READY</>"
    );
    let mut styles = TerminalStyles::new().with_default_styles(["text-red font-bold"]);
    assert!(styles.add_style("").is_err());
}

#[test]
#[ignore = "由父测试隔离终端列宽"]
fn terminal_parent_width_driver() {
    use openrust::console::view::components::{StyleArgument, TerminalDimensions};
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/terminal-parent-width-oracle.json"
    ))
    .unwrap();
    let viewport = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    for row in rows.into_iter().filter(|row| row["viewport"] == viewport) {
        let chain = row["ancestors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                let number = |index: usize| value[index].as_i64().unwrap();
                TerminalDimensions {
                    width: if let Some(text) = value[0].as_str() {
                        StyleArgument::Text(text)
                    } else {
                        StyleArgument::Integer(number(0))
                    },
                    min_width: number(1),
                    max_width: number(2),
                    margin: [number(3), number(4)],
                    padding: [number(5), number(6)],
                }
            })
            .collect::<Vec<_>>();
        let result = TerminalStyles::get_parent_width(&chain);
        if row["error"].is_null() {
            assert_eq!(result.unwrap(), row["width"], "{row}");
        } else {
            assert!(result.is_err(), "{row}");
        }
    }
}

#[test]
fn explicit_parent_width_preserves_integer_text_and_nested_constraints() {
    for width in [20, 40, 83] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_parent_width_driver",
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
        "components:standalone-style {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let mut styles =
                    TerminalStyles::new().with_default_styles(["font-bold", "text-cyan"]);
                let text = match mode.as_str() {
                    "invalid" => {
                        styles.text("unknown")?;
                        "invalid"
                    }
                    "width" => {
                        styles.add_style("w-1/0")?;
                        "invalid width"
                    }
                    "two" => "verified second",
                    _ => "verified first",
                };
                styles.add_style("ml-1")?;
                let text = styles.format(text)?;
                output.as_output().writeln(&text, OutputFormat::Normal)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn standalone_style_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:standalone-style".into(),
        std::env::var("STANDALONE_STYLES_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_standalone_style_formats_text_and_returns_errors() {
    for (mode, code, message) in [
        ("one", 0, "verified first"),
        ("two", 0, "verified second"),
        ("invalid", 1, "unknown"),
        ("width", 1, "分母不能为零"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "standalone_style_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("STANDALONE_STYLES_MODE", mode)
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
    openrust::support::facades::Route::get("/standalone-style", response)?;
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
            "components:standalone-style",
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
async fn actual_http_standalone_styles_keep_request_state_after_error() {
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
            .get(format!("http://{address}/standalone-style?mode={mode}"))
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
                .get(format!("http://{address}/standalone-style?mode={mode}"))
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
