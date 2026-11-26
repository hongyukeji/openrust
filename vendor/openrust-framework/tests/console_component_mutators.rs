use openrust::{
    console::{
        ConsoleError,
        view::components::mutators::{
            EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsurePunctuation,
            EnsureRelativePaths, Mutator,
        },
    },
    contracts::foundation::Application as _,
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn pipeline(value: &str) -> Result<String, ConsoleError> {
    let mut value = value.to_owned();
    for mutator in [
        &EnsureDynamicContentIsHighlighted as &dyn Mutator,
        &EnsurePunctuation,
        &EnsureNoPunctuation,
        &EnsureRelativePaths,
    ] {
        value = mutator.apply(&value)?;
    }
    Ok(value)
}
fn application(path: &std::path::Path) -> Arc<Application> {
    Arc::new(Application::configure(path.into()).create().unwrap())
}
#[tokio::test]
async fn fixed_source_all_four_public_mutators_and_ordered_pipeline() {
    let root = tempfile::tempdir().unwrap();
    let app = application(root.path());
    let base = root.path().display().to_string();
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/component-mutators-oracle.json"
    ))
    .unwrap();
    for bound in [false, true] {
        if !bound {
            app.container().offset_unset("path.base");
        }
        let scoped = if bound {
            application(root.path())
        } else {
            app.clone()
        };
        App::set_facade_application(Some(&scoped), async {
            for case in cases.iter().filter(|c| c["bound"] == bound) {
                let input = case["input"].as_str().unwrap().replace("__BASE__", &base);
                for (name, mutator) in [
                    (
                        "EnsureDynamicContentIsHighlighted",
                        &EnsureDynamicContentIsHighlighted as &dyn Mutator,
                    ),
                    ("EnsurePunctuation", &EnsurePunctuation),
                    ("EnsureNoPunctuation", &EnsureNoPunctuation),
                    ("EnsureRelativePaths", &EnsureRelativePaths),
                ] {
                    assert_eq!(
                        mutator.apply(&input).unwrap(),
                        case["results"][name]
                            .as_str()
                            .unwrap()
                            .replace("__BASE__", &base),
                        "{case}: {name}"
                    );
                }
                assert_eq!(
                    pipeline(&input).unwrap(),
                    case["pipeline"]
                        .as_str()
                        .unwrap()
                        .replace("__BASE__", &base)
                );
            }
        })
        .await;
    }
    assert_eq!(
        EnsureRelativePaths.apply("__BASE__/x").unwrap(),
        "__BASE__/x"
    );
}
#[tokio::test]
async fn path_transform_uses_current_application_and_parallel_scopes_do_not_leak() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let a = application(first.path());
    let b = application(second.path());
    let input = format!("{}/a {}/b", first.path().display(), second.path().display());
    let mut tasks = vec![];
    for n in 0..24 {
        let app = if n % 2 == 0 { a.clone() } else { b.clone() };
        let input = input.clone();
        let expected = if n % 2 == 0 {
            format!("a {}/b", second.path().display())
        } else {
            format!("{}/a b", first.path().display())
        };
        tasks.push(tokio::spawn(async move {
            App::set_facade_application(Some(&app), async {
                tokio::task::yield_now().await;
                assert_eq!(EnsureRelativePaths.apply(&input).unwrap(), expected);
            })
            .await;
            assert_eq!(EnsureRelativePaths.apply(&input).unwrap(), input);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
}
#[tokio::test]
async fn broken_application_binding_propagates_instead_of_silently_hiding_error() {
    use openrust::contracts::container::Container as _;
    let root = tempfile::tempdir().unwrap();
    let app = application(root.path());
    app.container()
        .instance_named("app", Arc::new("wrong service".to_owned()))
        .unwrap();
    App::set_facade_application(Some(&app), async {
        assert!(EnsureRelativePaths.apply("unchanged").is_err());
    })
    .await;
    assert_eq!(EnsureRelativePaths.apply("unchanged").unwrap(), "unchanged");
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "components:mutate",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let app = App::get_facade_root().map_err(ConsoleError::from_source)?;
                let result =
                    pipeline(&format!("Created [{}/example].", app.base_path().display()))?;
                openrust::console::OutputStyle::new(input, output)
                    .writeln(result.as_str(), openrust::console::OutputOptions::default())?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/components", || async {
        let mut output = openrust::console::Output::default();
        Artisan::call("components:mutate", &[], &mut output)
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        openrust::http::Response::json(http::StatusCode::OK, json!({"output": output.stdout}))
    })?;
    Ok(())
}
#[test]
#[ignore = "由父测试执行真实Artisan子进程"]
fn component_mutators_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt
        .block_on(app.handle_command(&["components:mutate".into(), "--no-ansi".into()]))
        .unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn real_artisan_resolves_path_context_and_writes_output() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "component_mutators_cli_driver",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Created [example]\n"));
}
#[tokio::test]
async fn real_http_calls_artisan_with_component_mutators_and_application_context() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().web(routes).commands(commands))
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
    for _ in 0..2 {
        let result = reqwest::get(format!("http://{address}/components"))
            .await
            .unwrap();
        assert_eq!(result.status(), 200);
        assert_eq!(
            result.json::<Value>().await.unwrap(),
            json!({"output":"Created [example]\n"})
        );
    }
    server.abort();
    let _ = server.await;
}
