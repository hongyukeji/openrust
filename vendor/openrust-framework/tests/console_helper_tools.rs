use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::console::{
    ConsoleError, DefaultQuestionHelper, Helper, Output, OutputFormatter, QuestionHelper,
};
use openrust::support::NumberValue;
use serde_json::{Value, json};
#[test]
fn owned_and_inherited_text_numeric_and_formatter_methods_match_fixed_source() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/helper-tools-oracle.json"
    ))
    .unwrap();
    assert_eq!(cases["mb_detect_order"], json!(["ASCII", "UTF-8"]));
    let mut differences = vec![];
    for (index, row) in cases["texts"].as_array().unwrap().iter().enumerate() {
        let data = row["bytes"].as_str().map(|v| STANDARD.decode(v).unwrap());
        let bytes = data.as_deref();
        let width = Helper::width_bytes(bytes);
        let length = Helper::length_bytes(bytes);
        if json!(width) != row["width"] || json!(length) != row["length"] {
            differences.push(json!({"index":index,"row":row,"width":width,"length":length}));
        }
        let text = bytes.map(std::str::from_utf8).transpose();
        if let Ok(text) = text {
            let widths: [fn(Option<&str>) -> usize; 3] = [
                Helper::width,
                QuestionHelper::width,
                DefaultQuestionHelper::width,
            ];
            for method in widths {
                assert_eq!(method(text), width, "{index}");
            }
            let lengths = [
                Helper::length,
                QuestionHelper::length,
                DefaultQuestionHelper::length,
            ];
            for method in lengths {
                assert_eq!(method(text), length, "{index}");
            }
        }
        for row_slice in row["slices"].as_array().unwrap() {
            let from = row_slice["from"].as_i64().unwrap();
            let len = row_slice["length"].as_i64();
            let actual = Helper::substr_bytes(bytes, from, len);
            if json!(STANDARD.encode(&actual)) != row_slice["output"] {
                differences.push(
                    json!({"index":index,"slice":row_slice,"actual":STANDARD.encode(&actual)}),
                );
            }
            if let Ok(text) = text {
                let slices = [
                    Helper::substr,
                    QuestionHelper::substr,
                    DefaultQuestionHelper::substr,
                ];
                for method in slices {
                    assert_eq!(method(text, from, len).as_bytes(), actual, "{index}");
                }
            }
        }
    }
    for (index, row) in cases["times"].as_array().unwrap().iter().enumerate() {
        let value = if row["float"] == true {
            NumberValue::Float(row["value"].as_f64().unwrap())
        } else {
            NumberValue::Integer(row["value"].as_i64().unwrap())
        };
        let precision = row["precision"].as_i64().unwrap();
        for result in [
            Helper::format_time_with(value, precision),
            QuestionHelper::format_time_with(value, precision),
            DefaultQuestionHelper::format_time_with(value, precision),
        ] {
            if result.is_err() == row["error"].is_null()
                || result.as_ref().is_ok_and(|v| json!(v) != row["output"])
            {
                differences.push(json!({"time":index,"row":row,"actual":result.ok()}));
            }
        }
        if precision == 1 {
            let result = Helper::format_time(value);
            assert_eq!(result.is_err(), !row["error"].is_null());
            if let Ok(result) = result {
                assert_eq!(result, row["output"]);
            }
        }
    }
    for row in cases["memory"].as_array().unwrap() {
        let memory = row["value"].as_i64().unwrap();
        for value in [
            Helper::format_memory(memory),
            QuestionHelper::format_memory(memory),
            DefaultQuestionHelper::format_memory(memory),
        ] {
            assert_eq!(value, row["output"]);
        }
    }
    for row in cases["format"].as_array().unwrap() {
        let formatter = OutputFormatter::new(row["ansi"].as_bool().unwrap());
        let result = Helper::remove_decoration(&formatter, row["text"].as_str());
        assert_eq!(result.is_err(), !row["error"].is_null(), "{row}");
        if let Ok(value) = result {
            assert_eq!(value, row["output"], "{row}");
        }
        assert_eq!(formatter.is_decorated(), row["after"], "{row}");
        for method in [
            QuestionHelper::remove_decoration,
            DefaultQuestionHelper::remove_decoration,
        ] {
            let formatter = OutputFormatter::new(row["ansi"].as_bool().unwrap());
            let result = method(&formatter, row["text"].as_str());
            assert_eq!(result.is_err(), !row["error"].is_null());
            if let Ok(value) = result {
                assert_eq!(value, row["output"]);
            }
            assert_eq!(formatter.is_decorated(), row["after"]);
        }
    }
    assert!(
        differences.is_empty(),
        "{}",
        serde_json::to_string_pretty(&differences).unwrap()
    );
}
fn run_view(view: &str) -> Result<Value, ConsoleError> {
    let formatter = OutputFormatter::new(true);
    if view == "invalid" {
        Helper::remove_decoration(&formatter, Some("<fg=#invalid>Bad</>"))?;
        unreachable!();
    }
    let text = if view == "unicode" {
        "界e\u{301}"
    } else {
        "abc"
    };
    let stripped =
        QuestionHelper::remove_decoration(&formatter, Some("<info>Ready</info>\x1b[31m!\x1b[0m"))?;
    Ok(
        json!({"width":QuestionHelper::width(Some(text)),"length":DefaultQuestionHelper::length(Some(text)),"slice":Helper::substr(Some(text),1,Some(1)),"time":Helper::format_time_with(61.001,2)?,"memory":Helper::format_memory(1024),"stripped":stripped,"decorated":formatter.is_decorated()}),
    )
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "helpers:tools {view=normal}",
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
fn helper_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        "helpers:tools".into(),
        std::env::var("HELPER_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_helpers_and_invalid_format_exit() {
    for view in ["normal", "unicode", "invalid"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "helper_cli_driver", "--ignored", "--nocapture"])
            .env("HELPER_VIEW", view)
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
            assert!(String::from_utf8_lossy(&result.stdout).contains("Ready!"));
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/helper", response)?;
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
        let result =
            openrust::support::facades::Artisan::call("helpers:tools", &[view.into()], &mut output)
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
async fn actual_http_helper_failure_recovery_and_parallel_formatters() {
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
            .get(format!("http://{address}/helper?view=invalid"))
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
                .get(format!("http://{address}/helper?view={view}"))
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
