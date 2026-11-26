use openrust::console::{
    AnsiColorMode, ConsoleError, Output, OutputFormat, Terminal,
    view::components::{Component, ComponentContext, ComponentViewCompiler},
};
use serde_json::{Value, json};
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
#[test]
#[ignore = "由父消费者隔离终端尺寸、色彩模式和全局能力配置"]
fn sizing_vectors_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/component-sizing-oracle.json"
    ))
    .unwrap();
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    let mode = std::env::var("SIZING_MODE").unwrap();
    Terminal::set_color_mode(Some(match mode.as_str() {
        "Ansi8" => AnsiColorMode::Ansi8,
        "Ansi24" => AnsiColorMode::Ansi24,
        _ => AnsiColorMode::Ansi4,
    }));
    let mut differences = vec![];
    let mut visited = 0;
    for (index, case) in cases
        .iter()
        .enumerate()
        .filter(|(_, c)| c["width"] == width)
    {
        visited += 1;
        let mut output = Output::default();
        output.set_decorated(case["ansi"] == true);
        let mut component = Component::new(&mut output);
        component.set_view_compiler(Some(Box::new(Markup)));
        let result = component.render_view_with(
            case["html"].as_str().unwrap(),
            json!({}),
            if case["raw"] == true {
                OutputFormat::Raw
            } else {
                OutputFormat::Normal
            },
        );
        if result.is_err() == case["error"].is_null() || output.stdout != case["output"] {
            differences.push(json!({"index":index,"case":case,"actual":output.stdout,"error":result.err().map(|e|e.to_string())}));
        }
    }
    assert!(visited > 0, "尺寸测试不得跳过所有来源场景");
    println!("checked {visited} source sizing cases");
    assert!(
        differences.is_empty(),
        "{}",
        serde_json::to_string_pretty(&differences).unwrap()
    );
}
#[test]
fn fractional_widths_justification_and_cached_descendants_match_fixed_source() {
    for width in [20, 31, 80] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sizing_vectors_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("COLUMNS", width.to_string())
            .env("SIZING_MODE", "Ansi4")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{width}: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

fn markup(view: &str) -> Result<&'static str, ConsoleError> {
    match view {
        "half" => Ok("<span class=\"w-1/2\">Half</span>"),
        "between" => Ok("<div class=\"w-20 justify-between\"><span>A</span><span>B</span></div>"),
        "cache" => Ok(
            "<div class=\"flex w-20\"><div class=\"flex-1\"><span class=\"w-full\">X</span></div><span>Z</span></div>",
        ),
        "nested" => Ok("<div class=\"min-w-1/2\"><span class=\"w-full\">Nested</span></div>"),
        "invalid" => Ok("<span class=\"w-1/0\">Invalid</span>"),
        _ => Err(ConsoleError::new("未注册的尺寸验收视图")),
    }
}
fn expected(view: &str) -> Value {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/component-sizing-oracle.json"
    ))
    .unwrap();
    cases
        .into_iter()
        .find(|c| {
            c["html"] == markup(view).unwrap()
                && c["width"] == 20
                && c["ansi"] == false
                && c["raw"] == true
        })
        .expect("实际消费者必须引用固定来源同数据场景")
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:sizing {view=half}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let view = input
                    .argument("view")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let mut component = Component::new(output);
                component.set_view_compiler(Some(Box::new(Markup)));
                component.render_view_with(markup(&view)?, json!({}), OutputFormat::Raw)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者调用真实Artisan和退出码"]
fn sizing_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        "components:sizing".into(),
        std::env::var("SIZING_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_fractional_widths_cached_flex_and_invalid_denominator() {
    for view in ["half", "invalid", "between", "cache", "nested"] {
        let reference = expected(view);
        let code = if reference["error"].is_null() { 0 } else { 1 };
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sizing_cli_driver", "--ignored", "--nocapture"])
            .env("COLUMNS", "20")
            .env("SIZING_VIEW", view)
            .output()
            .unwrap();
        assert_eq!(child.status.code(), Some(code), "{child:?}");
        let output = String::from_utf8_lossy(&child.stdout).into_owned()
            + &String::from_utf8_lossy(&child.stderr);
        if code == 0 {
            assert!(
                output.contains(reference["output"].as_str().unwrap()),
                "{view}: {output}"
            );
        } else {
            assert!(output.contains("分母"), "{output}");
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/sizing", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let view = request
            .query(Some("view"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "half".into());
        let mut output = Output::default();
        let result = openrust::support::facades::Artisan::call(
            "components:sizing",
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
            json!({"output":output.stdout,"error":output.stderr}),
        )
    })
}
#[test]
fn actual_http_sizes_and_alignment_recover_from_failure_and_stay_request_local() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sizing_http_driver", "--ignored", "--nocapture"])
        .env("COLUMNS", "20")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "由父消费者隔离列宽并运行真实HTTP"]
async fn sizing_http_driver() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
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
    let failed = client
        .get(format!("http://{address}/sizing?view=invalid"))
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);
    for view in ["half", "between", "cache", "nested"] {
        let response = client
            .get(format!("http://{address}/sizing?view={view}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["output"], expected(view)["output"], "{view}: {body}");
    }
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let client = client.clone();
        requests.spawn(async move {
            let response = client
                .get(format!("http://{address}/sizing?view=cache"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["output"], expected("cache")["output"]);
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
