use openrust::{
    console::{ConsoleError, Input, Output},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "dispatch:report {user} {--U|urgent} {--id=*} {--queue=default}",
        |input: &Input, output: &mut Output| {
            let info = json!({
                "user":input.argument("user"),
                "urgent":input.option("urgent"),
                "id":input.option("id").unwrap_or_else(||json!([])),
                "queue":input.option("queue"),
            });
            input.io(output).line(&info.to_string())?;
            Ok(0)
        },
    )?;
    Ok(())
}

fn app(root: &std::path::Path) -> Application {
    Application::configure(root.into())
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap()
}

#[tokio::test]
async fn named_and_line_call_actual_commands_with_fixed_arguments() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/artisan-call-arguments-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = app(root.path());
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        for row in &rows {
            let mut output = Output::default();
            let result = if row["type"] == "named" {
                let data = row["input"].as_object().unwrap();
                let parameters = data
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.clone()))
                    .collect::<Vec<_>>();
                Artisan::call_named("dispatch:report", &parameters, &mut output).await
            } else {
                Artisan::call_line(row["input"].as_str().unwrap(), &mut output).await
            };
            assert_eq!(
                result.as_ref().ok().copied(),
                row["status"].as_i64().map(|v| v as i32),
                "{row}: {result:?}"
            );
            let data: Value = if output.stdout.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&output.stdout).unwrap()
            };
            assert_eq!(data, row["out"], "{row}");
        }
        for (input, expected) in [
            ("dispatch:report \"\\x41\\101\\n\"", "AA\n"),
            ("dispatch:report \"\\xc3\\xa9\"", "é"),
            ("dispatch:report alpha\" beta\"", "alpha beta"),
        ] {
            let mut output = Output::default();
            assert_eq!(Artisan::call_line(input, &mut output).await.unwrap(), 0);
            let data: Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(data["user"], expected, "{input}");
        }
        let mut output = Output::default();
        assert!(Artisan::call_line(" \t ", &mut output).await.is_err());
        assert!(
            Artisan::call_named(
                "dispatch:report",
                &[
                    ("user", json!("Ada")),
                    ("-U", json!(true)),
                    ("--urgent", json!(true))
                ],
                &mut output
            )
            .await
            .is_err()
        );
        assert!(output.stdout.is_empty());
    })
    .await;
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/named", || async {
        let mut output = Output::default();
        let status = Artisan::call_named(
            "dispatch:report",
            &[
                ("user", json!("Ada Lovelace")),
                ("--urgent", json!(false)),
                ("--id", json!([5, 13])),
            ],
            &mut output,
        )
        .await
        .map_err(|error| openrust::http::HttpError::Internal {
            source: Box::new(error),
        })?;
        let args: Value = serde_json::from_str(&output.stdout).unwrap();
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"code":status,"output":args}),
        )
    })?;
    Route::get("/line", || async {
        let mut output = Output::default();
        let status = Artisan::call_line(
            "dispatch:report \"Ada Lovelace\" --urgent --id=5 --id=13",
            &mut output,
        )
        .await
        .map_err(|error| openrust::http::HttpError::Internal {
            source: Box::new(error),
        })?;
        let args: Value = serde_json::from_str(&output.stdout).unwrap();
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"code":status,"output":args}),
        )
    })?;
    Route::get("/invalid", || async {
        let result = Artisan::call_named(
            "dispatch:report",
            &[("--unknown", json!(true))],
            &mut Output::default(),
        )
        .await;
        let status = if result.is_err() {
            openrust::http::StatusCode::UNPROCESSABLE_ENTITY
        } else {
            openrust::http::StatusCode::OK
        };
        openrust::http::Response::json(status, json!({"ok":result.is_ok()}))
    })?;
    Ok(())
}

#[tokio::test]
async fn real_http_named_and_line_consumers_validate_and_keep_values() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(app(root.path()));
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
    let failure = client
        .get(format!("http://{address}/invalid"))
        .send()
        .await
        .unwrap();
    assert_eq!(failure.status(), 422);
    for (route, urgent) in [("named", false), ("line", true)] {
        let response = client
            .get(format!("http://{address}/{route}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(
            body,
            json!({"code":0,"output":{"user":"Ada Lovelace","urgent":urgent,"id":["5","13"],"queue":"default"}})
        );
    }
    server.abort();
    let _ = server.await;
}

#[test]
#[ignore = "由父消费者执行实际命令"]
fn programmatic_arguments_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(|| {
            commands()?;
            Artisan::command_with(
                "dispatch:outer {mode}",
                openrust::console::AsyncCommandCallback::new(|_, input, output| {
                    Box::pin(async move {
                        match input.argument("mode").as_ref().and_then(Value::as_str) {
                            Some("named") => {
                                Artisan::call_named(
                                    "dispatch:report",
                                    &[
                                        ("user", json!("Ada")),
                                        ("--id", json!([5, 13])),
                                        ("--urgent", json!(false)),
                                    ],
                                    output,
                                )
                                .await
                            }
                            Some("line") => {
                                Artisan::call_line(
                                    "dispatch:report \"Ada Lovelace\" -U --id=5 --id=13",
                                    output,
                                )
                                .await
                            }
                            _ => {
                                Artisan::call_named(
                                    "dispatch:report",
                                    &[("--unknown", json!(true))],
                                    output,
                                )
                                .await
                            }
                        }
                    })
                }),
            )?;
            Ok(())
        }))
        .create()
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let env = std::env::var("ARTISAN_ARGUMENTS_MODE").unwrap();
    let code = rt
        .block_on(app.handle_command(&["dispatch:outer".into(), env.into(), "--no-ansi".into()]))
        .unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}

#[test]
fn real_artisan_nested_named_and_line_arguments_propagate_status() {
    for (name, code, expected) in [
        ("named", 0, "Ada"),
        ("line", 0, "Ada Lovelace"),
        ("invalid", 1, "命令参数"),
    ] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "programmatic_arguments_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("ARTISAN_ARGUMENTS_MODE", name)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(code), "{result:?}");
        let contents = String::from_utf8_lossy(if code == 0 {
            &result.stdout
        } else {
            &result.stderr
        });
        assert!(contents.contains(expected), "{contents}");
    }
}
