use openrust::{
    console::{AnsiColorMode, Color, ConsoleError, Input, Output, OutputFormat, Terminal},
    foundation::{Application, configuration::Routing},
    support::facades::Artisan,
};
use serde_json::{Value, json};
fn modes() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/color-oracle.json"
    ))
    .unwrap()
}
fn terminals() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/terminal-oracle.json"
    ))
    .unwrap()
}
fn mode(name: &str) -> AnsiColorMode {
    match name {
        "Ansi4" => AnsiColorMode::Ansi4,
        "Ansi8" => AnsiColorMode::Ansi8,
        "Ansi24" => AnsiColorMode::Ansi24,
        _ => panic!("未知颜色模式"),
    }
}
fn register() -> Result<(), ConsoleError> {
    #[cfg(unix)]
    Artisan::command_with("terminal:recovery", |input: &Input, output: &mut Output| {
        let terminal = Terminal::new();
        let before = [terminal.get_width(), terminal.get_height()];
        let file = std::fs::File::open(std::env::var_os("TERMINAL_DEVICE").expect("专用终端设备"))
            .map_err(ConsoleError::from_source)?;
        rustix::stdio::dup2_stdin(&file).map_err(ConsoleError::from_source)?;
        let after = [terminal.get_width(), terminal.get_height()];
        input.io(output).get_output().write(
            json!({"before":before,"after":after}).to_string().as_str(),
            false,
            OutputFormat::Raw,
        )?;
        Ok(0)
    })?;
    Artisan::command_with("terminal:probe", |input: &Input, output: &mut Output| {
        let t = Terminal::new();
        let initial = Terminal::get_color_mode();
        let color = Color::new("#fff", "", &[])?;
        Terminal::set_color_mode(Some(AnsiColorMode::Ansi4));
        let forced = Terminal::get_color_mode();
        let old = color.apply("x");
        let fresh = Color::new("#fff", "", &[])?.apply("x");
        Terminal::set_color_mode(None);
        let restored = Terminal::get_color_mode();
        assert_eq!(Terminal::DEFAULT_COLOR_MODE, AnsiColorMode::Ansi4);
        let result = json!({"mode":format!("{initial:?}"),"forced":format!("{forced:?}"),"restored":format!("{restored:?}"),"old":old,"fresh":fresh,"width":t.get_width(),"height":t.get_height(),"stty":Terminal::has_stty_available()});
        input.io(output).get_output().write(
            result.to_string().as_str(),
            false,
            OutputFormat::Raw,
        )?;
        Ok(0)
    })?;
    Artisan::command_with("color:probe", |input: &Input, output: &mut Output| {
        let data: Value = serde_json::from_str(&std::env::var("COLOR_CASE").unwrap()).unwrap();
        let selected = mode(data["mode"].as_str().unwrap());
        Terminal::set_color_mode(Some(selected));
        let mut io = input.io(output);
        for case in data["colors"].as_array().unwrap() {
            let args = case["args"].as_array().unwrap();
            let options: Vec<&str> = args[2]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let color = Color::new(
                args[0].as_str().unwrap(),
                args[1].as_str().unwrap(),
                &options,
            );
            assert_eq!(color.is_err(), case["error"].as_bool().unwrap(), "{case}");
            if let Ok(color) = color {
                assert_eq!(color.set(), case["set"].as_str().unwrap(), "{case}");
                assert_eq!(color.unset(), case["unset"].as_str().unwrap(), "{case}");
                assert_eq!(color.apply(""), case["empty"].as_str().unwrap(), "{case}");
                let rendered = color.apply("sample");
                assert_eq!(rendered, case["output"].as_str().unwrap(), "{case}");
                io.get_output()
                    .write(rendered.as_str(), false, OutputFormat::Raw)?;
            }
        }
        for case in data["hexes"].as_array().unwrap() {
            let actual =
                selected.convert_from_hex_to_ansi_color_code(case["hex"].as_str().unwrap());
            assert_eq!(actual.is_err(), case["error"].as_bool().unwrap(), "{case}");
            if let Ok(actual) = actual {
                assert_eq!(actual, case["output"].as_str().unwrap(), "{case}");
            }
        }
        assert_eq!(Color::default().apply("plain"), "plain");
        Ok(0)
    })?;
    Ok(())
}
#[test]
#[ignore = "由独立环境的实际Artisan父消费者驱动"]
fn color_terminal_cli_fixture() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let root = std::path::PathBuf::from(std::env::var_os("COLOR_ROOT").unwrap());
    let mut app = Application::configure(root)
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    rt.block_on(app.boot()).unwrap();
    let target = std::env::var("COLOR_COMMAND").unwrap();
    let code = rt
        .block_on(app.handle_command(&[target.into(), "--no-ansi".into()]))
        .unwrap();
    std::process::exit(code);
}
fn child(
    root: &std::path::Path,
    command: &str,
    env: &serde_json::Map<String, Value>,
    data: Option<&Value>,
) -> String {
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "color_terminal_cli_fixture",
        "--ignored",
        "--nocapture",
    ])
    .env("COLOR_ROOT", root)
    .env("COLOR_COMMAND", command);
    for key in ["TERM", "COLORTERM", "COLUMNS", "LINES", "ANSICON"] {
        cmd.env_remove(key);
    }
    for (key, value) in env {
        cmd.env(key, value.as_str().unwrap());
    }
    if let Some(data) = data {
        cmd.env("COLOR_CASE", data.to_string());
    }
    let output = cmd.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .strip_prefix("\nrunning 1 test\n")
        .unwrap()
        .to_owned()
}
#[test]
fn fixed_color_and_hex_source_consumers_run_through_real_artisan() {
    let root = tempfile::tempdir().unwrap();
    for data in modes() {
        let actual = child(
            root.path(),
            "color:probe",
            &serde_json::Map::new(),
            Some(&data),
        );
        let expected: String = data["colors"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["output"].as_str())
            .collect();
        assert_eq!(actual, expected, "{}", data["mode"]);
    }
}
#[test]
fn fixed_terminal_environment_forced_modes_reset_and_compiled_colors_match() {
    let root = tempfile::tempdir().unwrap();
    for data in terminals() {
        let actual = child(
            root.path(),
            "terminal:probe",
            data["env"].as_object().unwrap(),
            None,
        );
        assert_eq!(
            serde_json::from_str::<Value>(&actual).unwrap(),
            data["expected"],
            "{data}"
        );
    }
}
fn color_routes() -> Result<(), openrust::routing::RoutingError> {
    for path in ["/color", "/color/error"] {
        let error = path.ends_with("error");
        openrust::support::facades::Route::get(path, move || async move {
            let color = Color::new(if error { "invalid" } else { "red" }, "", &["bold"]).map_err(
                |source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                },
            )?;
            openrust::http::Response::json(
                http::StatusCode::OK,
                json!({"color":color.apply("ready"),"width":Terminal::new().get_width(),"height":Terminal::new().get_height()}),
            )
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn real_http_color_errors_do_not_poison_later_requests() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().web(color_routes))
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
    for (path, status) in [("/color", 200), ("/color/error", 500), ("/color", 200)] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert_eq!(
                response.json::<Value>().await.unwrap()["color"],
                "\x1b[31;1mready\x1b[39;22m"
            );
        }
    }
    server.abort();
    let _ = server.await;
}
