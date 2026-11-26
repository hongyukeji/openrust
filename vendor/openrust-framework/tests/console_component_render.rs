use openrust::console::{
    AsOutput as _, ConsoleError, Output, OutputFormat, OutputOptions,
    view::components::{
        BuiltinComponentViewRenderer, Component, ComponentContext, ComponentViewCompiler,
        ComponentViewRenderer,
    },
};
use serde_json::{Value, json};
#[test]
#[ignore = "由父消费者隔离终端尺寸和颜色环境"]
fn render_vectors_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/component-render-oracle.json"
    ))
    .unwrap();
    let width = std::env::var("COLUMNS").unwrap().parse::<u64>().unwrap();
    let ansi = std::env::var("RENDER_ANSI").unwrap() == "1";
    for (index, case) in cases
        .iter()
        .enumerate()
        .filter(|(_, c)| c["width"] == width && c["ansi"] == ansi)
    {
        let mut output = Output::default();
        output.set_decorated(ansi);
        let result = Component::new(&mut output)
            .render_view(case["view"].as_str().unwrap(), case["data"].clone());
        assert!(result.is_ok(), "{index}: {case}: {result:?}");
        assert_eq!(output.stdout, case["output"], "case {index}: {case}");
    }
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/component-render-markup-oracle.json"
    ))
    .unwrap();
    let mut differences = vec![];
    for (index, case) in cases
        .iter()
        .enumerate()
        .filter(|(_, c)| c["width"] == width && c["ansi"] == ansi)
    {
        let mut output = Output::default();
        output.set_decorated(ansi);
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
        if (case["error"].is_null() && (result.is_err() || output.stdout != case["output"]))
            || (!case["error"].is_null() && result.is_ok())
        {
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
fn templates_render_through_actual_terminal_markup_in_six_isolated_environments() {
    for width in [20, 80, 160] {
        for ansi in [false, true] {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "render_vectors_driver",
                    "--ignored",
                    "--nocapture",
                ])
                .env("COLUMNS", width.to_string())
                .env("RENDER_ANSI", if ansi { "1" } else { "0" })
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{width} {ansi}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
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
struct Renderer(usize);
impl ComponentViewRenderer for Renderer {
    fn render(
        &mut self,
        html: &str,
        options: OutputOptions,
        output: &mut ComponentContext<'_>,
    ) -> Result<(), ConsoleError> {
        self.0 += 1;
        output
            .as_output()
            .writeln(format!("render:{}:{html}", self.0).as_str(), options)?;
        if html == "fail" {
            return Err(ConsoleError::new("渲染失败"));
        }
        BuiltinComponentViewRenderer::new().render("<strong>Parent</strong>", options, output)
    }
}
#[test]
fn instance_renderer_receives_actual_compile_result_original_output_options_and_recovers() {
    let mut output = Output::default();
    let mut component = Component::new(&mut output);
    component.set_view_renderer(Some(Box::new(Renderer(0))));
    assert!(component.render_view("missing", json!({})).is_err());
    component.set_view_compiler(Some(Box::new(Markup)));
    component
        .render_view_with("<b>Custom</b>", json!({}), OutputFormat::Raw)
        .unwrap();
    assert!(component.render_view("fail", json!({})).is_err());
    component.render_view("Again", json!({})).unwrap();
    component.set_view_renderer(None);
    component
        .render_view("<span class='mx-2'>Default</span>", json!({}))
        .unwrap();
    assert_eq!(
        output.stdout,
        "render:1:<b>Custom</b>\n\x1b[1mParent\x1b[0m\nrender:2:fail\nrender:3:Again\n\x1b[1mParent\x1b[0m\n  Default  \n"
    );
}
#[test]
fn unsupported_markup_fails_instead_of_silently_losing_layout_and_compile_errors_do_not_render() {
    let mut output = Output::default();
    let mut component = Component::new(&mut output);
    assert!(component.render_view("missing", json!({})).is_err());
    component.set_view_compiler(Some(Box::new(Markup)));
    for html in [
        "<div class='not-a-style'>X</div>",
        "<pre class='not-a-style'>X</pre>",
        "<table style='unknown'><tr><td>X</td></tr></table>",
        "<span class='w-999999999'>X</span>",
    ] {
        assert!(component.render_view(html, json!({})).is_err(), "{html}");
    }
    component
        .render_view("<span>Recovered &amp; safe</span>", json!({}))
        .unwrap();
    assert_eq!(output.stdout, "Recovered & safe\n");
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "component:render {view=alert} {--custom}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let view = input
                    .argument("view")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let custom = input.option("custom").and_then(|v| v.as_bool()) == Some(true);
                if view == "stderr" {
                    let mut style = openrust::console::OutputStyle::new(input, output);
                    Component::new(&mut style.get_error_style())
                        .render_view("bullet-list", json!({"elements":["Error"]}))?;
                    return Ok(0);
                }
                let mut component = Component::new(output);
                if custom {
                    component.set_view_compiler(Some(Box::new(Markup)));
                    component.set_view_renderer(Some(Box::new(Renderer(0))));
                }
                let data = json!({"content":"Compiled [item] <b>&","marginTop":1,"bgColor":"blue","fgColor":"white","title":"info","elements":["One","Two"],"first":"Key","second":"Value"});
                component.render_view(&view, data)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行实际Artisan并核对退出码"]
fn render_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let mut args: Vec<std::ffi::OsString> = vec![
        "component:render".into(),
        std::env::var("RENDER_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    if std::env::var_os("RENDER_CUSTOM").is_some() {
        args.push("--custom".into());
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_renders_builtins_custom_views_and_reports_missing_views() {
    for (view, custom, code) in [
        ("alert", false, 0),
        ("missing", false, 1),
        ("custom", true, 0),
        ("fail", true, 1),
        ("stderr", false, 0),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "render_cli_driver", "--ignored", "--nocapture"])
            .env("RENDER_VIEW", view);
        if custom {
            command.env("RENDER_CUSTOM", "1");
        }
        let result = command.output().unwrap();
        assert_eq!(result.status.code(), Some(code), "{result:?}");
        if view == "stderr" {
            assert!(!String::from_utf8_lossy(&result.stdout).contains("⇂ Error"));
            assert!(String::from_utf8_lossy(&result.stderr).contains("⇂ Error"));
        }
        let text =
            String::from_utf8(result.stdout).unwrap() + &String::from_utf8(result.stderr).unwrap();
        if code == 0 {
            assert!(!text.contains("HTML:"), "{text}");
            if view == "stderr" {
                assert!(!text.contains("Compiled"), "{text}");
                assert!(text.contains("⇂ Error"), "{text}");
            } else if custom {
                assert!(text.contains("custom"), "{text}");
            } else {
                assert!(
                    text.contains("CREATED [ITEM] <B>&") || text.contains("COMPILED [ITEM] <B>&"),
                    "{text}"
                );
            }
        } else {
            assert!(
                text.contains(if custom {
                    "渲染失败"
                } else {
                    "未找到控制台组件视图"
                }),
                "{text}"
            );
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/render", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let view = request
            .query(Some("view"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "alert".into());
        let mut args = vec![view.into()];
        if request.query(Some("custom"))?.is_some() {
            args.push("--custom".into());
        }
        let mut output = Output::default();
        let result =
            openrust::support::facades::Artisan::call("component:render", &args, &mut output).await;
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
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_rendering_and_custom_hooks_recover_and_remain_request_local() {
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
    for view in ["alert", "line", "bullet-list", "two-column-detail"] {
        let response = reqwest::get(format!("http://{address}/render?view={view}"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert!(!body["output"].as_str().unwrap().is_empty(), "{body}");
    }
    let response = reqwest::get(format!("http://{address}/render?view=missing"))
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    let response = reqwest::get(format!("http://{address}/render?view=fail&custom=1"))
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["output"].as_str().unwrap().contains("render:1:fail"),
        "{body}"
    );
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..12 {
        requests.spawn(async move {
            let response = reqwest::get(format!("http://{address}/render?view=custom&custom=1"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert!(
                body["output"].as_str().unwrap().contains("render:1:custom"),
                "{body}"
            );
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    server.abort();
    let _ = server.await;
}

#[test]
fn component_rendering_preserves_section_error_style_and_quiet_filtering() {
    let mut output = Output::default();
    output.set_decorated(true);
    {
        let mut section = output.section();
        Component::new(&mut section)
            .render_view_with(
                "bullet-list",
                json!({"elements":["Section"]}),
                OutputFormat::Plain,
            )
            .unwrap();
        let expected: Value = serde_json::from_str(include_str!(
            "../../../.agents/project/audits/artisan-tutorial/component-render-section-oracle.json"
        ))
        .unwrap();
        assert_eq!(section.get_content(), expected["content"]);
    }
    output.set_decorated(false);
    let input = openrust::console::CommandDefinition::from_signature("render:test")
        .unwrap()
        .parse(&[])
        .unwrap();
    {
        let mut style = openrust::console::OutputStyle::new(&input, &mut output);
        let mut errors = style.get_error_style();
        Component::new(&mut errors)
            .render_view("bullet-list", json!({"elements":["Error"]}))
            .unwrap();
    }
    let expected: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/component-render-section-oracle.json"
    ))
    .unwrap();
    assert!(
        output
            .stdout
            .ends_with(expected["buffered_error_output"].as_str().unwrap())
    );
    assert!(output.stderr.is_empty());
    let mut quiet = Output::default();
    quiet.set_verbosity(openrust::console::OutputVerbosity::Quiet);
    let mut component = Component::new(&mut quiet);
    component
        .render_view("alert", json!({"content":"Quiet"}))
        .unwrap();
    assert!(component.render_view("missing", json!({})).is_err());
    assert!(quiet.stdout.is_empty());
}
