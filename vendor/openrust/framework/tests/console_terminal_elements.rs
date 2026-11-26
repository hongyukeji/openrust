use openrust::console::{
    ConsoleError, Output, OutputFormat,
    view::components::{TerminalChild, TerminalElement, TerminalElementKind},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/terminal-element-oracle.json"
    ))
    .unwrap()
}

#[test]
fn independent_element_content_replacement_shares_styles_and_outputs() {
    use TerminalElementKind::*;
    let source = oracle();
    let mut origin = TerminalElement::new(Span, "origin");
    assert_eq!(origin.to_markup().unwrap(), source["constructor"]);
    let mut copy = origin.set_content("copied");
    copy.configure(|style| {
        style.font_bold();
        Ok(())
    })
    .unwrap();
    assert_eq!(origin.to_markup().unwrap(), source["shared_font_origin"]);
    assert_eq!(copy.to_markup().unwrap(), source["shared_font_copy"]);
    assert_eq!(
        origin
            .configure(|style| Ok(style.has_style("theme")))
            .unwrap(),
        source["has_marker_before"]
    );
    origin
        .configure(|style| {
            style.set_style("theme");
            Ok(())
        })
        .unwrap();
    assert_eq!(
        copy.configure(|style| Ok(style.has_style("theme")))
            .unwrap(),
        source["has_marker_after"]
    );
    let mut output = Output::default();
    copy.render(&mut output, OutputFormat::Normal).unwrap();
    copy.render(&mut output, OutputFormat::Raw).unwrap();
    assert_eq!(output.stdout, source["render_modes"]);
    assert!(origin.configure(|_| copy.to_markup()).is_err());
    assert_eq!(origin.to_markup().unwrap(), source["shared_font_origin"]);
}

#[test]
fn nested_terminal_elements_preserve_inheritance_cached_join_and_default_classes() {
    use TerminalElementKind::*;
    let source = oracle();
    let child = TerminalElement::from_styles(Span, "world", "font-bold").unwrap();
    let parts = vec![TerminalChild::from("hello "), child.into(), "!".into()];
    let mut parent = TerminalElement::from_styles(Div, parts, "text-cyan ml-2").unwrap();
    assert_eq!(parent.to_markup().unwrap(), source["styled_children"]);
    assert_eq!(parent.to_markup().unwrap(), source["styled_children_cache"]);
    assert_eq!(
        serde_json::to_value(parent.default_styles().collect::<Vec<_>>()).unwrap(),
        source["default_styles"]
    );
    assert_eq!(
        TerminalElement::new(Div, "plain").to_markup().unwrap(),
        source["default_unresolved"]
    );
    let styled = TerminalElement::from_styles(Span, "raw", "font-bold").unwrap();
    let mut raw = TerminalElement::new(Raw, vec![TerminalChild::from("tool: "), styled.into()]);
    assert_eq!(raw.to_markup().unwrap(), "tool: <options=bold>raw</>");
    let mut line = TerminalElement::from_styles(BreakLine, "", "").unwrap();
    assert_eq!(line.to_markup().unwrap(), "\r");
}

#[test]
#[ignore = "由父测试隔离终端列宽和色彩模式"]
fn terminal_element_vectors_driver() {
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    openrust::console::Terminal::set_color_mode(Some(openrust::console::AnsiColorMode::Ansi4));
    let mut diffs = Vec::new();
    for row in oracle()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["width"] == width)
    {
        use TerminalElementKind::*;
        let kind = match row["kind"].as_str().unwrap() {
            "raw" => Raw,
            "span" => Span,
            "div" => Div,
            "paragraph" => Paragraph,
            "anchor" => Anchor,
            "break-line" => BreakLine,
            "rule" => Rule,
            "list-item" => ListItem,
            "ordered-list" => OrderedList,
            "unordered-list" => UnorderedList,
            "definition-list" => DefinitionList,
            "definition-term" => DefinitionTerm,
            "definition-description" => DefinitionDescription,
            kind => panic!("未知终端元素：{kind}"),
        };
        let text = row["text"].as_str().unwrap();
        let element = if row["apply"] == true {
            TerminalElement::from_styles(kind, text, row["class"].as_str().unwrap())
        } else {
            Ok(TerminalElement::new(kind, text))
        };
        let mut output = Output::default();
        output.set_decorated(row["ansi"] == true);
        let markup = element.and_then(|mut value| {
            let markup = value.to_markup()?;
            value.render(&mut output, OutputFormat::Normal)?;
            value.render(&mut output, OutputFormat::Raw)?;
            Ok(markup)
        });
        match markup {
            Ok(text) if row["markup"] == text && row["out"] == output.stdout => (),
            Err(_) if !row["error"].is_null() => (),
            other => diffs.push(
                serde_json::json!({"source":row,"actual":format!("{other:?}"),"out":output.stdout}),
            ),
        }
    }
    assert!(
        diffs.is_empty(),
        "{}",
        serde_json::to_string_pretty(&diffs).unwrap()
    );
}

#[test]
fn typed_elements_match_fixed_constructor_render_and_error_semantics() {
    for width in [20, 40] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_element_vectors_driver",
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
        "components:typed-element {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let prefix = TerminalElement::from_styles(
                    TerminalElementKind::Span,
                    "verified",
                    "font-bold",
                )?;
                let content = vec![
                    TerminalChild::from(prefix),
                    TerminalChild::from(match mode.as_str() {
                        "two" => " second",
                        _ => " first",
                    }),
                ];
                let classes = match mode.as_str() {
                    "invalid" => "text-unknown",
                    "width" => "w-1/0",
                    _ => "text-cyan",
                };
                let mut element =
                    TerminalElement::from_styles(TerminalElementKind::Div, content, classes)?;
                element.render(output, OutputFormat::Normal)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn typed_element_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:typed-element".into(),
        std::env::var("TYPED_ELEMENT_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_typed_elements_and_invalid_width() {
    for (mode, code, message) in [
        ("one", 0, "verified first"),
        ("two", 0, "verified second"),
        ("invalid", 1, "unknown"),
        ("width", 1, "分母不能为零"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "typed_element_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TYPED_ELEMENT_MODE", mode)
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
    openrust::support::facades::Route::get("/typed-element", response)?;
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
            "components:typed-element",
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
async fn actual_http_direct_elements_preserve_nested_content_after_failed_request() {
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
            .get(format!("http://{address}/typed-element?mode={mode}"))
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
                .get(format!("http://{address}/typed-element?mode={mode}"))
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
