use openrust::console::{
    ConsoleError, Output, OutputFormat, OutputOptions, OutputVerbosity, TableCell,
    TableCellOptions, TableCellStyle, TableRow, TableSeparator,
};
use serde_json::Value;
#[test]
fn fixed_source_sections_and_append_sequences_in_isolated_process() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "sections_source_driver",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
#[ignore = "由独立父消费者驱动，隔离终端环境变量"]
fn sections_source_driver() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/sections-oracle.json"
    ))
    .unwrap();
    for (case_index, case) in cases.iter().enumerate() {
        // 终端尺寸每次调用读取；本测试只在自己的进程中设置。
        unsafe {
            std::env::set_var("COLUMNS", case["width"].to_string());
        }
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let mut handles = vec![output.section().handle()];
        for (index, operation) in case["operations"].as_array().unwrap().iter().enumerate() {
            let op = operation[0].as_str().unwrap();
            let result = if op == "new" {
                handles.push(output.section().handle());
                Ok(())
            } else {
                let mut section = output
                    .section_output(&handles[operation[1].as_u64().unwrap() as usize])
                    .unwrap();
                match op {
                    "tone" => {
                        section.get_formatter().set_style(
                            "tone",
                            std::sync::Arc::new(
                                openrust::console::OutputFormatterStyle::new(
                                    operation[2].as_str(),
                                    None,
                                    &[],
                                )
                                .unwrap(),
                            ),
                        );
                        Ok(())
                    }
                    "formatter" => {
                        let formatter: openrust::console::FormatterHandle =
                            std::sync::Arc::new(openrust::console::OutputFormatter::new(
                                case["decorated"].as_bool().unwrap(),
                            ));
                        formatter.set_style(
                            "tone",
                            std::sync::Arc::new(
                                openrust::console::OutputFormatterStyle::new(
                                    operation[2].as_str(),
                                    None,
                                    &[],
                                )
                                .unwrap(),
                            ),
                        );
                        section.set_formatter(formatter);
                        Ok(())
                    }
                    "decorated" => {
                        section.set_decorated(operation[2].as_bool().unwrap());
                        Ok(())
                    }
                    "max" => section.set_max_height(operation[2].as_i64().unwrap()),
                    "level" => {
                        section.set_verbosity(match operation[2].as_i64().unwrap() {
                            16 => OutputVerbosity::Quiet,
                            256 => OutputVerbosity::Debug,
                            _ => OutputVerbosity::Normal,
                        });
                        Ok(())
                    }
                    "clear" => section.clear(operation[2].as_i64()),
                    "overwrite" => section.overwrite(operation[2].as_str().unwrap()),
                    _ => {
                        let options = OutputOptions {
                            format: match op {
                                "plain" => OutputFormat::Plain,
                                "raw" => OutputFormat::Raw,
                                _ => OutputFormat::Normal,
                            },
                            verbosity: (op == "verbose").then_some(OutputVerbosity::Verbose),
                        };
                        if let Some(lines) = operation[2].as_array() {
                            let lines: Vec<_> = lines.iter().map(|v| v.as_str().unwrap()).collect();
                            if op == "writeln" {
                                section.writeln_iter(lines, options)
                            } else {
                                section.write_iter(lines, options)
                            }
                        } else if op == "writeln" {
                            section.writeln(operation[2].as_str().unwrap(), options)
                        } else {
                            section.write(operation[2].as_str().unwrap(), options)
                        }
                    }
                }
            };
            let expected = &case["steps"][index];
            assert_eq!(
                result.is_err(),
                !expected["error"].is_null(),
                "case {case_index} step {index}"
            );
            assert_eq!(
                output.stdout,
                expected["output"].as_str().unwrap(),
                "case {case_index} step {index}"
            );
            for (id, handle) in handles.iter().enumerate() {
                let section = output.section_output(handle).unwrap();
                assert_eq!(
                    section.get_content(),
                    expected["content"][id].as_str().unwrap(),
                    "content case {case_index} step {index} section {id}"
                );
                assert_eq!(
                    section.get_visible_content(),
                    expected["visible"][id].as_str().unwrap(),
                    "visible case {case_index} step {index} section {id}"
                );
            }
        }
    }
    fixed_source_append_sequences();
    unsafe {
        std::env::remove_var("COLUMNS");
    }
}

fn options(value: &Value) -> Result<TableCellOptions, ConsoleError> {
    let style = value
        .get("style")
        .filter(|v| !v.is_null())
        .map(|v| TableCellStyle::new(v.as_object().unwrap().clone()))
        .transpose()?;
    Ok(TableCellOptions {
        colspan: value.get("colspan").cloned().unwrap_or_else(|| 1.into()),
        rowspan: value.get("rowspan").cloned().unwrap_or_else(|| 1.into()),
        style,
    })
}
fn cell(value: &Value) -> Result<TableCell, ConsoleError> {
    if value["separator"] == true {
        return Ok(TableSeparator::new()
            .with_options(options(&value["options"])?)
            .into());
    }
    if let Some(text) = value.get("cell") {
        return Ok(TableCell::new(text.clone()).with_options(options(&value["options"])?));
    }
    Ok(value.clone().into())
}
fn cells(value: &Value) -> Result<Vec<TableCell>, ConsoleError> {
    value.as_array().unwrap().iter().map(cell).collect()
}
fn row(value: &Value) -> Result<TableRow, ConsoleError> {
    if value == "@sep" {
        Ok(TableSeparator::new().into())
    } else {
        Ok(cells(value)?.into())
    }
}
fn fixed_source_append_sequences() {
    use openrust::console::{CommandDefinition, Table};
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-append-oracle.json"
    ))
    .unwrap();
    let input = CommandDefinition::from_signature("tables")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut failures = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let mut output = Output::default();
        output.set_decorated(case["decorated"].as_bool().unwrap());
        let first = {
            let mut section = output.section();
            section.writeln("prefix", OutputOptions::default()).unwrap();
            section.handle()
        };
        let later = {
            let mut section = output.section();
            section
                .writeln("other section", OutputOptions::default())
                .unwrap();
            section.handle()
        };
        let mut section = output.section_output(&first).unwrap();
        let mut style = section.style(&input);
        let mut table = Table::new(&mut style).unwrap();
        table
            .set_headers(cells(&case["headers"]).unwrap())
            .set_rows(
                case["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(row)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
            )
            .set_style_named(case["style"].as_str().unwrap())
            .unwrap();
        match case["mode"].as_str().unwrap() {
            "horizontal" => {
                table.set_horizontal(true);
            }
            "vertical" => {
                table.set_vertical(true);
            }
            _ => {}
        }
        if case["before"] == true {
            table.render().unwrap();
        }
        for (step, value) in case["append"].as_array().unwrap().iter().enumerate() {
            let result = table.append_row(row(value).unwrap());
            assert_eq!(
                result.is_err(),
                !case["steps"][step]["error"].is_null(),
                "append error case {index} step {step}"
            );
        }
        drop(table);
        drop(style);
        let expected = case["steps"].as_array().unwrap().last().unwrap();
        let content = section.get_content();
        if output.stdout != expected["output"].as_str().unwrap()
            || content != expected["content"].as_str().unwrap()
        {
            failures.push(format!(
                "case {index}: actual {:?} expected {:?}, content {:?} expected {:?}",
                output.stdout, expected["output"], content, expected["content"]
            ));
        }
        assert_eq!(
            output.section_output(&later).unwrap().get_content(),
            expected["later"].as_str().unwrap()
        );
    }
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
}
use openrust::{
    console::{Input, Table},
    foundation::{Application, configuration::Routing},
    support::facades::Artisan,
};
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tables:sections {mode}",
        |input: &Input, output: &mut Output| {
            if input.argument("mode") == Some(serde_json::json!("bad")) {
                let mut style = input.io(output);
                Table::new(style.get_output())?.append_row(["rejected"])?;
                return Ok(0);
            }
            let first = {
                let mut section = output.section();
                section.writeln("prefix", OutputOptions::default())?;
                section.handle()
            };
            {
                let mut section = output.section();
                section.writeln("other section", OutputOptions::default())?;
            }
            let mut section = output.section_output(&first)?;
            let mut style = section.style(input);
            let mut table = Table::new(&mut style)?;
            table.set_headers(["A", "B"]).set_rows([["x", "y"]]);
            table.append_row(["one", "two"])?;
            table.append_row(["longer", "last"])?;
            Ok(0)
        },
    )?;
    Ok(())
}
fn formal_output(decorated: bool) -> String {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-append-oracle.json"
    ))
    .unwrap();
    cases
        .iter()
        .find(|c| {
            c["decorated"] == decorated
                && c["style"] == "default"
                && c["mode"] == "default"
                && c["before"] == false
                && c["headers"] == serde_json::json!(["A", "B"])
                && !c["rows"].as_array().unwrap().is_empty()
                && c["rows"][0][0] == "x"
        })
        .unwrap()["steps"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["output"]
        .as_str()
        .unwrap()
        .to_owned()
}
#[test]
fn section_handles_cannot_cross_outputs_and_bound_style_retains_own_configuration() {
    let input = openrust::console::CommandDefinition::from_signature("sections")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let handle = output.section().handle();
    assert!(Output::default().section_output(&handle).is_err());
    output.set_verbosity(OutputVerbosity::Silent);
    {
        let mut section = output.section_output(&handle).unwrap();
        section.set_verbosity(OutputVerbosity::Verbose);
        let mut style = section.style(&input);
        assert!(style.is_verbose());
        assert!(!style.is_silent());
        style.writeln("accepted", OutputOptions::default()).unwrap();
        style.new_line(1).unwrap();
    }
    assert_eq!(output.stdout, "accepted\n\n");
    let mut another = Output::default();
    let mut io = input.io(&mut another);
    let mut table = Table::new(io.get_output()).unwrap();
    table.set_headers(["H"]);
    assert!(table.append_row(["rejected"]).is_err());
    table.render().unwrap();
    assert!(!another.stdout.contains("rejected"));
}
#[test]
#[ignore = "由真实Artisan父消费者驱动"]
fn sections_cli_fixture() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut app = Application::configure(std::env::var_os("TABLE_SECTIONS_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    runtime.block_on(app.boot()).unwrap();
    let args = [
        "tables:sections".into(),
        std::env::var_os("TABLE_SECTIONS_MODE").unwrap(),
        "--no-ansi".into(),
    ];
    std::process::exit(runtime.block_on(app.handle_command(&args)).unwrap());
}
#[test]
fn real_artisan_section_append_and_nonsection_error_exit() {
    for mode in ["ok", "bad"] {
        let root = tempfile::tempdir().unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sections_cli_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("TABLE_SECTIONS_ROOT", root.path())
            .env("TABLE_SECTIONS_MODE", mode)
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(if mode == "ok" { 0 } else { 1 }),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        assert_eq!(
            stdout.strip_prefix("\nrunning 1 test\n").unwrap(),
            if mode == "ok" {
                formal_output(false)
            } else {
                String::new()
            }
        );
        if mode == "bad" {
            assert!(String::from_utf8_lossy(&child.stderr).contains("分区"));
        } else {
            assert!(child.stderr.is_empty());
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, mode) in [("/sections", "ok"), ("/sections/error", "bad")] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            Artisan::call(
                "tables:sections",
                &[mode.into(), "--no-ansi".into()],
                &mut output,
            )
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                serde_json::json!({"table":output.stdout}),
            )
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_section_append_and_invalid_output_recover() {
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
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register).web(routes))
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
        .unwrap();
    });
    for (path, status) in [
        ("/sections", 200),
        ("/sections/error", 500),
        ("/sections", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert_eq!(
                response.json::<Value>().await.unwrap()["table"],
                formal_output(false)
            );
        }
    }
    server.abort();
    let _ = server.await;
}
