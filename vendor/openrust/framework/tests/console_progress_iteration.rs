use openrust::{
    console::{ConsoleError, Input, Output, ProgressBar},
    foundation::{Application, configuration::Routing},
    support::{
        ArrayKey,
        facades::{App, Artisan, Facade as _},
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/progress-iterator-oracle.json"
    ))
    .unwrap()
}
fn register() -> Result<(), ConsoleError> {
    let original = ProgressBar::get_placeholder_formatter_definition("bar")?.unwrap();
    ProgressBar::set_placeholder_formatter_definition(
        "bar",
        Arc::new(move |bar| {
            bar.min_seconds_between_redraws(1000.0);
            original(bar)
        }),
    )?;
    Artisan::command_with(
        "progress:iterate {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let effects = Arc::new(Mutex::new(Vec::<String>::new()));
            let items: Box<dyn Iterator<Item = (ArrayKey, String)> + Send> =
                match case["items"].as_str().unwrap() {
                    "list" => Box::new(
                        vec!["A", "B", "C"]
                            .into_iter()
                            .enumerate()
                            .map(|(i, v)| (ArrayKey::Integer(i as i64), v.to_owned())),
                    ),
                    "map" => Box::new(
                        vec![
                            (ArrayKey::from("a"), "A".to_owned()),
                            (ArrayKey::from(7), "B".to_owned()),
                        ]
                        .into_iter(),
                    ),
                    "empty" => Box::new(std::iter::empty()),
                    _ => {
                        let effects = effects.clone();
                        let mut index = 0;
                        let empty = case["items"] == "empty-generator";
                        Box::new(std::iter::from_fn(move || {
                            let mut log = effects.lock().unwrap();
                            let item = match index {
                                0 => {
                                    log.push("opened".into());
                                    if empty {
                                        log.push("ended".into());
                                        None
                                    } else {
                                        Some(("a".into(), "A".into()))
                                    }
                                }
                                1 => {
                                    log.push("resumed-a".into());
                                    Some((7.into(), "B".into()))
                                }
                                _ => {
                                    log.extend(["resumed-b".into(), "ended".into()]);
                                    None
                                }
                            };
                            index += 1;
                            item
                        }))
                    }
                };
            let mut io = input.io(output);
            let mut seen = Vec::new();
            let mut steps = Vec::new();
            assert!(effects.lock().unwrap().is_empty());
            let max = case["max"].as_i64();
            if case["styled"] == true {
                let mut it = io.get_output().progress_iterate_with_max(items, max);
                if case["consume"] != "unstarted" {
                    for value in &mut it {
                        let (key, value) = value?;
                        seen.push(json!([key, value]));
                        if case["consume"] == "break" {
                            break;
                        }
                    }
                }
            } else {
                let mut bar = ProgressBar::new(
                    io.get_output().get_output(),
                    case["initial"].as_i64().unwrap(),
                )?;
                bar.min_seconds_between_redraws(1000.0);
                let mut it = bar.iterate_with_max(items, max);
                assert_eq!(
                    it.progress_bar().get_progress(),
                    case["before"]["progress"].as_i64().unwrap()
                );
                assert_eq!(
                    it.progress_bar().get_max_steps(),
                    case["before"]["max"].as_i64().unwrap()
                );
                if case["consume"] != "unstarted" {
                    while let Some(value) = it.next() {
                        let (key, value) = value?;
                        seen.push(json!([key, value]));
                        steps.push(json!([
                            it.progress_bar().get_progress(),
                            it.progress_bar().get_max_steps()
                        ]));
                        if case["consume"] == "break" {
                            break;
                        }
                    }
                }
                drop(it);
                assert_eq!(json!(bar.get_progress()), case["progress"], "{case}");
                assert_eq!(json!(bar.get_max_steps()), case["max_after"], "{case}");
                assert_eq!(
                    bar.get_progress_percent(),
                    case["percent"].as_f64().unwrap(),
                    "{case}"
                );
            }
            assert_eq!(json!(seen), case["seen"], "{case}");
            assert_eq!(json!(steps), case["steps"], "{case}");
            assert_eq!(json!(*effects.lock().unwrap()), case["effects"], "{case}");
            assert_eq!(
                json!(io.get_output().new_lines_written()),
                case["new_lines"],
                "{case}"
            );
            assert_eq!(
                json!(io.get_output().new_line_written()),
                case["new_line"],
                "{case}"
            );
            drop(io);
            if case["stream"] != true {
                assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
            }
            Ok(0)
        },
    )?;
    Artisan::command_with(
        "progress:iteration-error",
        |input: &Input, output: &mut Output| {
            let mut io = input.io(output);
            let mut bar = io.get_output().create_progress_bar(2)?;
            bar.set_format("%current:Z%");
            let mut it = bar.iterate([1, 2]);
            it.next().unwrap()?;
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn lazy_lifecycle_keys_early_drop_and_unknown_totals_match_actual_source() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases() {
            let flag = if case["decorated"] == true {
                "--ansi"
            } else {
                "--no-ansi"
            };
            Artisan::call(
                "progress:iterate",
                &[case.to_string().into(), flag.into()],
                &mut Output::default(),
            )
            .await
            .unwrap();
        }
    })
    .await;
}
#[test]
fn original_items_are_not_cloned_and_iterator_errors_are_terminal() {
    struct Item(String);
    let items = vec![Item("a".into()), Item("b".into())];
    let pointer = items.as_ptr();
    let mut output = Output::default();
    let mut bar = ProgressBar::new(&mut output, 0).unwrap();
    {
        let mut it = bar.iterate(&items);
        assert_eq!(it.next().unwrap().unwrap().0, "a");
        assert!(std::ptr::eq(it.next().unwrap().unwrap(), &items[1]));
        assert!(it.next().is_none());
        assert!(it.next().is_none());
    }
    assert_eq!(items.as_ptr(), pointer);
    assert_eq!(bar.get_progress(), 2);
    bar.set_format("%current:Z%");
    {
        let mut it = bar.iterate(&items);
        assert!(it.next().unwrap().is_err());
        assert!(it.next().is_none());
    }
    bar.set_format("%current%/%max%");
    {
        let mut it = bar.iterate(&items);
        assert!(it.next().unwrap().is_ok());
    }
    assert_eq!(bar.get_progress(), 0);
}
#[test]
#[ignore = "由实际Artisan子进程驱动"]
fn iteration_cli_fixture() {
    let app = Application::configure(std::env::var("ITERATION_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let args: Vec<std::ffi::OsString> =
        serde_json::from_str::<Vec<String>>(&std::env::var("ITERATION_ARGS").unwrap())
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    std::process::exit(code);
}
#[test]
fn actual_artisan_stderr_keeps_lazy_output_and_exit_status() {
    let root = tempfile::tempdir().unwrap();
    let streams: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/progress-iterator-cli-oracle.json"
    ))
    .unwrap();
    for (index, mut case) in cases().into_iter().enumerate() {
        case["stream"] = json!(true);
        let flag = if case["decorated"] == true {
            "--ansi"
        } else {
            "--no-ansi"
        };
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "iteration_cli_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("ITERATION_ROOT", root.path())
            .env(
                "ITERATION_ARGS",
                json!(["progress:iterate", case.to_string(), flag]).to_string(),
            )
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(
            String::from_utf8(child.stderr).unwrap(),
            streams[index]["stderr"].as_str().unwrap(),
            "{case}"
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        assert_eq!(
            stdout.strip_prefix("\nrunning 1 test\n").unwrap(),
            streams[index]["stdout"].as_str().unwrap(),
            "{case}"
        );
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    Route::get("/iterate/{index}", iteration_response)?;
    Ok(())
}
fn iteration_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let index = request.route("index").unwrap().parse::<usize>().unwrap();
        let mut output = Output::default();
        let (command, args) = if index == 999 {
            ("progress:iteration-error", vec![])
        } else {
            (
                "progress:iterate",
                vec![cases()[index].to_string().into(), "--no-ansi".into()],
            )
        };
        Artisan::call(command, &args, &mut output)
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
    })
}
#[tokio::test]
async fn actual_http_iteration_errors_and_recovery_use_the_registered_entry() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register).web(routes))
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
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
    for (index, status) in [
        (0, 200),
        (5, 200),
        (6, 200),
        (17, 200),
        (999, 500),
        (0, 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}/iterate/{index}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert_eq!(
                response.json::<Value>().await.unwrap()["output"],
                cases()[index]["output"]
            );
        }
    }
    server.abort();
    let _ = server.await;
}
