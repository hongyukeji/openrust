use openrust::console::{AsOutput as _, ConsoleError, Output, OutputFormat};
use std::sync::Arc;

use openrust::console::view::components::{StyleArgument, TerminalStyles};
use serde_json::{Value, json};

#[test]
fn raw_terminal_properties_match_fixed_replace_and_merge() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/terminal-style-properties-oracle.json"
    ))
    .unwrap();
    for row in rows.into_iter().filter(|row| row["attached"] == false) {
        let mut style = TerminalStyles::new();
        for (step, snapshot) in row["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["progress"].as_array().unwrap())
        {
            let values = step.as_array().unwrap();
            let number = |index: usize| values[index].as_i64().unwrap();
            let text = |index: usize| values[index].as_str().unwrap();
            match text(0) {
                "with" => style.with(values[1].as_object().unwrap().clone()).unwrap(),
                "setProperties" => style
                    .set_properties(values[1].as_object().unwrap().clone())
                    .unwrap(),
                "fontBold" => style.font_bold(),
                "fontNormal" => style.font_normal().unwrap(),
                "flex" => style.flex().unwrap(),
                "flex1" => style.flex1().unwrap(),
                "justifyCenter" => style.justify_center().unwrap(),
                "w" => style.w(StyleArgument::Integer(number(1))).unwrap(),
                "wAuto" => style.w_auto().unwrap(),
                "ml" => style.ml(number(1)).unwrap(),
                "mr" => style.mr(number(1)).unwrap(),
                "mx" => style.mx(number(1)).unwrap(),
                "p" => style.p(number(1)).unwrap(),
                "px" => style.px(number(1)).unwrap(),
                "href" => style.href(text(1)).unwrap(),
                "truncate" => style.truncate_with(number(1), text(2)).unwrap(),
                "addStyle" => style.add_style(text(1)).unwrap(),
                method => panic!("未知终端样式入口：{method}"),
            };
            assert_eq!(json!(style.get_properties()), *snapshot, "{row}");
        }
        assert_eq!(json!(style.get_properties()), row["properties"], "{row}");
        let result = style.format("Hello world");
        if row["error"].is_null() {
            assert_eq!(result.unwrap(), row["markup"], "{row}");
        } else {
            assert!(result.is_err(), "{row}");
        }
    }
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:style-properties {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let mut styles = TerminalStyles::new();
                styles.with(serde_json::Map::from_iter([
                    ("styles".into(), json!({"ml": 1})),
                    ("colors".into(), json!({"fg": "cyan"})),
                ]))?;
                let text = match mode.as_str() {
                    "invalid" => {
                        styles.set_properties(serde_json::Map::new())?;
                        "invalid"
                    }
                    "width" => {
                        styles.with(serde_json::Map::from_iter([(
                            "styles".into(),
                            json!({"width": "1/0"}),
                        )]))?;
                        "invalid width"
                    }
                    "two" => {
                        styles.with(serde_json::Map::from_iter([(
                            "colors".into(),
                            json!({"bg": "black"}),
                        )]))?;
                        assert_eq!(styles.get_properties()["colors"]["fg"], "cyan");
                        "verified second"
                    }
                    _ => "verified first",
                };
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
fn terminal_properties_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:style-properties".into(),
        std::env::var("TERMINAL_PROPERTIES_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_style_properties_format_and_report_invalid_shape() {
    for (mode, code, message) in [
        ("one", 0, "verified first"),
        ("two", 0, "verified second"),
        ("invalid", 1, "isFirstChild"),
        ("width", 1, "分母不能为零"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_properties_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TERMINAL_PROPERTIES_MODE", mode)
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
    openrust::support::facades::Route::get("/style-properties", response)?;
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
            "components:style-properties",
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
async fn actual_http_styles_keep_property_updates_local() {
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
            .get(format!("http://{address}/style-properties?mode={mode}"))
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
                .get(format!("http://{address}/style-properties?mode={mode}"))
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
