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
fn palette_vectors_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/component-palette-oracle.json"
    ))
    .unwrap();
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    let mode = std::env::var("PALETTE_MODE").unwrap();
    Terminal::set_color_mode(Some(match mode.as_str() {
        "Ansi8" => AnsiColorMode::Ansi8,
        "Ansi24" => AnsiColorMode::Ansi24,
        _ => AnsiColorMode::Ansi4,
    }));
    let mut differences = vec![];
    for (index, case) in cases
        .iter()
        .enumerate()
        .filter(|(_, c)| c["width"] == width && c["mode"] == mode)
    {
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
    assert!(
        differences.is_empty(),
        "{}",
        serde_json::to_string_pretty(&differences).unwrap()
    );
}
#[test]
fn fixed_palette_variants_breakpoints_order_original_style_tokens_and_output_modes() {
    for width in [20, 63, 64, 75, 76, 80, 101, 102, 127, 128, 152, 153] {
        for mode in if width == 80 {
            vec!["Ansi4", "Ansi8", "Ansi24"]
        } else {
            vec!["Ansi4"]
        } {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "palette_vectors_driver",
                    "--ignored",
                    "--nocapture",
                ])
                .env("COLUMNS", width.to_string())
                .env("PALETTE_MODE", mode)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{width} {mode}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "components:palette {view=colors}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let view = input
                    .argument("view")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let html = match view.as_str() {
                    "colors" => "<span class='bg-slate-900 text-amber-200'>Palette</span>",
                    "responsive" => {
                        "<span class='text-red sm:text-blue md:text-green'>Responsive</span>"
                    }
                    "invalid" => "<span class='text-missing'>Invalid</span>",
                    "inactive" => "<span class='2xl:not-a-style'>Inactive</span>",
                    _ => return Err(ConsoleError::new("未注册的验收视图")),
                };
                let mut component = Component::new(output);
                component.set_view_compiler(Some(Box::new(Markup)));
                component.render_view_with(html, json!({}), OutputFormat::Raw)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者运行真实Artisan与退出码"]
fn palette_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        "components:palette".into(),
        std::env::var("PALETTE_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_palette_conditional_styles_failure_and_recovery() {
    for (view, code, expected) in [
        ("colors", 0, "<bg=#0f172a;fg=#fde68a>Palette</>"),
        ("invalid", 1, "控制台颜色不存在"),
        ("inactive", 0, "Inactive"),
        ("responsive", 0, "<fg=green>Responsive</>"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "palette_cli_driver", "--ignored", "--nocapture"])
            .env("COLUMNS", "80")
            .env("PALETTE_VIEW", view)
            .output()
            .unwrap();
        assert_eq!(child.status.code(), Some(code), "{child:?}");
        let output = String::from_utf8_lossy(&child.stdout).into_owned()
            + &String::from_utf8_lossy(&child.stderr);
        assert!(output.contains(expected), "{output}");
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/palette", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let view = request
            .query(Some("view"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "colors".into());
        let mut output = Output::default();
        let result = openrust::support::facades::Artisan::call(
            "components:palette",
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
fn real_http_palette_requests_recover_from_bad_color_and_stay_independent() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "palette_http_driver", "--ignored", "--nocapture"])
        .env("COLUMNS", "80")
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
#[ignore = "由父消费者隔离终端列宽并运行真实HTTP"]
async fn palette_http_driver() {
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
        .get(format!("http://{address}/palette?view=invalid"))
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);
    for (view, expected) in [
        ("responsive", "<fg=green>Responsive</>\n"),
        ("inactive", "Inactive\n"),
    ] {
        let response = client
            .get(format!("http://{address}/palette?view={view}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["output"], expected);
    }
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let client = client.clone();
        requests.spawn(async move {
            let response = client
                .get(format!("http://{address}/palette?view=colors"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["output"], "<bg=#0f172a;fg=#fde68a>Palette</>\n");
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}
