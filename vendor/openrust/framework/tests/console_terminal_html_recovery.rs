use openrust::console::{
    AnsiColorMode, ConsoleError, Output, OutputFormat, Terminal,
    view::components::{
        BuiltinComponentViewRenderer, Component, ComponentContext, ComponentViewCompiler,
        StyleRepository,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

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
        "../../../../.agents/project/audits/artisan-tutorial/terminal-html-recovery-oracle.json"
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
#[ignore = "由父测试隔离终端列宽"]
fn terminal_html_recovery_driver() {
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    Terminal::set_color_mode(Some(AnsiColorMode::Ansi4));
    let rows = oracle();
    let mut differences = Vec::new();
    let styles = StyleRepository::new();
    for row in rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["width"] == width)
    {
        let html = row["html"].as_str().unwrap();
        let ansi = row["ansi"].as_bool().unwrap();
        let raw = render(&styles, html, ansi, OutputFormat::Raw);
        let normal = render(&styles, html, ansi, OutputFormat::Normal);
        let matches = if row["error"].is_null() {
            normal.as_deref().ok() == row["out"].as_str()
                && raw
                    .as_deref()
                    .ok()
                    .map(|value| value.strip_suffix('\n').unwrap_or(value))
                    == row["markup"].as_str()
        } else {
            raw.is_err() && normal.is_err()
        };
        if !matches {
            differences.push(format!("{} / width={width}, ansi={ansi}: expected raw={}, formatted={}, error={}; received raw={raw:?}, formatted={normal:?}",
                row["kind"], row["markup"], row["out"], row["error"]));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn malformed_markup_recovery_matches_fixed_source() {
    for width in ["20", "80"] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_html_recovery_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("COLUMNS", width)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:html-recovery {case=one}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mode = input
                    .argument("case")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let html = match mode.as_str() {
                    "invalid" => "<!-- before --><span>later</span>",
                    "width" => "<!doctype html>",
                    "two" => "<div>a</p>b</div>",
                    _ => "<div><span class=\"text-red\"/>second</div>",
                };
                let mut component = Component::new(output);
                component.set_view_compiler(Some(Box::new(Markup)));
                component.render_view_with(html, json!({}), OutputFormat::Normal)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

#[test]
#[ignore = "由父测试执行真实 Artisan"]
fn terminal_html_recovery_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "components:html-recovery".into(),
        std::env::var("TERMINAL_HTML_RECOVERY_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_recovers_html_and_returns_empty_body_errors() {
    for (mode, code, message) in [
        ("one", 0, "second"),
        ("two", 0, "ab"),
        ("invalid", 1, "没有可渲染的首元素"),
        ("width", 1, "没有可渲染的首元素"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_html_recovery_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TERMINAL_HTML_RECOVERY_MODE", mode)
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
    openrust::support::facades::Route::get("/html-recovery", response)?;
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
            "components:html-recovery",
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
async fn actual_http_html_recovers_markup_after_invalid_fragments() {
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
            .get(format!("http://{address}/html-recovery?mode={mode}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
    }
    let mut calls = tokio::task::JoinSet::new();
    for index in 0..12 {
        let client = client.clone();
        let (mode, expected) = if index % 2 == 0 {
            ("one", "second")
        } else {
            ("two", "ab")
        };
        calls.spawn(async move {
            let response = client
                .get(format!("http://{address}/html-recovery?mode={mode}"))
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
