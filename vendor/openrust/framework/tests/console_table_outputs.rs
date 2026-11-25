use openrust::console::{
    AsOutput, CommandDefinition, Output, OutputReference, OutputStyle, OutputVerbosity, Table,
    contracts::NewLineAware,
};
use serde_json::{Value, json};
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-output-oracle.json"
    ))
    .unwrap()
}
fn parent_counts(output: &mut OutputReference<'_>) -> Vec<Value> {
    let mut result = Vec::new();
    if let Some(view) = output.as_new_line_aware() {
        result.push(json!([view.new_lines_written(), view.new_line_written()]));
        result.extend(parent_counts(&mut output.get_output().unwrap()));
    }
    result
}
fn run_case(style: &mut OutputStyle<'_>, case: &Value) -> Value {
    let (info, append_error) = {
        let mut table = match case["mode"].as_str().unwrap() {
            "factory" => style.create_table().unwrap(),
            "style" => Table::new(style).unwrap(),
            "underlying" => Table::new(style.get_output()).unwrap(),
            _ => unreachable!(),
        };
        let info = json!({"header":table.get_style().get_cell_header_format(),"row":table.get_style().get_cell_row_format()});
        table.set_headers(["H", "V"]).set_rows([["x", "y"]]);
        if case["before"] == true {
            table.render().unwrap();
        }
        let error = table.append_row(["next", "row"]).is_err();
        table.render().unwrap();
        table.get_style_mut().set_cell_header_format("LOCAL %s");
        (info, error)
    };
    assert_eq!(append_error, case["append_error"].is_string());
    let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
    counts.extend(parent_counts(style.get_output()));
    counts.reverse();
    json!({"output":style.get_output().stdout,"counts":counts,"initial_header_format":info["header"],"initial_row_format":info["row"],"global_guide_header":Table::get_style_definition("symfony-style-guide").unwrap().get_cell_header_format(),"global_default_header":Table::get_style_definition("default").unwrap().get_cell_header_format()})
}
#[test]
fn constructors_and_style_factory_match_actual_source() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "table_outputs_driver",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
#[test]
#[ignore = "由父消费者隔离终端尺寸与应用命名样式"]
fn table_outputs_driver() {
    use openrust::{
        foundation::Application,
        support::facades::{App, Facade as _},
    };
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into()).create().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(app.boot()).unwrap();
    runtime.block_on(App::set_facade_application(Some(&app),async{
  let original=Table::get_style_definition("default").unwrap().clone();let guide=Table::get_style_definition("symfony-style-guide").unwrap().clone();
  let input=CommandDefinition::from_signature("table-outputs").unwrap().parse(&[]).unwrap();let mut failures=Vec::new();
  for(index,case)in cases().iter().enumerate(){
   Table::set_style_definition("default",original.clone()).unwrap();Table::set_style_definition("symfony-style-guide",guide.clone()).unwrap();
   if case["custom_guide"]==true{Table::get_style_definition("symfony-style-guide").unwrap().set_cell_header_format("BASE %s").set_cell_row_format("<comment>%s</comment>");}
   let mut output=Output::default();output.set_verbosity(if case["level"]==16{OutputVerbosity::Quiet}else{OutputVerbosity::Normal});output.set_decorated(case["decorated"].as_bool().unwrap());
   let mut section;let target:&mut dyn AsOutput=if case["section"]==true{section=output.section();&mut section}else{&mut output};
   let mut first=OutputStyle::new(&input,target);let actual=if case["depth"]==2{let mut second=OutputStyle::new(&input,&mut first);run_case(&mut second,case)}else{run_case(&mut first,case)};
   let expected=json!({"output":case["output"],"counts":case["counts"],"initial_header_format":case["initial_header_format"],"initial_row_format":case["initial_row_format"],"global_guide_header":case["global_guide_header"],"global_default_header":case["global_default_header"]});
   if actual!=expected{failures.push(format!("case {index}: actual {actual:?}, expected {expected:?}"));}
  }assert!(failures.is_empty(),"{}",failures.join("\n"));
 }));
}
fn render(table: &mut Table<'_>, append: bool) {
    table.set_headers(["H", "V"]).set_rows([["x", "y"]]);
    if append {
        table.append_row(["next", "row"]).unwrap();
    }
    table.render().unwrap();
}
fn expected(section: bool) -> String {
    cases()
        .into_iter()
        .find(|c| {
            c["mode"] == "underlying"
                && c["section"] == section
                && c["depth"] == 1
                && c["level"] == 32
                && c["decorated"] == false
                && c["before"] == false
                && c["custom_guide"] == false
        })
        .unwrap()["output"]
        .as_str()
        .unwrap()
        .to_owned()
}
#[test]
fn actual_root_section_reference_and_box_are_real_constructor_consumers() {
    let mut output = Output::default();
    render(&mut Table::new(&mut output).unwrap(), false);
    assert_eq!(output.stdout, expected(false));
    let mut output = Output::default();
    {
        let mut section = output.section();
        render(&mut Table::new(&mut section).unwrap(), true);
    }
    assert_eq!(output.stdout, expected(true));
    let mut output = Output::default();
    {
        let mut reference = output.as_output();
        render(&mut Table::new(&mut reference).unwrap(), false);
    }
    assert_eq!(output.stdout, expected(false));
    let mut output = Box::new(Output::default());
    render(&mut Table::new(&mut output).unwrap(), false);
    assert_eq!(output.stdout, expected(false));
}

fn console_cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/table-console-oracle.json"
    ))
    .unwrap()
}
fn console_scene(
    style: &mut OutputStyle<'_>,
    case: &Value,
) -> Result<Value, openrust::console::ConsoleError> {
    let append_error = {
        let mut table = match case["mode"].as_str().unwrap() {
            "factory" => style.create_table()?,
            "style" => Table::new(style)?,
            "underlying" => Table::new(style.get_output())?,
            _ => unreachable!(),
        };
        table.set_headers(["H", "V"]).set_rows([["x", "y"]]);
        if case["before"] == true {
            table.render()?;
        }
        let failed = table.append_row(["next", "row"]).is_err();
        table.render()?;
        failed
    };
    let mut counts = vec![json!([style.new_lines_written(), style.new_line_written()])];
    counts.extend(parent_counts(style.get_output()));
    counts.reverse();
    Ok(json!({"append_error":append_error,"counts":counts}))
}
fn register_console() -> Result<(), openrust::console::ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "tables:outputs",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let case: Value =
                    serde_json::from_str(&std::env::var("TABLE_OUTPUT_CASE").unwrap()).unwrap();
                let mut first = OutputStyle::new(input, output);
                let meta = if case["depth"] == 2 {
                    let mut second = OutputStyle::new(input, &mut first);
                    console_scene(&mut second, &case)?
                } else {
                    console_scene(&mut first, &case)?
                };
                eprintln!("TABLE_OUTPUT_META:{meta}");
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
fn actual_console_factory_sections_and_constructor_identity_match_source() {
    let root = tempfile::tempdir().unwrap();
    for (index, case) in console_cases().iter().enumerate() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "table_outputs_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("TABLE_OUTPUT_ROOT", root.path())
            .env("TABLE_OUTPUT_CASE", case.to_string())
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(0),
            "case {index}: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        let output = String::from_utf8(child.stdout).unwrap();
        let output = output.split_once("TABLE_OUTPUT_BEGIN\n").unwrap().1;
        assert_eq!(output, case["output"].as_str().unwrap(), "console {index}");
        let errors = String::from_utf8(child.stderr).unwrap();
        let meta: Value =
            serde_json::from_str(errors.trim().strip_prefix("TABLE_OUTPUT_META:").unwrap())
                .unwrap();
        assert_eq!(meta, case["meta"], "meta {index}");
    }
}
#[test]
#[ignore = "由实际ConsoleOutput父消费者驱动"]
fn table_outputs_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    use std::io::Write as _;
    let root = std::env::var_os("TABLE_OUTPUT_ROOT").unwrap();
    let app = Application::configure(root.into())
        .with_routing(Routing::default().commands(register_console))
        .create()
        .unwrap();
    let case: Value = serde_json::from_str(&std::env::var("TABLE_OUTPUT_CASE").unwrap()).unwrap();
    let mut args = vec![
        "tables:outputs".into(),
        if case["decorated"] == true {
            "--ansi".into()
        } else {
            "--no-ansi".into()
        },
    ];
    if case["level"] == 16 {
        args.push("--quiet".into());
    }
    println!("TABLE_OUTPUT_BEGIN");
    std::io::stdout().flush().unwrap();
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&args))
        .unwrap();
    std::process::exit(status);
}
#[test]
fn standalone_style_factory_borrows_only_output_and_keeps_no_newline_side_effect() {
    let input = CommandDefinition::from_signature("table-factory")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut style = OutputStyle::new(&input, &mut output);
    {
        let mut table = style.create_table().unwrap();
        assert_eq!(
            table.get_style().get_cell_header_format(),
            "<info>%s</info>"
        );
        table
            .set_headers(["H", "V"])
            .set_rows([["x", "y"]])
            .render()
            .unwrap();
    }
    assert_eq!(style.new_lines_written(), 1);
    assert!(!style.new_line_written());
    assert!(!output.stdout.is_empty());
}

fn register_http() -> Result<(), openrust::console::ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "tables:factory {--invalid}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut style = OutputStyle::new(input, output);
                if input.option("invalid").and_then(|value| value.as_bool()) == Some(true) {
                    Table::new(&mut style)?.append_row(["rejected"])?;
                    return Ok(0);
                }
                {
                    let mut table = style.create_table()?;
                    table
                        .set_headers(["H", "V"])
                        .set_rows([["x", "y"]])
                        .render()?;
                    table.get_style_mut().set_cell_header_format("LOCAL %s");
                }
                assert_eq!(style.new_lines_written(), 1);
                assert!(!style.new_line_written());
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (path, invalid) in [("/table-outputs", false), ("/table-outputs/error", true)] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let mut args = vec!["--no-ansi".into()];
            if invalid {
                args.push("--invalid".into());
            }
            openrust::support::facades::Artisan::call("tables:factory", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            let header = Table::get_style_definition("symfony-style-guide")
                .unwrap()
                .get_cell_header_format();
            openrust::http::Response::json(
                http::StatusCode::OK,
                json!({"output":output.stdout,"guide_header":header}),
            )
        })?;
    }
    Ok(())
}
fn expected_factory() -> Value {
    let case = cases()
        .into_iter()
        .find(|c| {
            c["mode"] == "factory"
                && c["depth"] == 1
                && c["section"] == false
                && c["decorated"] == false
                && c["level"] == 32
                && c["before"] == false
                && c["custom_guide"] == false
        })
        .unwrap();
    json!({"output":case["output"],"guide_header":case["global_guide_header"]})
}
#[tokio::test]
async fn real_http_factory_rejection_recovery_and_concurrent_clone_isolation() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing},
    };
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register_http).web(routes))
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
    for (path, status) in [
        ("/table-outputs", 200),
        ("/table-outputs/error", 500),
        ("/table-outputs", 200),
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert!(
            response.headers()[reqwest::header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let body: Value = response.json().await.unwrap();
        if status == 200 {
            assert_eq!(body, expected_factory());
        } else {
            assert!(body.is_object());
        }
    }
    let mut tasks = Vec::new();
    for _ in 0..12 {
        tasks.push(tokio::spawn(async move {
            let response = reqwest::get(format!("http://{address}/table-outputs"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.json::<Value>().await.unwrap(), expected_factory());
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    let _ = server.await;
}
