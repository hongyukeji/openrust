use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, configuration::Routing},
    support::facades::{App, Artisan, Facade as _, Route},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct RegistryProbe {
    registrations: AtomicUsize,
    fail_first: bool,
}
struct Counter {
    definition: CommandDefinition,
    count: AtomicUsize,
}
impl Command for Counter {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let count = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            if input.option("fail") == Some(json!(true)) {
                return Err(ConsoleError::new(format!("counter-failure-{count}")));
            }
            if input.option("nested") == Some(json!(true)) {
                let mut nested = Output::default();
                Artisan::call("lifecycle:counter", &[], &mut nested).await?;
                output
                    .stdout
                    .push_str(&format!("{count}:{}", nested.stdout));
            } else {
                output.stdout.push_str(&format!("{count}\n"));
            }
            Ok(0)
        })
    }
}
fn commands() -> Result<(), ConsoleError> {
    let app = App::get_facade_application()
        .map_err(ConsoleError::from_source)?
        .unwrap();
    let probe = app
        .container()
        .make::<RegistryProbe>()
        .map_err(ConsoleError::from_source)?;
    if probe.registrations.fetch_add(1, Ordering::SeqCst) == 0 && probe.fail_first {
        return Err(ConsoleError::new("registry-first-failure"));
    }
    Artisan::add(Box::new(Counter {
        definition: CommandDefinition::from_signature("lifecycle:counter {--fail} {--nested}")?,
        count: AtomicUsize::new(0),
    }))
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
        (
            "cache".into(),
            json!({"default":"array","stores":{"array":{"driver":"array"}}}),
        ),
    ]))
}
fn configured_app(root: &Path, fail_first: bool) -> (Application, Arc<RegistryProbe>) {
    let app = Application::configure(root.into())
        .with_config(config)
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap();
    let probe = Arc::new(RegistryProbe {
        registrations: AtomicUsize::new(0),
        fail_first,
    });
    app.container().instance(probe.clone()).unwrap();
    (app, probe)
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/counter", || async {
        let mut output = Output::default();
        Artisan::call("lifecycle:counter", &[], &mut output)
            .await
            .unwrap();
        output.stdout
    })?;
    Ok(())
}
#[tokio::test]
async fn facade_calls_reuse_command_instances_and_failed_calls_keep_the_same_instance() {
    let root = tempfile::tempdir().unwrap();
    let (mut application, probe) = configured_app(root.path(), false);
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../.agents/project/audits/artisan-tutorial/registry-oracle.json"
        ))
        .unwrap();
        for case in oracle.as_array().unwrap().iter().take(2) {
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("lifecycle:counter", &[], &mut output)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(output.stdout, case["output"].as_str().unwrap());
        }
        assert!(
            Artisan::call(
                "lifecycle:counter",
                &["--fail".into()],
                &mut Output::default()
            )
            .await
            .unwrap_err()
            .to_string()
            .contains(oracle[2]["error"].as_str().unwrap())
        );
        let mut output = Output::default();
        Artisan::call("lifecycle:counter", &[], &mut output)
            .await
            .unwrap();
        assert_eq!(output.stdout, oracle[3]["output"].as_str().unwrap());
    })
    .await;
    assert_eq!(probe.registrations.load(Ordering::SeqCst), 1);
}
async fn serve(
    root: &Path,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<RegistryProbe>,
) {
    use axum::handler::Handler as _;
    let (application, probe) = configured_app(root, false);
    let mut kernel = openrust::foundation::http::Kernel::new(application);
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
        .unwrap()
    });
    (address, server, probe)
}
#[tokio::test]
async fn actual_http_concurrent_first_calls_initialize_once_and_two_applications_remain_isolated() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let (address, server, probe) = serve(root.path()).await;
    let (other_address, other_server, other_probe) = serve(other.path()).await;
    let client = reqwest::Client::new();
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..48 {
        let client = client.clone();
        requests.spawn(async move {
            let response = client
                .get(format!("http://{address}/counter"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            response
                .text()
                .await
                .unwrap()
                .trim()
                .parse::<usize>()
                .unwrap()
        });
    }
    let mut values = Vec::new();
    while let Some(value) = requests.join_next().await {
        values.push(value.unwrap());
    }
    values.sort();
    assert_eq!(values, (1..=48).collect::<Vec<_>>());
    for (address, expected) in [(other_address, "1"), (address, "49"), (other_address, "2")] {
        assert_eq!(
            client
                .get(format!("http://{address}/counter"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
                .trim(),
            expected
        );
    }
    assert_eq!(probe.registrations.load(Ordering::SeqCst), 1);
    assert_eq!(other_probe.registrations.load(Ordering::SeqCst), 1);
    server.abort();
    other_server.abort();
    let _ = server.await;
    let _ = other_server.await;
}
#[tokio::test]
async fn failed_initialization_retries_without_publishing_partial_registry_and_nested_calls_do_not_deadlock()
 {
    let root = tempfile::tempdir().unwrap();
    let (mut application, probe) = configured_app(root.path(), true);
    application.boot().await.unwrap();
    App::set_facade_application(Some(&application), async {
        assert!(
            Artisan::call("lifecycle:counter", &[], &mut Output::default())
                .await
                .unwrap_err()
                .to_string()
                .contains("registry-first-failure")
        );
        let mut output = Output::default();
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                Artisan::call("lifecycle:counter", &["--nested".into()], &mut output)
            )
            .await
            .unwrap()
            .unwrap(),
            0
        );
        assert_eq!(output.stdout, "1:2\n");
        let mut output = Output::default();
        Artisan::call("lifecycle:counter", &[], &mut output)
            .await
            .unwrap();
        assert_eq!(output.stdout, "3\n");
    })
    .await;
    assert_eq!(probe.registrations.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn independent_artisan_binary_reuses_registry_for_nested_calls() {
    if let Ok(root) = std::env::var("REGISTRY_CHILD_ROOT") {
        let (application, probe) = configured_app(Path::new(&root), false);
        assert_eq!(
            application
                .handle_command(&["lifecycle:counter".into(), "--nested".into()])
                .await
                .unwrap(),
            0
        );
        assert_eq!(probe.registrations.load(Ordering::SeqCst), 1);
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "independent_artisan_binary_reuses_registry_for_nested_calls",
            "--nocapture",
        ])
        .env("REGISTRY_CHILD_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{} {}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1:2\n"));
}
