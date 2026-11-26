use openrust::console::view::components::{
    StyleArgument, TerminalChild, TerminalElement, TerminalElementKind, TerminalStyles,
};
use openrust::console::{ConsoleError, Output, OutputFormat};
use serde_json::{Value, json};
use std::sync::Arc;

fn attached() -> Value {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/attached-terminal-style-oracle.json"
    ))
    .unwrap()
}

#[test]
fn element_attachment_preserves_shared_styles_and_latest_live_content() {
    use TerminalElementKind::*;
    let source = attached();
    let style = TerminalStyles::new().with_default_styles(["font-bold", "text-cyan"]);
    let mut origin = TerminalElement::with_styles(Span, "old", style);
    assert_eq!(origin.to_markup().unwrap(), source["new_markup"]);
    assert_eq!(origin.get_length(None).unwrap(), source["new_length"]);
    assert_eq!(
        origin.get_length(Some("<fg=cyan>工具🦀</>")).unwrap(),
        source["explicit_length"]
    );
    assert_eq!(origin.get_inner_width().unwrap(), source["new_inner"]);
    assert_eq!(
        json!(origin.default_styles().collect::<Vec<_>>()),
        source["default_styles"]
    );

    origin
        .configure(|styles| styles.add_style("ml-2 mr-1").map(|_| ()))
        .unwrap();
    assert_eq!(
        json!(origin.get_properties().unwrap()),
        source["first_properties"]
    );
    assert_eq!(origin.get_inner_width().unwrap(), source["first_width"]);
    let mut copy = origin.set_content("replacement");
    assert_eq!(
        origin.get_length(None).unwrap(),
        source["old_reads_latest_length"]
    );
    assert_eq!(copy.get_length(None).unwrap(), source["copy_length"]);
    assert_eq!(
        origin.get_inner_width().unwrap(),
        source["old_reads_latest_inner"]
    );
    assert_eq!(origin.to_markup().unwrap(), source["old_renders_own"]);
    assert_eq!(copy.to_markup().unwrap(), source["copy_markup"]);

    origin
        .with(serde_json::Map::from_iter([(
            "colors".into(),
            json!({"bg":"blue"}),
        )]))
        .unwrap();
    assert_eq!(origin.to_markup().unwrap(), source["after_merge_original"]);
    assert_eq!(copy.to_markup().unwrap(), source["after_merge_copy"]);
    copy.set_properties(serde_json::Map::from_iter([
        ("isFirstChild".into(), json!(true)),
        ("styles".into(), json!({"display":"block","pl":1})),
    ]))
    .unwrap();
    assert_eq!(
        origin.to_markup().unwrap(),
        source["after_replace_original"]
    );
    assert_eq!(copy.to_markup().unwrap(), source["after_replace_copy"]);
    assert_eq!(
        json!(copy.get_properties().unwrap()),
        source["after_replace_properties"]
    );

    let content = vec![TerminalChild::from(origin), copy.into(), "!".into()];
    let mut parent = TerminalElement::from_styles(Div, content, "font-bold text-red").unwrap();
    assert_eq!(parent.to_markup().unwrap(), source["nested"]);
    assert_eq!(parent.get_length(None).unwrap(), source["nested_length"]);
}

#[test]
fn weak_attachment_returns_to_live_element_when_latest_is_released() {
    // 固定 PHP 的强回指保持最后一个元素。原生弱引用避免保留有向元素环。
    assert_eq!(attached()["live_style_records_dropped_latest"], 18);
    let origin = TerminalElement::new(TerminalElementKind::Span, "old");
    let ephemeral = origin.set_content("ephemeral content");
    assert_eq!(origin.get_length(None).unwrap(), 17);
    drop(ephemeral);
    assert_eq!(origin.get_length(None).unwrap(), 3);
}

#[test]
fn attached_style_replace_merge_and_format_use_fixed_element_values() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/terminal-style-properties-oracle.json"
    ))
    .unwrap();
    for row in rows.into_iter().filter(|row| row["attached"] == true) {
        let mut element = TerminalElement::new(TerminalElementKind::Span, "Hello world");
        for (step, snapshot) in row["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["progress"].as_array().unwrap())
        {
            let values = step.as_array().unwrap();
            let method = values[0].as_str().unwrap();
            let argument = |index: usize| values[index].as_i64().unwrap();
            let text = |index: usize| values[index].as_str().unwrap();
            match method {
                "with" => {
                    element
                        .with(values[1].as_object().unwrap().clone())
                        .unwrap();
                }
                "setProperties" => {
                    element
                        .set_properties(values[1].as_object().unwrap().clone())
                        .unwrap();
                }
                _ => element
                    .configure(|style| {
                        match method {
                            "fontBold" => style.font_bold(),
                            "fontNormal" => style.font_normal()?,
                            "flex" => style.flex()?,
                            "flex1" => style.flex1()?,
                            "justifyCenter" => style.justify_center()?,
                            "w" => style.w(StyleArgument::Integer(argument(1)))?,
                            "wAuto" => style.w_auto()?,
                            "ml" => style.ml(argument(1))?,
                            "mr" => style.mr(argument(1))?,
                            "mx" => style.mx(argument(1))?,
                            "p" => style.p(argument(1))?,
                            "px" => style.px(argument(1))?,
                            "href" => style.href(text(1))?,
                            "truncate" => style.truncate_with(argument(1), text(2))?,
                            "addStyle" => style.add_style(text(1))?,
                            method => panic!("未知样式调用：{method}"),
                        };
                        Ok(())
                    })
                    .unwrap(),
            };
            assert_eq!(json!(element.get_properties().unwrap()), *snapshot, "{row}");
        }
        let result = element.to_markup();
        if row["error"].is_null() {
            assert_eq!(result.unwrap(), row["markup"], "{row}");
        } else {
            assert!(result.is_err(), "{row}");
        }
    }
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:attached-style {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let styles = TerminalStyles::new().with_default_styles(["font-bold", "text-cyan"]);
                let mut origin =
                    TerminalElement::with_styles(TerminalElementKind::Span, "before", styles);
                origin.configure(|style| style.add_style("ml-1").map(|_| ()))?;
                let next_text = match mode.as_str() {
                    "invalid" => {
                        origin.set_properties(serde_json::Map::new())?;
                        "invalid"
                    }
                    "width" => {
                        origin.with(serde_json::Map::from_iter([(
                            "styles".into(),
                            json!({"width": "1/0"}),
                        )]))?;
                        "invalid width"
                    }
                    "two" => {
                        origin.with(serde_json::Map::from_iter([(
                            "colors".into(),
                            json!({"bg": "black"}),
                        )]))?;
                        "verified second"
                    }
                    _ => "verified first",
                };
                let mut contents = origin.set_content(next_text);
                assert_eq!(origin.get_length(None)?, contents.get_length(None)?);
                contents.render(output, OutputFormat::Normal)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn attached_style_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:attached-style".into(),
        std::env::var("ATTACHED_STYLE_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_attached_styles_share_content_and_keep_exit_codes() {
    for (mode, code, message) in [
        ("one", 0, "verified first"),
        ("two", 0, "verified second"),
        ("invalid", 1, "isFirstChild"),
        ("width", 1, "分母不能为零"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "attached_style_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("ATTACHED_STYLE_MODE", mode)
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
    openrust::support::facades::Route::get("/attached-style", response)?;
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
            "components:attached-style",
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
async fn actual_http_attached_styles_keep_property_updates_local() {
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
            .get(format!("http://{address}/attached-style?mode={mode}"))
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
                .get(format!("http://{address}/attached-style?mode={mode}"))
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
