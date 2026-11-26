use openrust::console::{
    AnsiColorMode, ConsoleError, Output, OutputFormat, Terminal,
    view::components::{
        BuiltinComponentViewRenderer, Component, ComponentContext, ComponentViewCompiler,
        StyleArgument, StyleRepository, TerminalStyle,
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

struct Markup;

impl ComponentViewCompiler for Markup {
    fn compile(
        &mut self,
        view: &str,
        _: Value,
        _: &mut ComponentContext<'_>,
    ) -> Result<String, ConsoleError> {
        Ok(view.to_owned())
    }
}

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/style-repository-oracle.json"
    ))
    .unwrap()
}

fn render(
    styles: &StyleRepository,
    content: &str,
    decorated: bool,
    format: OutputFormat,
) -> Result<String, ConsoleError> {
    let mut output = Output::default();
    output.set_decorated(decorated);
    let mut component = Component::new(&mut output);
    component.set_view_compiler(Some(Box::new(Markup)));
    component.set_view_renderer(Some(Box::new(BuiltinComponentViewRenderer::with_styles(
        styles.clone(),
    ))));
    component.render_view_with(content, json!({}), format)?;
    Ok(output.stdout)
}

#[test]
#[ignore = "由父测试隔离列宽和色彩模式"]
fn style_repository_vectors_driver() {
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    Terminal::set_color_mode(Some(AnsiColorMode::Ansi4));
    let mut differences = Vec::new();
    for row in oracle()["render"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["width"] == width)
    {
        let styles = StyleRepository::new();
        styles
            .create_with("notice", |result, _| {
                result.uppercase();
                Ok(())
            })
            .apply("prepend-[!]");
        styles.create("wrap").apply("append-[?]");
        styles.create("replaced").apply("prepend-[obsolete]");
        styles.create("replaced").apply("append-[new]");
        if row["kind"] == "shadow" {
            styles.create("font-bold").apply("lowercase");
        }
        for (name, color) in [
            ("brand", "#aBc"),
            ("brand-500", "#123456"),
            ("red", "#234567"),
            ("red-500", "#345678"),
        ] {
            styles.create(name).color(color).unwrap();
        }
        styles.create("empty");
        styles.create("sm:banner").apply("append-[custom]");
        styles.create("banner").apply("append-[unscoped]");
        let args = Arc::new(Mutex::new(Value::Null));
        let captured = args.clone();
        styles.create_with("receive", move |result, values| {
            *captured.lock().unwrap() = Value::from(values.len());
            result.font_bold();
            Ok(())
        });
        let html = row["html"].as_str().unwrap();
        for (format, key) in [(OutputFormat::Raw, "markup"), (OutputFormat::Normal, "out")] {
            let result = render(&styles, html, row["ansi"] == true, format);
            match (row["error"].is_null(), result) {
                (false, Err(_)) => (),
                (true, Ok(value)) => {
                    let expected = row[key].as_str().unwrap();
                    let actual = if key == "markup" { value.strip_suffix('\n').unwrap_or(&value) } else { &value };
                    if expected != actual {
                        differences.push(json!({"kind":row["kind"],"expected":expected,"actual":actual,"key":key,"ansi":row["ansi"]}));
                    }
                }
                (expected, actual) => differences.push(json!({"row":row,"key":key,"expected_success":expected,"actual":format!("{actual:?}")})),
            }
        }
        if row["kind"] == "array-callback" {
            assert_eq!(
                *args.lock().unwrap(),
                row["callback_arguments"].as_array().unwrap().len()
            );
        }
    }
    assert!(
        differences.is_empty(),
        "{}",
        serde_json::to_string_pretty(&differences).unwrap()
    );
}

#[test]
fn custom_colors_callbacks_and_media_match_fixed_rendering() {
    for width in [20, 82] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "style_repository_vectors_driver",
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
fn registry_handles_preserve_instance_identity_explicit_arguments_and_error_state() {
    let source = oracle();
    let styles = StyleRepository::new();
    let original = styles.create("color");
    for row in source["colors"].as_array().unwrap() {
        let result = original.color(row["value"].as_str().unwrap());
        assert_eq!(result.is_ok(), row["error"].is_null());
        assert_eq!(original.get_color(), row["stored"]);
    }
    let retrieved = styles.get("color").unwrap();
    let replacement = styles.create("color");
    retrieved.color("#456").unwrap();
    assert_eq!(original.get_color(), "#456");
    assert_eq!(
        replacement.get_color(),
        source["identities"]["replacement_color"]
    );
    styles.flush();
    assert!(!styles.has("color"));
    assert!(styles.get("color").is_err());
    assert_eq!(original.get_color(), "#456");

    let seen = Arc::new(Mutex::new(Value::Null));
    let captured = seen.clone();
    let style = TerminalStyle::new_with_color(
        move |styles, args| {
            *captured.lock().unwrap() = Value::Array(
                args.iter()
                    .map(|arg| match arg {
                        StyleArgument::Integer(value) => json!(value),
                        StyleArgument::Text(value) => json!(value),
                    })
                    .collect(),
            );
            styles.uppercase();
            Ok(())
        },
        "#f12",
    );
    style.apply("append-[?]");
    let copy = style.clone();
    styles.create_with("invoke", move |styles, _| {
        copy.invoke(
            styles,
            &[StyleArgument::Integer(7), StyleArgument::Text("alpha")],
        )
    });
    assert_eq!(
        render(
            &styles,
            "<span class='invoke'>hello</span>",
            false,
            OutputFormat::Raw
        )
        .unwrap()
        .strip_suffix('\n')
        .unwrap(),
        source["invocation"]["markup"]
    );
    assert_eq!(style.get_color(), source["invocation"]["color"]);
    assert_eq!(*seen.lock().unwrap(), source["invocation"]["args"]);
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:registered-style {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                use openrust::console::{
                    AsOutput as _, view::components::ComponentViewRenderer as _,
                };
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let styles = StyleRepository::new();
                let style = styles.create("banner");
                match mode.as_str() {
                    "invalid" => style.apply("unknown"),
                    "cycle" => style.apply("banner"),
                    "two" => style.apply("prepend-[second]"),
                    _ => style.apply("prepend-[first]"),
                }
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
fn repository_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:registered-style".into(),
        std::env::var("STYLE_REPOSITORY_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_style_registry_handles_success_invalid_style_and_cycle() {
    for (mode, code, message) in [
        ("one", 0, "[first]verified"),
        ("two", 0, "[second]verified"),
        ("invalid", 1, "unknown"),
        ("cycle", 1, "递归"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "repository_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("STYLE_REPOSITORY_MODE", mode)
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
    openrust::support::facades::Route::get("/registered-style", response)?;
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
            "components:registered-style",
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
async fn actual_http_parallel_registries_are_isolated_after_failure() {
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
    for mode in ["invalid", "cycle"] {
        let response = client
            .get(format!("http://{address}/registered-style?mode={mode}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
    }
    let mut calls = tokio::task::JoinSet::new();
    for index in 0..12 {
        let client = client.clone();
        let (mode, expected) = if index % 2 == 0 {
            ("one", "[first]verified")
        } else {
            ("two", "[second]verified")
        };
        calls.spawn(async move {
            let response = client
                .get(format!("http://{address}/registered-style?mode={mode}"))
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
