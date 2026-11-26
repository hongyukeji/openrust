use openrust::console::{
    AsOutput, BlockOptions, CommandDefinition, ConsoleError, ConsoleStyle, FormatterStyle, Input,
    Output, OutputFormatterStyle, OutputOptions, OutputStyle, OutputVerbosity, ProgressBar,
    contracts::NewLineAware,
};
use serde_json::{Value, json};
use std::sync::Arc;
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/error-style-oracle.json"
    ))
    .unwrap()
}
trait SceneStyle: AsOutput {
    fn title(&mut self, message: &str) -> Result<(), ConsoleError>;
    fn write(&mut self, message: &str, newline: bool) -> Result<(), ConsoleError>;
    fn error_style(&mut self) -> ConsoleStyle<'_>;
    fn properties(&self) -> Value;
    fn counts(&self) -> Value;
}
macro_rules! scene_style{($ty:ident,$count:expr)=>{impl SceneStyle for $ty<'_>{fn title(&mut self,message:&str)->Result<(),ConsoleError>{$ty::title(self,message)}fn write(&mut self,message:&str,newline:bool)->Result<(),ConsoleError>{$ty::write(self,message,newline,OutputOptions::default())}fn error_style(&mut self)->ConsoleStyle<'_>{self.get_error_style()}fn properties(&self)->Value{json!({"verbosity":self.get_verbosity()as u16,"decorated":self.is_decorated(),"probe":self.get_formatter().has_style("probe")})}fn counts(&self)->Value{($count)(self)}}}}
scene_style!(OutputStyle, |s: &OutputStyle<'_>| json!([
    s.new_lines_written(),
    s.new_line_written()
]));
scene_style!(ConsoleStyle, |_: &ConsoleStyle<'_>| Value::Null);
fn scene<S: SceneStyle>(original: &mut S, c: &Value) -> Value {
    original.title("Main").unwrap();
    original.write("tail", false).unwrap();
    let mut error = original.error_style();
    let aware = error.as_output().as_new_line_aware().is_some();
    let mut info = json!({"new_line_aware":aware,"is_laravel_style":false,"failure":false});
    match c["variant"].as_u64().unwrap() {
        1 => {
            error.set_verbosity(OutputVerbosity::Verbose);
            error.set_decorated(!c["decorated"].as_bool().unwrap());
            let style: FormatterStyle =
                Arc::new(OutputFormatterStyle::new(Some("magenta"), None, &[]).unwrap());
            error.get_formatter().set_style("probe", style);
            error
                .writeln("<probe>err</probe>", OutputOptions::default())
                .unwrap();
        }
        2 => {
            let result = error.title("<fg=bogus>broken</>");
            assert!(result.is_err());
            info["failure"] = json!(result.is_err());
            error.text("Recovered").unwrap();
        }
        _ => unreachable!(),
    }
    info["error_properties"] = SceneStyle::properties(&error);
    drop(error);
    info["original_properties"] = original.properties();
    if c["variant"] == 2 {
        original.write("end", true).unwrap();
    }
    info
}
async fn async_scene<S: SceneStyle>(original: &mut S, c: &Value) -> Value {
    if c["variant"] != 0 {
        return scene(original, c);
    }
    original.title("Main").unwrap();
    original.write("tail", false).unwrap();
    let mut error = original.error_style();
    let aware = error.as_output().as_new_line_aware().is_some();
    let mut info = json!({"new_line_aware":aware,"is_laravel_style":false,"failure":false});
    error.text("Error text").unwrap();
    error.error("Failure").unwrap();
    error.new_line(2).unwrap();
    error
        .write(&["a", "b"], false, OutputOptions::default())
        .unwrap();
    error.writeln("done", OutputOptions::default()).unwrap();
    {
        let mut table = error.create_table().unwrap();
        table.set_headers(["K", "V"]).set_rows([["x", "y"]]);
        info["append_error"] = json!(table.append_row(["more", "data"]).is_err());
        table.render().unwrap();
    }
    info["ask"] = json!(error.ask_with("Name", Some("Fallback")).await.unwrap());
    error.progress_start(2).unwrap();
    error.progress_advance().unwrap();
    error.progress_finish().unwrap();
    error.get_error_style().note("Nested").unwrap();
    error.text("Recovered").unwrap();
    info["error_properties"] = SceneStyle::properties(&error);
    drop(error);
    info["original_properties"] = original.properties();
    info
}
fn chain<'a>(
    out: &'a mut (dyn AsOutput + Send),
    input: &'a Input,
    kinds: &'a [u8],
    c: &'a Value,
) -> openrust::console::ConsoleFuture<'a, (Value, Vec<Value>)> {
    Box::pin(async move {
        if kinds[0] == b'L' {
            let mut layer = OutputStyle::new(input, out);
            branch(&mut layer, input, &kinds[1..], c).await
        } else {
            let mut layer = ConsoleStyle::new(input, out);
            branch(&mut layer, input, &kinds[1..], c).await
        }
    })
}
async fn branch<S: SceneStyle + Send>(
    layer: &mut S,
    input: &Input,
    rest: &[u8],
    c: &Value,
) -> (Value, Vec<Value>) {
    let (mut info, mut layers) = if rest.is_empty() {
        (async_scene(layer, c).await, Vec::new())
    } else {
        chain(layer, input, rest, c).await
    };
    layers.insert(0, layer.counts());
    info["layers"] = json!(layers);
    (info, layers)
}
fn configure_formats() {
    for name in [
        "normal",
        "verbose",
        "very_verbose",
        "debug",
        "normal_nomax",
        "verbose_nomax",
        "very_verbose_nomax",
        "debug_nomax",
    ] {
        ProgressBar::set_format_definition(name, "%current%/%max%").unwrap();
    }
}
#[test]
fn fixed_source_error_styles_preserve_target_and_base_class_boundaries() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "error_style_driver", "--ignored", "--nocapture"])
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
#[ignore = "由父消费者隔离尺寸与应用定义"]
fn error_style_driver() {
    use openrust::{
        foundation::Application,
        support::facades::{App, Facade as _},
    };
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into()).create().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(app.boot()).unwrap();
    rt.block_on(App::set_facade_application(Some(&app), async {
        configure_formats();
        let mut failures = Vec::new();
        for (index, c) in cases().iter().enumerate() {
            let mut input = CommandDefinition::from_signature("errors")
                .unwrap()
                .parse(&[])
                .unwrap();
            input.set_interactive(false);
            let mut output = Output::default();
            output.set_verbosity(if c["level"] == 16 {
                OutputVerbosity::Quiet
            } else {
                OutputVerbosity::Normal
            });
            output.set_decorated(c["decorated"].as_bool().unwrap());
            let mut section;
            let target: &mut (dyn AsOutput + Send) = if c["section"] == true {
                section = output.section();
                &mut section
            } else {
                &mut output
            };
            let (info, _) = chain(target, &input, c["shape"].as_str().unwrap().as_bytes(), c).await;
            let actual = json!({"metadata":info,"output":output.stdout});
            let expected = json!({"metadata":c["metadata"],"output":c["output"]});
            if actual != expected {
                failures.push(format!("case{index}: actual{actual},expected{expected}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }));
}

fn console_cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/error-style-console-oracle.json"
    ))
    .unwrap()
}
fn register_console() -> Result<(), ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:errors {case}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                configure_formats();
                let raw = input.argument("case").unwrap();
                let c: Value = serde_json::from_str(raw.as_str().unwrap()).unwrap();
                let mut reading = input.clone();
                reading.set_interactive(false);
                let (info, _) = chain(
                    output,
                    &reading,
                    c["shape"].as_str().unwrap().as_bytes(),
                    &c,
                )
                .await;
                eprintln!("ERROR_STYLE_META{info}");
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
fn actual_console_error_style_streams_and_mixed_layer_counters_match_source() {
    for (index, c) in console_cases().iter().enumerate() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "error_style_cli_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("ERROR_STYLE_CASE", c.to_string())
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(0),
            "{index}:{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let stderr = String::from_utf8(child.stderr).unwrap();
        let stdout = stdout.split_once("ERROR_STYLE_BEGIN").unwrap().1;
        let (stderr, metadata) = stderr.rsplit_once("ERROR_STYLE_META").unwrap();
        assert_eq!(stdout, c["stdout"].as_str().unwrap(), "stdout{index}");
        assert_eq!(stderr, c["stderr"].as_str().unwrap(), "stderr{index}");
        let metadata: Value = serde_json::from_str(metadata).unwrap();
        assert_eq!(metadata, c["metadata"], "metadata{index}");
    }
}
#[test]
#[ignore = "由父消费者实际启动独立Artisan并比较stdout/stderr"]
fn error_style_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    use std::io::Write as _;
    let c: Value = serde_json::from_str(&std::env::var("ERROR_STYLE_CASE").unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register_console))
        .create()
        .unwrap();
    let mut args = vec![
        "styles:errors".into(),
        c.to_string().into(),
        "--no-interaction".into(),
        if c["decorated"] == true {
            "--ansi".into()
        } else {
            "--no-ansi".into()
        },
    ];
    if c["level"] == 16 {
        args.push("--quiet".into());
    }
    print!("ERROR_STYLE_BEGIN");
    std::io::stdout().flush().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let status = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(status);
}

#[test]
fn all_fifty_one_base_helpers_are_actual_public_consumers() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "base_api_driver", "--ignored", "--nocapture"])
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
fn store(values: &mut Value, name: &str, result: Result<Value, ConsoleError>) {
    values[name] = match result {
        Ok(value) => json!({"value":value,"error":false}),
        Err(_) => json!({"value":Value::Null,"error":true}),
    };
}
#[test]
#[ignore = "由父消费者隔离尺寸和应用进度定义"]
fn base_api_driver() {
    use openrust::{
        console::{CompletionCallback, DefinitionListItem, Question, QuestionCallback},
        foundation::Application,
        support::facades::{App, Facade as _},
    };
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into()).create().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(app.boot()).unwrap();
    rt.block_on(App::set_facade_application(Some(&app),async{configure_formats();let mut input=CommandDefinition::from_signature("base-api").unwrap().parse(&[]).unwrap();input.set_interactive(false);let mut output=Output::default();let mut style=ConsoleStyle::new(&input,&mut output);let mut flags=Vec::new();let mut values=json!({});let trace=Arc::new(Mutex::new(Vec::<String>::new()));for level in [OutputVerbosity::Silent,OutputVerbosity::Quiet,OutputVerbosity::Normal,OutputVerbosity::Verbose,OutputVerbosity::VeryVerbose,OutputVerbosity::Debug]{style.set_verbosity(level);flags.push(json!({"verbosity":style.get_verbosity()as u16,"silent":style.is_silent(),"quiet":style.is_quiet(),"verbose":style.is_verbose(),"very_verbose":style.is_very_verbose(),"debug":style.is_debug()}));}style.set_verbosity(OutputVerbosity::Normal);style.set_decorated(false);values["decorated"]=json!(style.is_decorated());style.set_formatter(style.get_formatter());values["formatter"]=json!(style.get_formatter().has_style("info"));style.write("write",false,OutputOptions::default()).unwrap();style.writeln("line",OutputOptions::default()).unwrap();let log=trace.clone();style.write_iter(["one","two"].into_iter().inspect(move|v|log.lock().unwrap().push(format!("write:{v}"))),false,OutputOptions::default()).unwrap();let log=trace.clone();style.writeln_iter(["one","two"].into_iter().inspect(move|v|log.lock().unwrap().push(format!("writeln:{v}"))),OutputOptions::default()).unwrap();store(&mut values,"ask_question",style.ask_question(&Question::new("Question",Some(json!("Answer"))).unwrap()).await);style.new_line(1).unwrap();style.progress_start(3).unwrap();style.progress_advance().unwrap();style.progress_advance_by(1).unwrap();style.progress_finish().unwrap();style.title("Title").unwrap();style.section("Section").unwrap();style.listing(["a","b"]).unwrap();style.text(&["line1","line2"]).unwrap();style.block("block",BlockOptions{kind:Some("BUILD"),..BlockOptions::default()}).unwrap();style.comment("comment").unwrap();style.success("success").unwrap();style.error("error").unwrap();style.warning("warning").unwrap();style.note("note").unwrap();style.info("info").unwrap();style.caution("caution").unwrap();{let mut table=style.create_table().unwrap();table.set_headers(["K","V"]).set_rows([["x","y"]]).render().unwrap();}style.table(["K","V"],[["x","y"]]).unwrap();style.horizontal_table(["K","V"],[["x","y"]]).unwrap();style.definition_list([DefinitionListItem::from("Profile"),DefinitionListItem::entry("Name","Ada")]).unwrap();store(&mut values,"ask",style.ask("Name").await.map(|v|json!(v)));store(&mut values,"ask_with",style.ask_with("Name",Some("Guest")).await.map(|v|json!(v)));store(&mut values,"confirm",style.confirm("Continue").await.map(|v|json!(v)));store(&mut values,"confirm_with",style.confirm_with("Continue",false).await.map(|v|json!(v)));store(&mut values,"ask_with_validator",style.ask_with_validator("Name",Some("Guest"),Some(QuestionCallback::sync(|v|Ok(json!({"value":v}))))).await);store(&mut values,"ask_hidden",style.ask_hidden("Token").await.map(|v|json!(v)));store(&mut values,"ask_hidden_with",style.ask_hidden_with("Token",Some(QuestionCallback::sync(|v|Ok(json!({"value":v}))))).await);let completion=Arc::new(AtomicBool::new(false));let called=completion.clone();let callback=CompletionCallback::new(move |_|{called.store(true,Ordering::Relaxed);Box::pin(async{Ok(vec!["JSON".into()])})});store(&mut values,"ask_with_completion",style.ask_with_completion("Format",callback,Some("JSON")).await.map(|v|json!(v)));store(&mut values,"choice",style.choice("Color",["red","green"]).await);store(&mut values,"choice_with",style.choice_with("Color",["red","green"],Some(json!("green")),false).await);store(&mut values,"choice_pairs_with",style.choice_pairs_with("Color",[("r","red"),("g","green")],Some(json!("green")),false).await);{let mut bar=style.create_progress_bar(2).unwrap();bar.start().unwrap();bar.advance().unwrap();bar.finish().unwrap();}style.new_line(1).unwrap();values["progress_iterate"]=json!(style.progress_iterate(["a","b"]).collect::<Result<Vec<_>,_>>().unwrap());values["progress_iterate_with_max"]=json!(style.progress_iterate_with_max(["c","d"],Some(5)).collect::<Result<Vec<_>,_>>().unwrap());let metadata=json!({"values":values,"flags":flags,"trace":*trace.lock().unwrap(),"completion_called":completion.load(Ordering::Relaxed)});drop(style);let actual=json!({"metadata":metadata,"output":output.stdout});let expected:Value=serde_json::from_str(include_str!("../../../.agents/project/audits/artisan-tutorial/base-style-api-oracle.json")).unwrap();assert_eq!(actual,expected);
}));
}

#[path = "support/style_interface.rs"]
mod style_interface_consumer;
fn base_interface_cases() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/base-style-interface-oracle.json"
    ))
    .unwrap()
}
async fn run_base_interface(style: &mut ConsoleStyle<'_>, c: &Value, dynamic: bool) -> Value {
    let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
    let steps = if dynamic {
        style_interface_consumer::exercise(
            style as &mut dyn openrust::console::StyleInterface,
            c,
            trace.clone(),
        )
        .await
    } else {
        style_interface_consumer::exercise(style, c, trace.clone()).await
    };
    let selected = style.as_output();
    let capability = selected
        .as_new_line_aware()
        .map(|v| json!([v.new_lines_written(), v.new_line_written()]));
    json!({"steps":steps,"output":selected.stdout,"trace":*trace.lock().unwrap(),"capability":capability})
}
#[test]
fn base_style_interface_all_entries_run_as_generic_and_dynamic_without_counting() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "base_interface_driver",
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
#[ignore = "由父消费者隔离尺寸和基础接口进度定义"]
fn base_interface_driver() {
    use openrust::{
        console::ConsoleInputStream,
        foundation::Application,
        support::facades::{App, Facade as _},
    };
    unsafe {
        std::env::set_var("COLUMNS", "80");
    }
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into()).create().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(app.boot()).unwrap();
    rt.block_on(App::set_facade_application(Some(&app),async{configure_formats();let mut failures=Vec::new();for dynamic in [false,true]{for(index,c)in base_interface_cases().iter().enumerate(){let mut input=CommandDefinition::from_signature("base-interface").unwrap().parse(&[]).unwrap();input.set_interactive(c["interactive"].as_bool().unwrap());input.set_stream(Some(ConsoleInputStream::new(tokio::io::BufReader::new(std::io::Cursor::new(b"Ada\nsample\n\n\n".to_vec())))));let mut output=Output::default();output.set_verbosity(if c["level"]==16{OutputVerbosity::Quiet}else{OutputVerbosity::Normal});output.set_decorated(c["decorated"].as_bool().unwrap());let mut section;let target:&mut dyn AsOutput=if c["section"]==true{section=output.section();&mut section}else{&mut output};let mut first=ConsoleStyle::new(&input,target);let mut actual=if c["depth"]==2{let mut second=ConsoleStyle::new(&input,&mut first);let mut value=run_base_interface(&mut second,c,dynamic).await;let inner=value.as_object_mut().unwrap().remove("capability").unwrap();drop(second);let selected=first.as_output();let outer=selected.as_new_line_aware().map(|v|json!([v.new_lines_written(),v.new_line_written()]));value["counts"]=json!([outer,inner]);value}else{let mut value=run_base_interface(&mut first,c,dynamic).await;let cap=value.as_object_mut().unwrap().remove("capability").unwrap();value["counts"]=json!([cap]);value};actual["interactive_after"]=json!(input.is_interactive());let steps:Vec<Value>=c["steps"].as_array().unwrap().iter().map(|step|json!({"method":step["method"],"value":step["value"],"failed":step["error"].is_string()})).collect();let expected=json!({"steps":steps,"output":c["output"],"trace":c["trace"],"counts":c["counts"],"interactive_after":c["interactive_after"]});if actual!=expected{failures.push(format!("case{index}/{dynamic}: actual{actual},expected{expected}"));}}}assert!(failures.is_empty(),"{}",failures.join("\n"));}));
}

fn register_stream() -> Result<(), ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:errors-stream {--newline}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let gate =
                    std::path::PathBuf::from(std::env::var_os("ERROR_STYLE_STREAM_GATE").unwrap());
                let mut original = OutputStyle::new(input, output);
                let mut errors = original.get_error_style();
                let messages = std::iter::once("FIRST").chain(std::iter::once_with(move || {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
                    while !gate.exists() && std::time::Instant::now() < deadline {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    assert!(gate.exists(), "流式首条消息未在读取下一项前输出");
                    "SECOND"
                }));
                if input.option("newline").and_then(|value| value.as_bool()) == Some(true) {
                    errors.writeln_iter(messages, OutputOptions::default())?;
                } else {
                    errors.write_iter(messages, false, OutputOptions::default())?;
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
fn actual_base_iterable_output_is_flushed_before_requesting_the_next_item() {
    use std::io::Read as _;
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/error-style-stream-oracle.json"
    ))
    .unwrap();
    for c in cases {
        let root = tempfile::tempdir().unwrap();
        let gate = root.path().join("release");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "error_style_stream_driver",
                "--ignored",
                "--nocapture",
            ])
            .env("ERROR_STYLE_STREAM_ROOT", root.path())
            .env("ERROR_STYLE_STREAM_GATE", &gate)
            .env("ERROR_STYLE_STREAM_NEWLINE", c["newline"].to_string())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut sent = false;
            loop {
                let mut buffer = [0_u8; 1024];
                let size = stderr.read(&mut buffer).unwrap();
                if size == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..size]);
                if !sent && bytes.windows(5).any(|w| w == b"FIRST") {
                    sender.send(()).unwrap();
                    sent = true;
                }
            }
            bytes
        });
        if let Err(error) = receiver.recv_timeout(std::time::Duration::from_secs(12)) {
            let _ = child.kill();
            let _ = child.wait();
            let bytes = reader.join().unwrap();
            panic!(
                "未在门闩释放前输出首项：{error:?} {}",
                String::from_utf8_lossy(&bytes)
            );
        }
        std::fs::write(&gate, "go").unwrap();
        let result = child.wait_with_output().unwrap();
        let stderr = String::from_utf8(reader.join().unwrap()).unwrap();
        assert_eq!(result.status.code(), Some(0), "{stderr}");
        assert_eq!(stderr, c["stderr"].as_str().unwrap());
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert_eq!(
            stdout.split_once("ERROR_STREAM_BEGIN").unwrap().1,
            c["stdout"].as_str().unwrap()
        );
    }
}
#[test]
#[ignore = "由实际二进制父消费者使用门闩观察流式写入"]
fn error_style_stream_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    use std::io::Write as _;
    let app = Application::configure(std::env::var_os("ERROR_STYLE_STREAM_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register_stream))
        .create()
        .unwrap();
    let mut args = vec!["styles:errors-stream".into(), "--no-ansi".into()];
    if std::env::var("ERROR_STYLE_STREAM_NEWLINE").unwrap() == "true" {
        args.push("--newline".into());
    }
    print!("ERROR_STREAM_BEGIN");
    std::io::stdout().flush().unwrap();
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&args))
        .unwrap();
    std::process::exit(status);
}

fn register_http() -> Result<(), ConsoleError> {
    use openrust::{console::AsyncCommandCallback, support::facades::Artisan};
    Artisan::command_with(
        "styles:errors-http {shape} {--fatal}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                if input.option("fatal").and_then(|value| value.as_bool()) == Some(true) {
                    let mut original = OutputStyle::new(input, output);
                    original.get_error_style().title("<fg=bogus>broken</>")?;
                    return Ok(0);
                }
                configure_formats();
                let shape = input.argument("shape").unwrap();
                let shape = shape.as_str().unwrap();
                let c =
                    json!({"shape":shape,"variant":0,"decorated":false,"level":32,"section":false});
                let mut reading = input.clone();
                reading.set_interactive(false);
                let (info, _) = chain(output, &reading, shape.as_bytes(), &c).await;
                output
                    .stderr
                    .push_str(&format!("ERROR_STYLE_HTTP_META{info}\n"));
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    for (shape, path, fatal) in [
        ("L", "/errors/direct", false),
        ("LBLL", "/errors/mixed", false),
        ("L", "/errors/failure", true),
    ] {
        openrust::support::facades::Route::get(path, move || async move {
            let mut output = Output::default();
            let mut args = vec![shape.into(), "--no-ansi".into(), "--no-interaction".into()];
            if fatal {
                args.push("--fatal".into());
            }
            openrust::support::facades::Artisan::call("styles:errors-http", &args, &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            let metadata: Value = serde_json::from_str(
                output
                    .stderr
                    .trim()
                    .strip_prefix("ERROR_STYLE_HTTP_META")
                    .unwrap(),
            )
            .unwrap();
            openrust::http::Response::json(
                http::StatusCode::OK,
                json!({"output":output.stdout,"metadata":metadata}),
            )
        })?;
    }
    Ok(())
}
fn expected_http(shape: &str) -> Value {
    let c = cases()
        .into_iter()
        .find(|c| {
            c["shape"] == shape
                && c["section"] == false
                && c["variant"] == 0
                && c["decorated"] == false
                && c["level"] == 32
        })
        .unwrap();
    json!({"output":c["output"],"metadata":c["metadata"]})
}
#[tokio::test]
async fn real_http_error_style_failures_recover_and_keep_mixed_styles_isolated() {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing},
    };
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
    for (path, status, shape) in [
        ("/errors/direct", 200, "L"),
        ("/errors/mixed", 200, "LBLL"),
        ("/errors/failure", 500, "L"),
        ("/errors/direct", 200, "L"),
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
            assert_eq!(body, expected_http(shape));
        } else {
            assert!(body.is_object());
        }
    }
    let mut tasks = Vec::new();
    for n in 0..12 {
        tasks.push(tokio::spawn(async move {
            let (shape, path) = if n % 2 == 0 {
                ("L", "direct")
            } else {
                ("LBLL", "mixed")
            };
            let body: Value = reqwest::get(format!("http://{address}/errors/{path}"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(body, expected_http(shape));
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
#[test]
fn actual_artisan_base_error_style_propagates_the_failure_exit_code() {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "error_style_fatal_cli_driver",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(1),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
#[test]
#[ignore = "由实际父进程核对错误样式的OS失败退出码"]
fn error_style_fatal_cli_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register_http))
        .create()
        .unwrap();
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&[
            "styles:errors-http".into(),
            "L".into(),
            "--fatal".into(),
            "--no-ansi".into(),
        ]))
        .unwrap();
    drop(root);
    std::process::exit(status);
}

fn register_error_live() -> Result<(), ConsoleError> {
    use openrust::{
        console::{AsyncCommandCallback, ChoiceKey, QuestionCallback, StyleInterface},
        support::facades::Artisan,
    };
    Artisan::command_with(
        "styles:error-live",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let mut reading = input.clone();
                reading.set_interactive(true);
                let metadata = {
                    let mut original = OutputStyle::new(&reading, output);
                    let mut errors = original.get_error_style();
                    let dynamic: &mut dyn StyleInterface = &mut errors;
                    let validator = QuestionCallback::sync(|value| {
                        if value == "bad" {
                            return Err(ConsoleError::new("Try again"));
                        }
                        Ok(json!({"name":value.as_str().unwrap_or_default().to_uppercase()}))
                    });
                    let name = dynamic.ask("Name", Some("Guest"), Some(validator)).await?;
                    let secret = dynamic
                        .ask_hidden(
                            "Password",
                            Some(QuestionCallback::sync(|value| {
                                Ok(json!({"length":value.as_str().unwrap_or_default().len()}))
                            })),
                        )
                        .await?;
                    let confirm = dynamic.confirm("Continue", true).await?;
                    let choice = dynamic
                        .choice(
                            "Color",
                            vec![
                                (ChoiceKey::from(0_i64), json!("red")),
                                (ChoiceKey::from(1_i64), json!("green")),
                            ],
                            Some(json!("green")),
                        )
                        .await?;
                    json!({"name":name,"secret":secret,"confirm":confirm,"choice":choice})
                };
                output
                    .stderr
                    .push_str(&format!("STYLE_QUESTIONS_META:{metadata}\n"));
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由真实PTY父消费者验收基础错误样式的stderr隐藏输入及取消"]
fn error_style_live_driver() {
    use openrust::foundation::{Application, configuration::Routing};
    let app = Application::configure(std::env::var_os("STYLE_QUESTIONS_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register_error_live))
        .create()
        .unwrap();
    let status = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(app.handle_command(&["styles:error-live".into(), "--no-ansi".into()]))
        .unwrap();
    if let Some(path) = std::env::var_os("STYLE_QUESTIONS_RELEASE") {
        eprintln!("STYLE_QUESTIONS_STATUS:{status}");
        let path = std::path::PathBuf::from(path);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    std::process::exit(status);
}
