use openrust::console::{
    ConsoleError, DebugFormatterHelper, FormatterHelper, HelperSet, Output, OutputMessages,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/formatter-helper-oracle.json"
    ))
    .unwrap()
}

#[test]
fn formatter_and_debug_helpers_match_fixed_source_values() {
    let cases = oracle();
    let helper = FormatterHelper::new();
    let debug = DebugFormatterHelper::new();
    assert_eq!(helper.get_name(), cases["formatter_name"]);
    assert_eq!(debug.get_name(), cases["debug_name"]);

    for row in cases["section"].as_array().unwrap() {
        let message = row["message"].as_str().unwrap();
        let style = row["style"].as_str().unwrap();
        assert_eq!(
            helper.format_section_with("api:check", message, style),
            row["output"]
        );
        if style == "info" {
            assert_eq!(helper.format_section("api:check", message), row["output"]);
        }
    }

    for row in cases["block"].as_array().unwrap() {
        let value = &row["value"];
        let lines;
        let message = if let Some(text) = value.as_str() {
            OutputMessages::Text(text)
        } else {
            lines = value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>();
            OutputMessages::Lines(&lines)
        };
        let style = row["style"].as_str().unwrap();
        let large = row["large"].as_bool().unwrap();
        assert_eq!(
            helper.format_block_with(message, style, large),
            row["output"],
            "{row}"
        );
        if !large {
            assert_eq!(helper.format_block(message, style), row["output"]);
        }
    }

    for row in cases["truncate"].as_array().unwrap() {
        let message = row["message"].as_str().unwrap();
        let limit = row["limit"].as_i64().unwrap();
        let suffix = row["suffix"].as_str().unwrap();
        assert_eq!(
            helper.truncate_with(message, limit, suffix),
            row["output"],
            "{row}"
        );
        if suffix == "..." {
            assert_eq!(helper.truncate(message, limit), row["output"]);
        }
    }

    for row in cases["debug"].as_array().unwrap() {
        let args = row["args"].as_array().unwrap();
        let arg = |i: usize| args[i].as_str().unwrap();
        let result = match row["method"].as_str().unwrap() {
            "start" => Ok(debug.start_with(arg(0), arg(1), arg(2))),
            "progress" => {
                debug.progress_with(arg(0), arg(1), args[2].as_bool().unwrap(), arg(3), arg(4))
            }
            "stop" => debug.stop_with(arg(0), arg(1), args[2].as_bool().unwrap(), arg(3)),
            other => panic!("未知调试场景：{other}"),
        };
        if row["error"].is_null() {
            assert_eq!(result.unwrap(), row["output"], "{row}");
        } else {
            assert!(
                result.unwrap_err().to_string().contains("尚未开始"),
                "{row}"
            );
        }
    }
}

fn run_view(view: &str) -> Result<Value, ConsoleError> {
    let set = HelperSet::new();
    let formatter = Arc::new(FormatterHelper::new());
    let debug = Arc::new(DebugFormatterHelper::new());
    set.set(formatter.clone(), None);
    set.set(debug.clone(), None);
    assert!(formatter.get_helper_set().unwrap().same_set(&set));
    assert!(debug.get_helper_set().unwrap().same_set(&set));
    if view == "invalid" {
        return debug.stop("missing", "x", false).map(|_| Value::Null);
    }
    let section = formatter.format_section(view, "Ready");
    let start = debug.start("check", view);
    let progress = debug.progress("check", "first\nsecond", false)?;
    let stop = debug.stop("check", "Done", true)?;
    Ok(json!({"section":section, "start":start, "progress":progress, "stop":stop}))
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "helpers:format {view=normal}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let view = input
                    .argument("view")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                output.comment(&run_view(&view)?.to_string());
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行真实Artisan退出码"]
fn format_helper_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        "helpers:format".into(),
        std::env::var("FORMAT_HELPER_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_format_helpers_and_unknown_session_exit() {
    for view in ["normal", "unicode", "invalid"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "format_helper_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("FORMAT_HELPER_VIEW", view)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(if view == "invalid" { 1 } else { 0 }),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if view != "invalid" {
            assert!(String::from_utf8_lossy(&result.stdout).contains("Ready"));
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/format-helper", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let view = request
            .query(Some("view"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "normal".into());
        let mut output = Output::default();
        let result = openrust::support::facades::Artisan::call(
            "helpers:format",
            &[view.into()],
            &mut output,
        )
        .await;
        openrust::http::Response::json(
            if result.is_ok() {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            json!({"output":output.stdout}),
        )
    })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_helper_failure_recovery_and_parallel_debug_sessions() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(k): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        r: axum::extract::Request,
    ) -> axum::response::Response {
        k.handle_transport(r).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(
            openrust::foundation::configuration::Routing::default()
                .web(routes)
                .commands(commands),
        )
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{address}/format-helper?view=invalid"))
            .send()
            .await
            .unwrap()
            .status(),
        500
    );
    let mut calls = tokio::task::JoinSet::new();
    for index in 0..14 {
        let client = client.clone();
        calls.spawn(async move {
            let view = if index % 2 == 0 { "normal" } else { "unicode" };
            let response = client
                .get(format!("http://{address}/format-helper?view={view}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            let value: Value =
                serde_json::from_str(body["output"].as_str().unwrap().trim()).unwrap();
            assert_eq!(value, run_view(view).unwrap());
        });
    }
    while let Some(result) = calls.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
