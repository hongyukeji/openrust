use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::{Application, configuration::Routing},
    support::facades::{Artisan, Route},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};
#[test]
fn native_edits_revalidate_atomically_and_preserve_arrays_empty_values_and_shutdown() {
    let definition = CommandDefinition::from_signature(
        "edit {user} {items?*} {--Q|queue=default} {--force} {--id=*}",
    )
    .unwrap();
    let original = definition
        .parse(&["Ada", "first", "-Q", "old", "--id=3", "--force"].map(Into::into))
        .unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut input = original.with_shutdown(cancel.clone());
    input.set_argument("user", ["Grace"]).unwrap();
    assert_eq!(input.argument("user"), Some(json!("Grace")));
    assert_eq!(original.argument("user"), Some(json!("Ada")));
    input
        .set_argument("items", ["-literal", "two words", "", "中文"])
        .unwrap();
    assert_eq!(
        input.argument("items"),
        Some(json!(["-literal", "two words", "", "中文"]))
    );
    input.set_option("queue", json!(null)).unwrap();
    assert_eq!(input.option("queue"), Some(Value::Null));
    input.set_option("force", json!(false)).unwrap();
    assert_eq!(input.option("force"), Some(json!(false)));
    input.set_option("id", json!(["7", null, "9"])).unwrap();
    assert_eq!(input.option("id"), Some(json!(["7", null, "9"])));
    input.set_option("id", json!([])).unwrap();
    assert_eq!(input.option("id"), Some(json!([])));
    let state = input.input().to_array();
    for (key, value) in [
        ("unknown", json!(true)),
        ("force", json!("false")),
        ("id", json!([{}])),
    ] {
        assert!(input.set_option(key, value).is_err());
        assert_eq!(input.input().to_array(), state);
    }
    assert!(
        input
            .set_argument("user", Vec::<std::ffi::OsString>::new())
            .is_err()
    );
    assert_eq!(input.input().to_array(), state);
    assert!(input.set_option("user", json!("wrong group")).is_err());
    assert!(input.set_argument("force", ["wrong group"]).is_err());
    cancel.cancel();
    assert!(input.shutdown().unwrap().is_cancelled());
}
#[test]
fn edits_do_not_silently_rebind_optional_positions_or_restore_nonempty_defaults() {
    let definition = CommandDefinition::from_signature(
        "edit {first?} {second=default} {--id=*1,2} {--queue=default}",
    )
    .unwrap();
    let mut input = definition.parse(&[]).unwrap();
    input.set_option("queue", json!("urgent")).unwrap();
    assert_eq!(input.argument("first"), None);
    assert_eq!(input.argument("second"), Some(json!("default")));
    let state = input.input().to_array();
    assert!(input.set_argument("second", ["override"]).is_err());
    assert_eq!(input.input().to_array(), state);
    assert!(input.set_option("id", json!([])).is_err());
    assert_eq!(input.option("id"), Some(json!(["1", "2"])));
}
#[cfg(unix)]
#[test]
fn unchanged_non_utf8_values_survive_reparsing() {
    use std::os::unix::ffi::OsStringExt as _;
    let definition = CommandDefinition::new(
        clap::Command::new("edit")
            .arg(
                clap::Arg::new("path")
                    .required(true)
                    .value_parser(clap::value_parser!(std::ffi::OsString)),
            )
            .arg(clap::Arg::new("queue").long("queue")),
    );
    let raw = std::ffi::OsString::from_vec(vec![b'a', 0xff]);
    let mut input = definition.parse(std::slice::from_ref(&raw)).unwrap();
    input.set_option("queue", json!("urgent")).unwrap();
    assert_eq!(
        input.matches().get_one::<std::ffi::OsString>("path"),
        Some(&raw)
    );
}
struct Echo(CommandDefinition);
impl Command for Echo {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output
                .stdout
                .push_str(input.matches().get_one::<String>("user").unwrap());
            assert_eq!(input.option("quiet"), Some(json!(false)));
            Ok(0)
        })
    }
}
#[tokio::test]
async fn edited_input_keeps_definition_identity_for_the_kernel_contract() {
    use openrust::contracts::console::Kernel as _;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into()).create().unwrap();
    let definition = CommandDefinition::from_signature("edit {user}").unwrap();
    let mut input = definition.parse(&["Ada".into()]).unwrap();
    input.set_argument("user", ["Grace"]).unwrap();
    let mut console = openrust::console::Application::new();
    console.add_command(Box::new(Echo(definition))).unwrap();
    let mut kernel = openrust::foundation::console::Kernel::new(app, console);
    kernel.bootstrap().await.unwrap();
    let mut output = Output::default();
    assert_eq!(
        kernel
            .call("edit", &input, Some(&mut output))
            .await
            .unwrap(),
        0
    );
    assert_eq!(output.stdout, "Grace");
}
fn config(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
        ("app".into(), json!({"env":"testing"})),
    ]))
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with("read:invalid {user}", |input: &Input, _: &mut Output| {
        let mut edited = input.clone();
        edited.set_argument("user", Vec::<std::ffi::OsString>::new())?;
        Ok(0)
    })?;
    Artisan::command_with("read {user} {--queue=default}",|input:&Input,output:&mut Output| {
        let mut edited=input.clone();edited.set_option("queue",json!("urgent"))?;edited.set_option("verbose",json!(2))?;
        output.stdout.push_str(&serde_json::to_string(&json!({"arguments":edited.arguments(),"options":edited.options(),"original":input.options()})).unwrap());Ok(0)
    })?.purpose("Read inherited and edited input")
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/bad-input", || async {
        let mut output = Output::default();
        let status = Artisan::call("read:invalid", &["Ada".into()], &mut output)
            .await
            .map_err(|error| openrust::http::HttpError::Internal {
                source: Box::new(error),
            })?;
        Ok::<_, openrust::http::HttpError>(status.to_string())
    })?;
    Route::get("/input", || async {
        let mut output = Output::default();
        assert_eq!(
            Artisan::call(
                "read",
                &["Ada", "--ansi", "--no-interaction"].map(Into::into),
                &mut output
            )
            .await
            .unwrap(),
            0
        );
        output.stdout
    })?;
    Ok(())
}
#[tokio::test]
async fn real_http_and_independent_artisan_keep_global_options_and_mutations_consistent() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    fn app(root: &Path) -> Application {
        Application::configure(root.into())
            .with_config(config)
            .with_routing(Routing::default().commands(commands).web(routes))
            .create()
            .unwrap()
    }
    if let Ok(root) = std::env::var("EDITED_INPUT_ROOT") {
        assert_eq!(
            app(Path::new(&root))
                .handle_command(&["read", "Ada", "--ansi", "--no-interaction"].map(Into::into))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            app(Path::new(&root))
                .handle_command(&["read:invalid", "Ada"].map(Into::into))
                .await
                .unwrap(),
            1
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(app(root.path()));
    kernel.bootstrap().await.unwrap();
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
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
        .unwrap()
    });
    let response = reqwest::get(format!("http://{address}/input"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["options"]["queue"], "urgent");
    assert_eq!(value["options"]["ansi"], true);
    assert_eq!(value["options"]["no-interaction"], true);
    assert_eq!(value["options"]["verbose"], true);
    assert_eq!(value["original"]["verbose"], false);
    assert_eq!(value["original"]["queue"], "default");
    let failed = reqwest::Client::new()
        .get(format!("http://{address}/bad-input"))
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);
    assert!(
        failed.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("application/json")
    );
    assert_eq!(
        reqwest::get(format!("http://{address}/input"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        text
    );
    server.abort();
    let _ = server.await;
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "real_http_and_independent_artisan_keep_global_options_and_mutations_consistent",
            "--nocapture",
        ])
        .env("EDITED_INPUT_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8(child.stdout).unwrap().contains(&text));
}

#[tokio::test]
async fn application_metadata_matches_global_input_vectors_without_losing_native_matches() {
    struct Globals(CommandDefinition);
    impl Command for Globals {
        fn definition(&self) -> &CommandDefinition {
            &self.0
        }
        fn handle<'a>(
            &'a self,
            _: &'a Application,
            input: &'a Input,
            output: &'a mut Output,
        ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
            Box::pin(async move {
                let mut edited = input.clone();
                edited.set_option("ansi", Value::Null)?;
                assert_eq!(edited.option("ansi"), Some(Value::Null));
                edited.set_option("ansi", json!(false))?;
                assert_eq!(edited.option("ansi"), Some(json!(false)));
                edited.set_option("ansi", json!(true))?;
                assert_eq!(edited.option("ansi"), Some(json!(true)));
                edited.set_option("verbose", json!(true))?;
                assert_eq!(edited.matches().get_count("verbose"), 1);
                output.stdout.push_str(&serde_json::to_string(&json!({"arguments":input.arguments(),"options":input.options(),"level":input.matches().get_count("verbose")})).unwrap());
                Ok(0)
            })
        }
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into()).create().unwrap();
    let mut console = openrust::console::Application::new();
    console
        .add_command(Box::new(Globals(CommandDefinition::new(
            clap::Command::new("read")
                .alias("read-alias")
                .arg(clap::Arg::new("user").required(true))
                .arg(
                    clap::Arg::new("queue")
                        .long("queue")
                        .default_value("default"),
                ),
        ))))
        .unwrap();
    use openrust::contracts::console::Kernel as _;
    let mut kernel = openrust::foundation::console::Kernel::new(app, console);
    kernel.bootstrap().await.unwrap();
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/artisan-tutorial/global-input-oracle.json"
    ))
    .unwrap();
    for (name, flags, ansi, verbose, env, level) in [
        (
            "read",
            vec!["--ansi", "-vv", "--env=testing", "--no-interaction"],
            json!(true),
            true,
            json!("testing"),
            2,
        ),
        (
            "read-alias",
            vec!["--no-ansi"],
            json!(false),
            false,
            Value::Null,
            0,
        ),
        ("read", vec![], Value::Null, false, Value::Null, 0),
    ] {
        let args: Vec<_> = [name, "Ada"]
            .into_iter()
            .chain(flags.iter().copied())
            .map(Into::into)
            .collect();
        let mut output = Output::default();
        assert_eq!(kernel.handle(&args, &mut output).await, 0);
        let result: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(result["arguments"], json!({"command":name,"user":"Ada"}));
        assert_eq!(result["options"]["ansi"], ansi);
        assert_eq!(result["options"]["verbose"], verbose);
        assert_eq!(result["options"]["env"], env);
        assert_eq!(result["level"], level);
        assert!(result["options"].get("no-ansi").is_none());
        let source = oracle
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["tokens"][0] == name && case["options"]["ansi"] == ansi)
            .unwrap();
        assert_eq!(result["arguments"], source["arguments"]);
        assert_eq!(result["options"], source["options"]);
    }
}
