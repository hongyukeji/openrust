use openrust::{
    config::{ConfigError, Environment},
    console::{CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade, Route},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsString, path::Path, sync::Arc};

#[test]
fn signatures_parse_required_optional_defaults_arrays_shortcuts_and_descriptions() {
    let signature = CommandDefinition::from_signature("mail:send {user : The user ID} {names?*} {--Q|queue=default : Queue name} {--id=*} {--force}").unwrap();
    let input = signature
        .parse(
            &[
                "1", "Taylor", "Dayle", "-Q", "urgent", "--id=7", "--id=9", "--force",
            ]
            .map(OsString::from),
        )
        .unwrap();
    assert_eq!(input.argument("user"), Some(json!("1")));
    assert_eq!(input.argument("names"), Some(json!(["Taylor", "Dayle"])));
    assert_eq!(input.option("queue"), Some(json!("urgent")));
    assert_eq!(input.option("id"), Some(json!(["7", "9"])));
    assert_eq!(input.option("force"), Some(json!(true)));
    assert_eq!(input.argument("queue"), None);
    assert_eq!(input.option("user"), None);
    assert_eq!(input.argument("unknown"), None);
    assert_eq!(
        input.arguments(),
        json!({"user":"1","names":["Taylor","Dayle"]})
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!(
        input.options(),
        json!({"queue":"urgent","id":["7","9"],"force":true})
            .as_object()
            .unwrap()
            .clone()
    );
    assert!(signature.parse(&[]).is_err());
    let input = signature.parse(&["1".into()]).unwrap();
    assert_eq!(input.argument("names"), Some(json!([])));
    assert_eq!(input.option("queue"), Some(json!("default")));
    assert_eq!(input.option("force"), Some(json!(false)));
    assert_eq!(input.option("id"), Some(json!([])));
    for (expression, argument, expected) in [
        ("send {user?}", "user", Value::Null),
        ("send {user=Taylor}", "user", json!("Taylor")),
        (
            "send {user=*Taylor, Dayle}",
            "user",
            json!(["Taylor", "Dayle"]),
        ),
    ] {
        let input = CommandDefinition::from_signature(expression)
            .unwrap()
            .parse(&[])
            .unwrap();
        assert_eq!(input.arguments()[argument], expected);
    }
    let input = CommandDefinition::from_signature("send {user*}").unwrap();
    assert!(input.parse(&[]).is_err());
    assert_eq!(
        input
            .parse(&["one".into(), "two".into()])
            .unwrap()
            .argument("user"),
        Some(json!(["one", "two"]))
    );
    let input = CommandDefinition::from_signature("send {--queue=} {--id=*1, 2}")
        .unwrap()
        .parse(&[])
        .unwrap();
    assert_eq!(input.options()["queue"], Value::Null);
    assert_eq!(input.options()["id"], json!(["1", "2"]));
}
#[test]
fn malformed_signatures_fail_registration_instead_of_panicking_during_dispatch() {
    for expression in [
        "",
        "{user}",
        "send {user",
        "send user",
        "send {}",
        "send {x} {x}",
        "send {x?} {y}",
        "send {x*} {y?}",
        "send {--ab|queue}",
        "send {--Q|one} {--Q|two}",
    ] {
        assert!(
            CommandDefinition::from_signature(expression).is_err(),
            "{expression}"
        );
    }
}
fn settings(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"testing"})),
        (
            "logging".into(),
            json!({"default":"null","channels":{"null":{"driver":"null"}}}),
        ),
    ]))
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tutorial:send {user} {--Q|queue=default} {--id=*} {--force}",
        |input: &Input, output: &mut Output| {
            output.stdout.push_str(
                &json!({"arguments":input.arguments(),"options":input.options()}).to_string(),
            );
            Ok(7)
        },
    )?
    .purpose("Send a tutorial message")?;
    Artisan::command("tutorial:legacy", |output: &mut Output| {
        output.comment("legacy");
    })?;
    Artisan::command_with("tutorial:async {user}", injected)?
        .purpose("Resolve an application service")
}
struct Mailer(&'static str);
fn injected<'a>(
    mailer: Arc<Mailer>,
    input: &'a Input,
    output: &'a mut Output,
) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
    Box::pin(async move {
        output
            .stdout
            .push_str(&format!("{}:{}", mailer.0, input.argument("user").unwrap()));
        Ok(0)
    })
}

fn app(root: &Path, label: &'static str) -> Application {
    let app = Application::configure(root.into())
        .with_config(settings)
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap();
    app.container().instance(Arc::new(Mailer(label))).unwrap();
    app
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/send", || async {
        let mut output = Output::default();
        let status = Artisan::call("tutorial:async", &["HTTP 中文".into()], &mut output)
            .await
            .unwrap();
        assert_eq!(status, 0);
        output.stdout
    })
}
#[tokio::test]
async fn actual_http_calls_and_independent_artisan_preserve_inputs_exit_help_and_app_isolation() {
    if let Ok(root) = std::env::var("ARTISAN_SIGNATURE_ROOT") {
        assert_eq!(
            app(Path::new(&root), "child")
                .handle_command(
                    &[
                        "tutorial:send",
                        "Ada",
                        "--queue=urgent",
                        "--id=3",
                        "--id=5",
                        "--force"
                    ]
                    .map(OsString::from)
                )
                .await
                .unwrap(),
            7
        );
        assert_eq!(
            app(Path::new(&root), "child")
                .handle_command(&["tutorial:legacy".into()])
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            app(Path::new(&root), "child")
                .handle_command(&["tutorial:async".into(), "CLI".into()])
                .await
                .unwrap(),
            0
        );
        return;
    }
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    let root = tempfile::tempdir().unwrap();
    let mut first = app(root.path(), "first");
    first.boot().await.unwrap();
    App::set_facade_application(Some(&first), async {
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("tutorial:send", &["Ada".into()], &mut output)
                .await
                .unwrap(),
            7
        );
        let actual: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(actual["arguments"]["user"], "Ada");
        assert_eq!(actual["options"]["queue"], "default");
        assert!(
            Artisan::call("tutorial:send", &[], &mut Output::default())
                .await
                .is_err()
        );
        let mut output = Output::default();
        assert_eq!(
            Artisan::call(
                "help",
                &["tutorial:send".into(), "--format=json".into()],
                &mut output
            )
            .await
            .unwrap(),
            0
        );
        assert!(output.stdout.contains("Send a tutorial message"));
    })
    .await;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    for label in ["left", "right"] {
        let directory = tempfile::tempdir().unwrap();
        let mut kernel = Kernel::new(app(directory.path(), label));
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
        let response = reqwest::get(format!("http://{address}/send"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.text().await.unwrap(),
            format!("{label}:\"HTTP 中文\"")
        );
        server.abort();
        let _ = server.await;
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "actual_http_calls_and_independent_artisan_preserve_inputs_exit_help_and_app_isolation",
            "--nocapture",
        ])
        .env("ARTISAN_SIGNATURE_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let output = String::from_utf8(child.stdout).unwrap();
    assert!(output.contains("child:\"CLI\""));
    assert!(output.contains("legacy"));
    assert!(output.contains("urgent"));
}

#[test]
fn fixed_parser_and_symfony_real_input_vectors_match_native_values() {
    static CASES: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!(
            "../../../.agents/project/audits/artisan-tutorial/signature-oracle.json"
        ))
        .unwrap()
    });
    for case in CASES.as_array().unwrap() {
        let signature = case["signature"].as_str().unwrap();
        let args: Vec<OsString> = case["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().into())
            .collect();
        let input = CommandDefinition::from_signature(signature)
            .unwrap()
            .parse(&args)
            .unwrap();
        for (name, actual) in [
            ("arguments", input.arguments()),
            ("options", input.options()),
        ] {
            let expected = case[name].as_object().cloned().unwrap_or_default();
            assert_eq!(actual, expected, "{signature} {args:?} {name}");
        }
    }
}

#[tokio::test]
async fn injected_services_resolve_lazily_and_recover_after_missing_binding() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().into())
        .with_config(settings)
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    App::set_facade_application(Some(&app), async {
        assert!(
            Artisan::call("tutorial:async", &["Ada".into()], &mut Output::default())
                .await
                .is_err()
        );
        let count = calls.clone();
        app.container()
            .bind(move |_| {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Arc::new(Mailer("bound")))
            })
            .unwrap();
        assert_eq!(
            Artisan::call("help", &["tutorial:async".into()], &mut Output::default())
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            Artisan::call("list", &[], &mut Output::default())
                .await
                .unwrap(),
            0
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        for user in ["Ada", "Grace"] {
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("tutorial:async", &[user.into()], &mut output)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(output.stdout, format!("bound:\"{user}\""));
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    })
    .await;
}
