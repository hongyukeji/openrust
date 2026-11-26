use openrust::{
    console::{ConsoleError, Input, Output},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _},
};
use serde_json::{Value, json};
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "progress:probe {case}",
        |input: &Input, output: &mut Output| {
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let mut trace = Vec::new();
            let mut result = Value::Null;
            let mut error = None;
            let mut io = input.io(output);
            match case["mode"].as_str().unwrap() {
                "count" | "failure" => {
                    let res = io.with_progress_bar(3, |bar| {
                        bar.min_seconds_between_redraws(
                            case["fixture_min_seconds"].as_f64().unwrap(),
                        );
                        trace.push(json!([bar.get_progress(), bar.get_max_steps()]));
                        bar.advance()?;
                        if case["mode"] == "failure" {
                            return Err(ConsoleError::new("failure"));
                        }
                        bar.advance_by(2)
                    });
                    if let Err(e) = res {
                        error = Some(e.to_string());
                    }
                }
                "list" | "empty" => {
                    let items = if case["mode"] == "empty" {
                        Vec::<Value>::new()
                    } else {
                        vec![json!("A"), json!("B"), json!("C")]
                    };
                    result = json!(io.with_progress_bar_iter(items, |value, bar, key| {
                        bar.min_seconds_between_redraws(
                            case["fixture_min_seconds"].as_f64().unwrap(),
                        );
                        trace.push(json!([value, key, bar.get_progress(), bar.get_max_steps()]));
                        Ok(())
                    })?);
                }
                "map" => {
                    let items = std::collections::BTreeMap::from([
                        ("a".to_owned(), json!("A")),
                        ("b".to_owned(), json!("B")),
                    ]);
                    result = json!(io.with_progress_bar_iter(items, |value, bar, key| {
                        bar.min_seconds_between_redraws(
                            case["fixture_min_seconds"].as_f64().unwrap(),
                        );
                        trace.push(json!([value, key, bar.get_progress(), bar.get_max_steps()]));
                        Ok(())
                    })?);
                }
                _ => panic!("未知进度测试模式"),
            }
            assert_eq!(
                io.get_output().new_lines_written() as u64,
                case["new_lines"].as_u64().unwrap()
            );
            assert_eq!(
                io.get_output().new_line_written(),
                case["new_line"].as_bool().unwrap()
            );
            drop(io);
            assert_eq!(trace, case["trace"].as_array().unwrap().to_vec(), "{case}");
            assert_eq!(result, case["result"], "{case}");
            assert_eq!(json!(error), case["error"], "{case}");
            assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
            Ok(0)
        },
    )?;
    register_operations()?;
    register_style()?;
    register_http_commands()?;
    Ok(())
}
#[tokio::test]
async fn progress_helpers_match_fixed_source_and_keep_output_style_counters() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/progress-oracle.json"
    ))
    .unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases.as_array().unwrap() {
            let mut args = vec![
                case.to_string().into(),
                if case["decorated"] == true {
                    "--ansi".into()
                } else {
                    "--no-ansi".into()
                },
            ];
            match case["level"].as_i64().unwrap() {
                16 => args.push("--quiet".into()),
                8 => args.push("--silent".into()),
                _ => {}
            }
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("progress:probe", &args, &mut output)
                    .await
                    .unwrap(),
                0
            );
        }
    })
    .await;
}

fn register_operations() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "progress:ops {case}",
        |input: &Input, output: &mut Output| {
            use openrust::console::{ProgressBar, ProgressFormatter};
            use std::sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            };
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let mut trace = Vec::new();
            let mut io = input.io(output);
            let mut bar = if case["styled"] == true {
                io.get_output()
                    .create_progress_bar(case["max"].as_i64().unwrap())?
            } else {
                ProgressBar::new_with_redraw_interval(
                    io.get_output().get_output(),
                    case["max"].as_i64().unwrap(),
                    match case["constructor_min"].as_str() {
                        Some("nan") => f64::NAN,
                        Some("zero") => 0.0,
                        Some("negative") => -1.0,
                        _ => 1.0 / 25.0,
                    },
                )?
            };
            for op in case["ops"].as_array().unwrap() {
                let result: Result<(), ConsoleError> = (|| {
                    match op[0].as_str().unwrap() {
                        "start" => {
                            bar.start_at(
                                op.get(1).and_then(Value::as_i64),
                                op.get(2).and_then(Value::as_i64).unwrap_or(0),
                            )?;
                        }
                        "advance" => {
                            bar.advance_by(op.get(1).and_then(Value::as_i64).unwrap_or(1))?;
                        }
                        "setProgress" => {
                            bar.set_progress(op[1].as_i64().unwrap())?;
                        }
                        "setMaxSteps" => {
                            bar.set_max_steps(op[1].as_i64());
                        }
                        "display" => {
                            bar.display()?;
                        }
                        "clear" => {
                            bar.clear()?;
                        }
                        "finish" => {
                            bar.finish()?;
                        }
                        "setBarWidth" => {
                            bar.set_bar_width(op[1].as_i64().unwrap());
                        }
                        "setBarCharacter" => {
                            bar.set_bar_character(op[1].as_str().unwrap());
                        }
                        "setEmptyBarCharacter" => {
                            bar.set_empty_bar_character(op[1].as_str().unwrap());
                        }
                        "setProgressCharacter" => {
                            bar.set_progress_character(op[1].as_str().unwrap());
                        }
                        "setOverwrite" => {
                            bar.set_overwrite(op[1].as_bool().unwrap());
                        }
                        "setRedrawFrequency" => {
                            bar.set_redraw_frequency(op[1].as_i64());
                        }
                        "minSecondsBetweenRedraws" => {
                            bar.min_seconds_between_redraws(op[1].as_f64().unwrap());
                        }
                        "maxSecondsBetweenRedraws" => {
                            bar.max_seconds_between_redraws(op[1].as_f64().unwrap());
                        }
                        "setFormat" => {
                            bar.set_format(op[1].as_str().unwrap());
                        }
                        "setMessage" => {
                            bar.set_message_named(
                                op[1].as_str().unwrap(),
                                op.get(2).and_then(Value::as_str).unwrap_or("message"),
                            );
                        }
                        "formatDefinition" => {
                            ProgressBar::set_format_definition(
                                op[1].as_str().unwrap(),
                                op[2].as_str().unwrap(),
                            )?;
                        }
                        "globalFormatter" | "localFormatter" => {
                            let prefix = op[2].as_str().unwrap().to_owned();
                            let calls = calls.clone();
                            let callback: ProgressFormatter = Arc::new(move |bar| {
                                calls.fetch_add(1, Ordering::SeqCst);
                                Ok(format!("{prefix}{}", bar.get_progress()))
                            });
                            if op[0] == "globalFormatter" {
                                ProgressBar::set_placeholder_formatter_definition(
                                    op[1].as_str().unwrap(),
                                    callback,
                                )?;
                            } else {
                                bar.set_placeholder_formatter(op[1].as_str().unwrap(), callback);
                            }
                        }
                        _ => panic!("未知进度操作"),
                    }
                    Ok(())
                })();
                trace.push(json!({"error":result.is_err(),"progress":bar.get_progress(),"max":bar.get_max_steps(),"percent":bar.get_progress_percent(),"offset":bar.get_bar_offset(),"width":bar.get_bar_width(),"bar":bar.get_bar_character(),"empty":bar.get_empty_bar_character(),"next":bar.get_progress_character(),"message":bar.get_message(),"calls":calls.load(Ordering::SeqCst)}));
            }
            drop(bar);
            assert_eq!(trace, case["trace"].as_array().unwrap().to_vec(), "{case}");
            assert_eq!(
                io.get_output().new_lines_written() as u64,
                case["new_lines"].as_u64().unwrap(),
                "{case}"
            );
            assert_eq!(
                io.get_output().new_line_written(),
                case["new_line"].as_bool().unwrap(),
                "{case}"
            );
            drop(io);
            if case["stream"] != true {
                assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
            }
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn progress_operations_match_fixed_source_via_the_public_command_entry() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/progress-operations-oracle.json"
    ))
    .unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case.get("columns").is_none())
        {
            let flag = if case["decorated"] == true {
                "--ansi"
            } else {
                "--no-ansi"
            };
            let mut output = Output::default();
            assert_eq!(
                Artisan::call(
                    "progress:ops",
                    &[case.to_string().into(), flag.into()],
                    &mut output
                )
                .await
                .unwrap(),
                0
            );
        }
    })
    .await;
}
#[test]
#[ignore = "由实际Artisan子进程及PTY驱动调用"]
fn progress_cli_fixture() {
    let root = std::env::var("PROGRESS_ROOT").unwrap();
    let arguments: Vec<std::ffi::OsString> =
        serde_json::from_str::<Vec<String>>(&std::env::var("PROGRESS_ARGUMENTS").unwrap())
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
    let app = Application::configure(root.into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let code = runtime.block_on(app.handle_command(&arguments)).unwrap();
    std::process::exit(code);
}
#[test]
fn actual_cli_progress_uses_stderr_and_respects_terminal_width() {
    let root = tempfile::tempdir().unwrap();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/progress-operations-oracle.json"
    ))
    .unwrap();
    for case in cases
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case.get("columns").is_some())
    {
        let mut case = case.clone();
        case["stream"] = json!(true);
        let flag = if case["decorated"] == true {
            "--ansi"
        } else {
            "--no-ansi"
        };
        let arguments = json!(["progress:ops", case.to_string(), flag]);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "progress_cli_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("PROGRESS_ROOT", root.path())
            .env("PROGRESS_ARGUMENTS", arguments.to_string())
            .env("COLUMNS", case["columns"].as_i64().unwrap().to_string())
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
            case["output"].as_str().unwrap(),
            "{case}"
        );
        assert!(!String::from_utf8_lossy(&child.stdout).contains('▓'));
    }
}

fn register_style() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "progress:style {case}",
        |input: &Input, output: &mut Output| {
            use openrust::console::ProgressBar;
            use std::sync::Arc;
            let case: Value =
                serde_json::from_str(input.argument("case").unwrap().as_str().unwrap()).unwrap();
            let original = ProgressBar::get_placeholder_formatter_definition("bar")?.unwrap();
            let delegate = original.clone();
            ProgressBar::set_placeholder_formatter_definition(
                "bar",
                Arc::new(move |bar| {
                    bar.min_seconds_between_redraws(1000.0);
                    delegate(bar)
                }),
            )?;
            let mut io = input.io(output);
            let mut trace = Vec::new();
            for op in case["ops"].as_array().unwrap() {
                let style = io.get_output();
                let result = match op[0].as_str().unwrap() {
                    "progressStart" => {
                        style.progress_start(op.get(1).and_then(Value::as_i64).unwrap_or(0))
                    }
                    "progressAdvance" => {
                        style.progress_advance_by(op.get(1).and_then(Value::as_i64).unwrap_or(1))
                    }
                    "progressFinish" => style.progress_finish(),
                    _ => panic!("未知样式进度操作"),
                };
                trace.push(json!({"error":result.is_err(),"new_lines":style.new_lines_written(),"new_line":style.new_line_written()}));
            }
            drop(io);
            ProgressBar::set_placeholder_formatter_definition("bar", original)?;
            assert_eq!(trace, case["trace"].as_array().unwrap().to_vec(), "{case}");
            assert_eq!(output.stdout, case["output"].as_str().unwrap(), "{case}");
            Ok(0)
        },
    )?;
    Ok(())
}
#[tokio::test]
async fn stored_style_progress_lifecycle_and_missing_state_match_fixed_source() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/progress-style-oracle.json"
    ))
    .unwrap();
    App::set_facade_application(Some(&app), async {
        for case in cases.as_array().unwrap() {
            let mut args = vec![
                case.to_string().into(),
                if case["decorated"] == true {
                    "--ansi".into()
                } else {
                    "--no-ansi".into()
                },
            ];
            match case["level"].as_i64().unwrap() {
                16 => args.push("--quiet".into()),
                8 => args.push("--silent".into()),
                _ => {}
            }
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("progress:style", &args, &mut output)
                    .await
                    .unwrap(),
                0
            );
        }
    })
    .await;
}
#[tokio::test]
async fn definitions_follow_the_owning_application_and_callbacks_can_reenter_the_registry() {
    use openrust::console::{ConsoleProgressError, ProgressBar};
    use std::{error::Error as _, sync::Arc};
    let a_root = tempfile::tempdir().unwrap();
    let b_root = tempfile::tempdir().unwrap();
    let a = Application::configure(a_root.path().into())
        .create()
        .unwrap();
    let b = Application::configure(b_root.path().into())
        .create()
        .unwrap();
    App::set_facade_application(None, async {
        assert!(matches!(
            ProgressBar::set_format_definition("owner", "outside")
                .unwrap_err()
                .source()
                .unwrap()
                .downcast_ref::<ConsoleProgressError>(),
            Some(ConsoleProgressError::MissingDefinitions)
        ));
        assert!(
            ProgressBar::get_format_definition("normal")
                .unwrap()
                .is_some()
        );
        assert!(
            ProgressBar::get_format_definition("unknown")
                .unwrap()
                .is_none()
        );
    })
    .await;
    let mut output = Output::default();
    let output_ref = &mut output;
    let mut bar = App::set_facade_application(Some(&a), async move {
        ProgressBar::set_format_definition("owner", "%probe% %current%/%max%").unwrap();
        ProgressBar::set_placeholder_formatter_definition(
            "probe",
            Arc::new(|bar| Ok(format!("A{}", bar.get_progress()))),
        )
        .unwrap();
        ProgressBar::new(output_ref, 2).unwrap()
    })
    .await;
    App::set_facade_application(Some(&b), async {
        assert!(
            ProgressBar::get_format_definition("owner")
                .unwrap()
                .is_none()
        );
        ProgressBar::set_format_definition("owner", "B").unwrap();
        ProgressBar::set_placeholder_formatter_definition("probe", Arc::new(|_| Ok("B".into())))
            .unwrap();
        bar.set_format("owner");
        bar.start().unwrap();
        bar.advance_by(2).unwrap();
        bar.finish().unwrap();
        assert_eq!(
            ProgressBar::get_format_definition("owner")
                .unwrap()
                .as_deref(),
            Some("B")
        );
    })
    .await;
    drop(bar);
    assert_eq!(output.stdout, "A0 0/2\nA2 2/2");
    App::set_facade_application(Some(&a), async {
        let mut output = Output::default();
        let mut bar = ProgressBar::new(&mut output, 1).unwrap();
        bar.set_placeholder_formatter(
            "probe",
            Arc::new(|bar| {
                ProgressBar::set_format_definition("from_callback", "ok")?;
                bar.set_message_named("changed", "item");
                Ok("local".into())
            }),
        );
        let callback = bar.get_placeholder_formatter("probe").unwrap();
        assert_eq!(callback(&mut bar).unwrap(), "local");
        assert_eq!(bar.get_message_named("item"), Some("changed"));
        assert_eq!(
            ProgressBar::get_format_definition("from_callback")
                .unwrap()
                .as_deref(),
            Some("ok")
        );
        let global = ProgressBar::get_placeholder_formatter_definition("probe")
            .unwrap()
            .unwrap();
        assert_eq!(global(&mut bar).unwrap(), "A0");
    })
    .await;
}
#[tokio::test]
async fn elapsed_estimates_memory_and_default_throttling_use_real_native_sources() {
    use openrust::console::{ConsoleProgressError, ConsoleTemplateError, ProgressBar};
    use std::{
        error::Error as _,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    let secs = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    };
    let mut output = Output::default();
    let before = secs();
    let mut bar = ProgressBar::new(&mut output, 5).unwrap();
    bar.set_format("%current%/%max%:%elapsed%:%estimated%:%remaining%");
    bar.start().unwrap();
    assert!(bar.get_start_time() >= before && bar.get_start_time() <= secs());
    assert_eq!(bar.get_estimated(), 0.0);
    assert_eq!(bar.get_remaining(), 0.0);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    bar.advance_by(2).unwrap();
    let before = secs();
    let estimated = bar.get_estimated();
    let remaining = bar.get_remaining();
    let after = secs();
    assert!(
        (before..=after)
            .any(|now| estimated == (((now - bar.get_start_time()) as f64 / 2.0) * 5.0).round())
    );
    assert!(
        (before..=after)
            .any(|now| remaining == (((now - bar.get_start_time()) as f64 / 2.0) * 3.0).round())
    );
    let memory = bar.get_placeholder_formatter("memory").unwrap()(&mut bar).unwrap();
    assert!(
        regex::Regex::new(r"^[0-9]+(?:\.[0-9])? (?:B|KiB|MiB|GiB)$")
            .unwrap()
            .is_match(&memory),
        "{memory}"
    );
    assert!(
        memory
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<f64>()
            .unwrap()
            > 0.0
    );
    bar.set_format("%current:Z%");
    let error = bar.display().unwrap_err();
    assert!(matches!(
        error
            .source()
            .unwrap()
            .downcast_ref::<ConsoleProgressError>(),
        Some(ConsoleProgressError::InvalidFormat(_))
    ));
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<ConsoleTemplateError>()
    );
    bar.set_format("ok");
    bar.display().unwrap();
    drop(bar);
    assert!(output.stdout.contains("2/5:"));
    let mut output = Output::default();
    let mut bar = ProgressBar::new(&mut output, 1).unwrap();
    bar.set_progress(i64::MAX).unwrap();
    let error = bar.advance().unwrap_err();
    assert!(matches!(
        error
            .source()
            .unwrap()
            .downcast_ref::<ConsoleProgressError>(),
        Some(ConsoleProgressError::StepOverflow { .. })
    ));
}
#[test]
fn iterable_callbacks_keep_non_clone_models_their_keys_and_the_original_collection() {
    use openrust::{
        console::CommandDefinition,
        database::eloquent::{Collection, Model, ModelMetadata, ModelState},
    };
    struct Record(ModelState);
    impl Model for Record {
        fn metadata() -> &'static ModelMetadata {
            static META: ModelMetadata = ModelMetadata::new("records");
            &META
        }
        fn from_state(state: ModelState) -> Self {
            Self(state)
        }
        fn state(&self) -> &ModelState {
            &self.0
        }
        fn state_mut(&mut self) -> &mut ModelState {
            &mut self.0
        }
    }
    let models = Collection::from_entries([
        ("east", Record(ModelState::default())),
        ("west", Record(ModelState::default())),
    ]);
    let original = models.all().as_ptr();
    let input = CommandDefinition::from_signature("progress")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut keys = Vec::new();
    let models = input
        .io(&mut output)
        .with_progress_bar_iter(models, |_, bar, key| {
            keys.push(key);
            assert_eq!(bar.get_max_steps(), 2);
            Ok(())
        })
        .unwrap();
    assert_eq!(original, models.all().as_ptr());
    assert!(models.get_by_key("east").is_some());
    assert_eq!(keys, vec!["east".into(), "west".into()]);
    let values = vec![1, 2, 3];
    let returned = input
        .io(&mut output)
        .with_progress_bar_iter(&values, |_, _, _| Ok(()))
        .unwrap();
    assert!(std::ptr::eq(returned, &values));
    let values = openrust::support::Collection::from_entries([("left", 1), ("right", 2)]);
    let returned = input
        .io(&mut output)
        .with_progress_bar_iter(&values, |_, _, _| Err::<(), _>(ConsoleError::new("failed")));
    assert!(returned.is_err());
    assert_eq!(values.get("left"), Some(&1));
}

fn register_http_commands() -> Result<(), ConsoleError> {
    Artisan::command_with("progress:owned", |input: &Input, output: &mut Output| {
        let mut io = input.io(output);
        let mut bar = io.get_output().create_progress_bar(2)?;
        bar.set_format("owned");
        bar.start()?;
        bar.advance_by(2)?;
        bar.finish()?;
        Ok(0)
    })?;
    Artisan::command_with("progress:failed", |input: &Input, output: &mut Output| {
        input
            .io(output)
            .with_progress_bar(3, |_| Err::<(), _>(ConsoleError::new("任务回调失败")))?;
        Ok(0)
    })?;
    Ok(())
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    for (path, command) in [
        ("/progress/default", "progress:owned"),
        ("/progress/error", "progress:failed"),
    ] {
        Route::get(path, move || async move {
            let mut output = Output::default();
            Artisan::call(command, &[], &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_progress_namespaces_are_isolated_and_failures_do_not_poison_later_requests() {
    use axum::handler::Handler as _;
    use openrust::{console::ProgressBar, contracts::http::Kernel as _};
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let mut servers = Vec::new();
    let mut addresses = Vec::new();
    for label in ["A", "B"] {
        let base = root.path().join(label);
        std::fs::create_dir_all(&base).unwrap();
        let mut app = Application::configure(base)
            .with_routing(Routing::default().commands(register).web(routes))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        App::set_facade_application(Some(&app), async {
            ProgressBar::set_format_definition("owned", "%owner%:%current%/%max%").unwrap();
            ProgressBar::set_placeholder_formatter_definition(
                "owner",
                Arc::new(move |_| Ok(label.into())),
            )
            .unwrap();
        })
        .await;
        let mut kernel = openrust::foundation::http::Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        addresses.push(listener.local_addr().unwrap());
        servers.push(tokio::spawn(async move {
            axum::serve(
                listener,
                transport.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap();
        }));
    }
    let client = reqwest::Client::new();
    futures_util::future::join_all((0..48).map(|index| {
        let client = client.clone();
        let address = addresses[index % 2];
        let label = ["A", "B"][index % 2];
        async move {
            let response = client
                .get(format!("http://{address}/progress/default"))
                .header("Accept", "application/json")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body = response.json::<Value>().await.unwrap();
            let output = body["output"].as_str().unwrap();
            assert!(output.starts_with(&format!("{label}:0/2")), "{body}");
            assert!(output.ends_with(&format!("{label}:2/2")), "{body}");
        }
    }))
    .await;
    for (index, address) in addresses.iter().enumerate() {
        let response = client
            .get(format!("http://{address}/progress/error"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 500);
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .contains("application/json")
        );
        let response = client
            .get(format!("http://{address}/progress/default"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response.json::<Value>().await.unwrap()["output"]
                .as_str()
                .unwrap()
                .contains(["A:2/2", "B:2/2"][index])
        );
    }
    for server in servers {
        server.abort();
        let _ = server.await;
    }
}
#[tokio::test]
async fn the_default_redraw_interval_suppresses_an_early_step_and_draws_a_later_step() {
    use openrust::console::ProgressBar;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    let mut observed = false;
    for _ in 0..5 {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let mut output = Output::default();
        let mut bar = ProgressBar::new(&mut output, 10).unwrap();
        bar.set_format("%tick%")
            .set_redraw_frequency(Some(1))
            .set_placeholder_formatter(
                "tick",
                Arc::new(move |bar| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(bar.get_progress().to_string())
                }),
            );
        let before = Instant::now();
        bar.start().unwrap();
        let initial = calls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(35)).await;
        bar.advance().unwrap();
        if before.elapsed() < Duration::from_millis(40) {
            assert_eq!(calls.load(Ordering::SeqCst), initial);
            tokio::time::sleep(Duration::from_millis(80)).await;
            bar.advance().unwrap();
            assert!(calls.load(Ordering::SeqCst) > initial);
            observed = true;
            break;
        }
    }
    assert!(observed, "未能取得35至40毫秒的实际计时窗口");
}

#[test]
fn progress_estimates_support_extreme_resume_points_without_overflow() {
    use openrust::console::ProgressBar;
    let mut output = Output::default();
    let mut bar = ProgressBar::new(&mut output, 3).unwrap();
    bar.set_format("%estimated%:%remaining%");
    bar.start_at(None, i64::MIN).unwrap();
    bar.set_progress(1).unwrap();
    assert_eq!(bar.get_estimated(), 0.0);
    assert_eq!(bar.get_remaining(), 0.0);
    drop(bar);
    let mut bar = ProgressBar::new(&mut output, i64::MAX).unwrap();
    bar.start_at(None, i64::MAX - 1).unwrap();
    bar.set_progress(i64::MAX).unwrap();
    assert!(bar.get_estimated().is_finite());
    assert!(bar.get_remaining().is_finite());
}

#[tokio::test]
async fn nan_constructor_redraw_matches_fixed_source() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let cases: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/progress-operations-oracle.json"
    ))
    .unwrap();
    let case = cases
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case_label"] == "constructor-nan" && case["decorated"] == true)
        .unwrap();
    App::set_facade_application(Some(&app), async {
        let mut output = Output::default();
        assert_eq!(
            Artisan::call(
                "progress:ops",
                &[case.to_string().into(), "--ansi".into()],
                &mut output
            )
            .await
            .unwrap(),
            0
        );
    })
    .await;
}
