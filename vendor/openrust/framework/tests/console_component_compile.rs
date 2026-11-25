use openrust::console::{
    AsOutput as _, ConsoleError, Output, OutputFormat,
    view::components::{
        BuiltinComponentViewCompiler, Component, ComponentContext, ComponentViewCompiler,
    },
};
use serde_json::{Value, json};
#[test]
fn owned_component_compile_matches_fixed_source_templates_entities_whitespace_and_failures() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/component-compile-oracle.json"
    ))
    .unwrap();
    for case in &cases {
        let mut output = Output::default();
        let mut component = Component::new(&mut output);
        let result = component.compile(case["view"].as_str().unwrap(), case["data"].clone());
        assert_eq!(
            result.is_err(),
            !case["error"].is_null(),
            "{case}: {result:?}"
        );
        if let Ok(html) = result {
            assert_eq!(html, case["answer"], "{case}");
        }
        assert!(output.stdout.is_empty(), "{case}");
        assert!(output.stderr.is_empty());
    }
}
struct CustomCompiler {
    index: usize,
}
impl ComponentViewCompiler for CustomCompiler {
    fn compile(
        &mut self,
        view: &str,
        mut data: Value,
        output: &mut ComponentContext<'_>,
    ) -> Result<String, ConsoleError> {
        self.index += 1;
        output.as_output().writeln(
            format!("compile:{view}:{}", self.index).as_str(),
            OutputFormat::Raw,
        )?;
        if view == "fail" {
            return Err(ConsoleError::new("编译失败"));
        }
        if view == "parent" {
            data["content"] = json!("Parent");
            return BuiltinComponentViewCompiler::new().compile("alert", data, output);
        }
        Ok(format!("<strong>{view}:{}</strong>", self.index))
    }
}
#[test]
fn custom_compiler_keeps_instance_state_original_output_and_default_fallback() {
    let mut output = Output::default();
    let mut component = Component::new(&mut output);
    component.set_view_compiler(Some(Box::new(CustomCompiler { index: 0 })));
    assert_eq!(
        component.compile("custom", json!({})).unwrap(),
        "<strong>custom:1</strong>"
    );
    assert!(
        component
            .compile("fail", json!({}))
            .unwrap_err()
            .to_string()
            .contains("编译失败")
    );
    assert!(
        component
            .compile("parent", json!({}))
            .unwrap()
            .contains("Parent")
    );
    component.set_view_compiler(None);
    assert!(
        component
            .compile("alert", json!({"content":"Default"}))
            .unwrap()
            .contains("Default")
    );
    assert_eq!(
        output.stdout,
        "compile:custom:1\ncompile:fail:2\ncompile:parent:3\n"
    );
}
#[test]
fn data_cannot_overwrite_template_selector_and_unknown_views_return_errors_without_output() {
    let mut output = Output::default();
    let mut component = Component::new(&mut output);
    let html = component
        .compile(
            "alert",
            json!({"content":"Expected","view":"missing","this":"ignored","data":"ignored"}),
        )
        .unwrap();
    assert!(html.contains("Expected"));
    for view in ["missing", "../line", "../../app", "line.php", ""] {
        assert!(component.compile(view, json!({})).is_err(), "{view}");
    }
    assert!(output.stdout.is_empty());
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "component:compile {view=alert} {--custom}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let view = input
                    .argument("view")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let custom = input.option("custom").and_then(|v| v.as_bool()) == Some(true);
                let mut component = Component::new(output);
                if custom {
                    component.set_view_compiler(Some(Box::new(CustomCompiler { index: 0 })));
                }
                let data = json!({"content":"Compiled [item] <b>&","marginTop":1,"bgColor":"blue","fgColor":"white","title":"info","elements":["One","Two"],"first":"Key","second":"Value"});
                let html = component.compile(&view, data)?;
                component
                    .get_output()
                    .writeln(format!("HTML:{}", json!(html)).as_str(), OutputFormat::Raw)?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行实际Artisan并核对退出码"]
fn compile_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let mut args: Vec<std::ffi::OsString> = vec![
        "component:compile".into(),
        std::env::var("COMPILE_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    if std::env::var_os("COMPILE_CUSTOM").is_some() {
        args.push("--custom".into());
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_compiles_builtins_custom_views_and_reports_missing_views() {
    for (view, custom, code) in [
        ("alert", false, 0),
        ("missing", false, 1),
        ("custom", true, 0),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "compile_cli_driver", "--ignored", "--nocapture"])
            .env("COMPILE_VIEW", view);
        if custom {
            command.env("COMPILE_CUSTOM", "1");
        }
        let result = command.output().unwrap();
        assert_eq!(result.status.code(), Some(code), "{result:?}");
        let text =
            String::from_utf8(result.stdout).unwrap() + &String::from_utf8(result.stderr).unwrap();
        if code == 0 {
            assert!(text.contains("HTML:"), "{text}");
            if custom {
                assert!(text.contains("<strong>custom:1</strong>"), "{text}");
            } else {
                assert!(text.contains("&lt;b&gt;&amp;"), "{text}");
            }
        } else {
            assert!(text.contains("未找到控制台组件视图"), "{text}");
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/compile", response)?;
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
            openrust::support::facades::Artisan::call("component:compile", &args, &mut output)
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
#[tokio::test]
async fn actual_http_compilation_and_custom_hooks_recover_and_remain_request_local() {
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
        let response = reqwest::get(format!("http://{address}/compile?view={view}"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert!(body["output"].as_str().unwrap().contains("HTML:"), "{body}");
    }
    let response = reqwest::get(format!("http://{address}/compile?view=missing"))
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..12 {
        requests.spawn(async move {
            let response = reqwest::get(format!("http://{address}/compile?view=custom&custom=1"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert!(
                body["output"]
                    .as_str()
                    .unwrap()
                    .contains("<strong>custom:1</strong>"),
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
