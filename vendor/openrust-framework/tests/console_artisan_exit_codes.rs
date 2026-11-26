use openrust::{
    console::{AsyncCommandCallback, ConsoleError, Input, Output, events::CommandFinished},
    contracts::events::{Dispatcher as _, Event, Listener},
    events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

fn stop(input: &Input) -> Result<(), ConsoleError> {
    input.fail("Stop this operation")
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command("exit:implicit", |out| out.stdout.push_str("finished\n"))?;
    Artisan::command_with(
        "exit:probe {mode=implicit}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut io = input.io(output);
                match io.argument("mode").as_ref().and_then(Value::as_str) {
                    Some("explicit") => return Ok(23),
                    Some("negative") => return Ok(-1),
                    Some("default") => return io.fail_default(),
                    Some("throwable") => {
                        return Err(ConsoleError::from_source(std::io::Error::other(
                            "outside manual failure",
                        )));
                    }
                    Some("nested") => {
                        io.line("before failure")?;
                        stop(input)?;
                    }
                    _ => {}
                }
                io.line("finished")?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

fn application(path: &std::path::Path) -> Application {
    Application::configure(path.into())
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap()
}

type Trace = Arc<Mutex<Vec<(String, i32, String)>>>;
struct Recorder(Trace);

impl Listener for Recorder {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event.downcast_ref::<CommandFinished>().unwrap();
            self.0.lock().unwrap().push((
                event.command.clone(),
                event.exit_code,
                event.output.clone(),
            ));
            Ok(None)
        })
    }
}

#[tokio::test]
async fn implicit_explicit_and_manual_failure_match_source_and_finish_events() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/artisan-exit-codes-oracle.json"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut app = application(root.path());
    app.boot().await.unwrap();
    let trace: Trace = Default::default();
    app.make::<openrust::events::Dispatcher>()
        .await
        .unwrap()
        .listen(
            CommandFinished::NAME,
            Arc::new(Recorder(trace.clone())),
            Delivery::Sync,
        )
        .unwrap();
    App::set_facade_application(Some(&app), async {
        for case in &cases {
            let mut output = Output::default();
            let mode = case["mode"].as_str().unwrap();
            let flag = if case["ansi"] == true {
                "--ansi"
            } else {
                "--no-ansi"
            };
            let (command, args) = if mode == "implicit" {
                ("exit:implicit", vec![flag.into()])
            } else {
                ("exit:probe", vec![mode.into(), flag.into()])
            };
            trace.lock().unwrap().clear();
            let result = Artisan::call(command, &args, &mut output).await;
            assert_eq!(
                result.as_ref().ok().copied(),
                case["status"].as_i64().map(|v| v as i32),
                "{case}"
            );
            if let Err(error) = result {
                assert!(
                    error.to_string().contains("outside manual failure"),
                    "{error}"
                );
                assert_eq!(
                    *trace.lock().unwrap(),
                    [(command.into(), 1, output.stdout.clone())]
                );
            } else {
                assert_eq!(
                    *trace.lock().unwrap(),
                    [(command.into(), result.unwrap(), output.stdout.clone())]
                );
            }
            assert_eq!(output.stdout, case["out"], "{case}");
        }
    })
    .await;
}

#[test]
#[ignore = "由父消费者执行实际命令"]
fn exit_codes_cli_driver() {
    let mode = std::env::var("ARTISAN_EXIT_MODE").unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = application(root.path());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime
        .block_on(app.handle_command(&["exit:probe".into(), mode.into(), "--no-ansi".into()]))
        .unwrap();
    drop(runtime);
    drop(root);
    std::process::exit(code);
}

#[test]
fn actual_artisan_return_codes_and_stop_output() {
    for (mode, expected_code, text) in [
        ("implicit", 0, "finished"),
        ("explicit", 23, ""),
        ("default", 1, "Command failed manually."),
        ("nested", 1, "Stop this operation."),
        ("throwable", 1, "outside manual failure"),
    ] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "exit_codes_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("ARTISAN_EXIT_MODE", mode)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&child.stdout);
        let stderr = String::from_utf8_lossy(&child.stderr);
        assert_eq!(
            child.status.code(),
            Some(expected_code),
            "{stdout}\n{stderr}"
        );
        if mode == "throwable" {
            assert!(stderr.contains(text), "{stderr}");
        } else {
            assert!(stdout.contains(text), "{stdout}");
            if expected_code == 1 {
                assert!(!stdout.contains("finished"), "{stdout}");
                assert!(stderr.is_empty(), "{stderr}");
            }
        }
    }
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/artisan-exit", response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let mode = request
            .query(Some("mode"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "implicit".into());
        let mut output = Output::default();
        let result = Artisan::call(
            "exit:probe",
            &[mode.into(), "--no-ansi".into()],
            &mut output,
        )
        .await;
        openrust::http::Response::json(
            if result.is_ok() {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            json!({"status":result.ok(),"output":output.stdout}),
        )
    })
}
#[tokio::test]
async fn actual_http_artisan_returns_manual_failure_as_an_exit_status() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(application(root.path()));
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
    for (mode, response, status) in [
        ("nested", 200, Some(1)),
        ("throwable", 500, None),
        ("explicit", 200, Some(23)),
        ("implicit", 200, Some(0)),
    ] {
        let result = client
            .get(format!("http://{address}/artisan-exit?mode={mode}"))
            .send()
            .await
            .unwrap();
        assert_eq!(result.status(), response);
        let body: Value = result.json().await.unwrap();
        assert_eq!(body["status"], json!(status));
        if mode == "nested" {
            assert!(
                body["output"]
                    .as_str()
                    .unwrap()
                    .contains("Stop this operation.")
            );
        }
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn default_failure_resolves_application_language() {
    let root = tempfile::tempdir().unwrap();
    let mut app = application(root.path());
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        for (locale, text) in [
            ("zh-Hans", "命令已手动终止。"),
            ("zh-Hant", "命令已手動終止。"),
            ("en", "Command failed manually."),
        ] {
            App::set_locale(locale).await.unwrap();
            let mut output = Output::default();
            let code = Artisan::call(
                "exit:probe",
                &["default".into(), "--no-ansi".into()],
                &mut output,
            )
            .await
            .unwrap();
            assert_eq!(code, 1);
            assert!(
                output.stdout.contains(text),
                "{locale}: {:?}",
                output.stdout
            );
        }
    })
    .await;
}
