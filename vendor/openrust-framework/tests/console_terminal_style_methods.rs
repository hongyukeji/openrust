use openrust::console::{
    AnsiColorMode, AsOutput as _, ConsoleError, Output, OutputFormat, Terminal,
    view::components::{
        BuiltinComponentViewRenderer, ComponentContext, ComponentViewRenderer as _, StyleArgument,
        StyleRepository, TerminalStyles,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

fn apply(styles: &mut TerminalStyles<'_>, steps: &[Value]) -> Result<(), ConsoleError> {
    for encoded in steps {
        let values = encoded.as_array().unwrap();
        let method = values[0].as_str().unwrap();
        let text = |index: usize| values[index].as_str().unwrap();
        let number = |index: usize| values[index].as_i64().unwrap();
        let dimension = |index: usize| {
            values[index]
                .as_i64()
                .map_or_else(|| StyleArgument::Text(text(index)), StyleArgument::Integer)
        };
        match method {
            "setStyle" => styles.set_style(text(1)),
            "addStyle" => styles.add_style(text(1))?,
            "fontBold" => styles.font_bold(),
            "fontNormal" => styles.font_normal()?,
            "italic" => styles.italic()?,
            "underline" => styles.underline()?,
            "strong" => styles.strong()?,
            "lineThrough" => styles.line_through()?,
            "invisible" => styles.invisible()?,
            "uppercase" => styles.uppercase(),
            "lowercase" => styles.lowercase()?,
            "capitalize" => styles.capitalize()?,
            "snakecase" => styles.snakecase()?,
            "bg" => {
                if values.len() >= 3 {
                    styles.bg_with(text(1), number(2))?
                } else {
                    styles.bg(text(1))?
                }
            }
            "text" => {
                if values.len() > 2 {
                    styles.text_with(text(1), number(2))?
                } else {
                    styles.text(text(1))?
                }
            }
            "prepend" => styles.prepend(text(1))?,
            "append" => styles.append(text(1))?,
            "href" => styles.href(text(1))?,
            "contentRepeat" => styles.content_repeat(text(1))?,
            "truncate" => {
                if values.len() >= 2 {
                    styles.truncate_with(number(1), text(2))?
                } else {
                    styles.truncate()?
                }
            }
            "w" => styles.w(dimension(1))?,
            "minW" => styles.min_w(dimension(1))?,
            "maxW" => styles.max_w(dimension(1))?,
            "wFull" => styles.w_full()?,
            "wAuto" => styles.w_auto()?,
            "m" => styles.m(number(1))?,
            "ml" => styles.ml(number(1))?,
            "mr" => styles.mr(number(1))?,
            "mt" => styles.mt(number(1))?,
            "mb" => styles.mb(number(1))?,
            "mx" => styles.mx(number(1))?,
            "my" => styles.my(number(1))?,
            "p" => styles.p(number(1))?,
            "pl" => styles.pl(number(1))?,
            "pr" => styles.pr(number(1))?,
            "pt" => styles.pt(number(1))?,
            "pb" => styles.pb(number(1))?,
            "px" => styles.px(number(1))?,
            "py" => styles.py(number(1))?,
            "spaceX" => styles.space_x(number(1))?,
            "spaceY" => styles.space_y(number(1))?,
            "flex" => styles.flex()?,
            "block" => styles.block()?,
            "hidden" => styles.hidden()?,
            "justifyBetween" => styles.justify_between()?,
            "justifyAround" => styles.justify_around()?,
            "justifyEvenly" => styles.justify_evenly()?,
            "justifyCenter" => styles.justify_center()?,
            "borderT" => {
                if values.len() >= 2 {
                    styles.border_t_with(number(1))?
                } else {
                    styles.border_t()?
                }
            }
            "list" => {
                if values.len() >= 3 {
                    styles.list_with(text(1), number(2))?
                } else {
                    styles.list(text(1))?
                }
            }
            _ => panic!("无法识别固定样式入口：{method}"),
        };
    }
    Ok(())
}

#[test]
#[ignore = "由父测试隔离终端列宽和色彩模式"]
fn style_method_vectors_driver() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/terminal-style-methods-oracle.json"
    ))
    .unwrap();
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    Terminal::set_color_mode(Some(AnsiColorMode::Ansi4));
    let mut differences = Vec::new();
    for row in rows.into_iter().filter(|r| r["width"] == width) {
        let exact = row["has"].clone();
        let steps = row["methods"].as_array().unwrap().clone();
        let repository = StyleRepository::new();
        repository.create("brand-500").color("#9755aa").unwrap();
        repository.create_with("probe", move |styles, _| {
            apply(styles, &steps)?;
            for name in ["tested", "flex-1", "font-bold"] {
                assert_eq!(styles.has_style(name), exact[name], "{name}: {steps:?}");
            }
            Ok(())
        });
        let mut renderer = BuiltinComponentViewRenderer::with_styles(repository);
        for (format, key) in [(OutputFormat::Raw, "markup"), (OutputFormat::Normal, "out")] {
            let mut output = Output::default();
            output.set_decorated(row["ansi"] == true);
            let result = renderer.render(
                row["html"].as_str().unwrap(),
                format.into(),
                &mut ComponentContext::Output(output.as_output()),
            );
            if result.is_err() && !row["error"].is_null() {
                continue;
            }
            let expected = row[key].as_str();
            let actual = if key == "markup" {
                output.stdout.strip_suffix('\n').unwrap_or(&output.stdout)
            } else {
                &output.stdout
            };
            if result.is_err() || expected != Some(actual) {
                differences.push(json!({"case":row["case"],"key":key,"ansi":row["ansi"],"source":expected,"actual":actual,"source_error":row["error"],"error":result.err().map(|e|e.to_string())}));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "{}",
        serde_json::to_string_pretty(&differences).unwrap()
    );
}

#[test]
fn direct_style_methods_use_fixed_output_and_marker_semantics() {
    for width in [24, 40] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "style_method_vectors_driver",
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
        "components:typed-style {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let styles = StyleRepository::new();
                styles.create_with("banner", move |style, _| {
                    match mode.as_str() {
                        "invalid" => style.text("unknown")?,
                        "width" => style.w(StyleArgument::Text("1/0"))?,
                        "two" => style.prepend("[second]")?,
                        _ => style.prepend("[first]")?.uppercase(),
                    };
                    Ok(())
                });
                let mut renderer = BuiltinComponentViewRenderer::with_styles(styles);
                renderer.render(
                    "<div class='banner'>verified</div>",
                    OutputFormat::Raw.into(),
                    &mut ComponentContext::Output(output.as_output()),
                )?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn typed_styles_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:typed-style".into(),
        std::env::var("TYPED_STYLES_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_typed_style_methods_and_invalid_args() {
    for (mode, code, message) in [
        ("one", 0, "[FIRST]VERIFIED"),
        ("two", 0, "[second]verified"),
        ("invalid", 1, "unknown"),
        ("width", 1, "分母不能为零"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "typed_styles_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TYPED_STYLES_MODE", mode)
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
    openrust::support::facades::Route::get("/typed-style", response)?;
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
            "components:typed-style",
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
async fn actual_http_typed_style_commands_recover_and_keep_request_state() {
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
            .get(format!("http://{address}/typed-style?mode={mode}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
    }
    let mut calls = tokio::task::JoinSet::new();
    for index in 0..12 {
        let client = client.clone();
        let (mode, expected) = if index % 2 == 0 {
            ("one", "[FIRST]VERIFIED")
        } else {
            ("two", "[second]verified")
        };
        calls.spawn(async move {
            let response = client
                .get(format!("http://{address}/typed-style?mode={mode}"))
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
