use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use openrust::{
    console::{
        AsyncCommandCallback, ConsoleError, Output,
        events::{ArtisanStarting, CommandFinished, CommandStarting},
    },
    contracts::{
        console::Kernel as _,
        container::Container as _,
        events::{Event, Listener},
        foundation::Application as _,
    },
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult, ListenerRegistry,
        ListenerResponse,
    },
    foundation::{Application, configuration::Routing, events::DiscoverEvents},
    support::facades::{App, Artisan, Route},
};
use serde_json::{Value, json};

const REFERENCE: &str =
    include_str!("../../../../.agents/project/audits/artisan-tutorial/artisan-events-oracle.json");

#[derive(Default)]
struct Trace(Mutex<Vec<Value>>);

impl Trace {
    fn record(&self, phase: &str, command: &str, input: Option<&str>, status: Option<i32>) {
        self.0
            .lock()
            .unwrap()
            .push(json!([phase, command, input, status]));
    }

    fn value(&self) -> Value {
        json!(*self.0.lock().unwrap())
    }
}

fn parsed_value(input: &clap::ArgMatches) -> Option<&str> {
    input
        .try_get_one::<String>("value")
        .ok()
        .flatten()
        .map(String::as_str)
}

struct Events;
impl Listener for Events {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let trace = App::make::<Trace>().await.unwrap();
            if let Some(event) = event.downcast_ref::<ArtisanStarting>() {
                trace.record("artisan", "starting", None, None);
                event
                    .with_artisan(|artisan| {
                        let trace = trace.clone();
                        artisan.command("event-probe:dynamic", move |_| {
                            trace.record("handle", "event-probe:dynamic", None, None);
                        })?;
                        Ok(())
                    })
                    .map_err(|source| EventError::Listener {
                        source: Box::new(source),
                    })?;
            }
            if let Some(event) = event.downcast_ref::<CommandStarting>() {
                trace.record("start", &event.command, parsed_value(&event.input), None);
            }
            if let Some(event) = event.downcast_ref::<CommandFinished>() {
                trace.record(
                    "finish",
                    &event.command,
                    parsed_value(&event.input),
                    Some(event.exit_code),
                );
            }
            Ok(None)
        })
    }
}

fn listeners(registry: &mut ListenerRegistry) -> EventResult<()> {
    for name in [
        ArtisanStarting::NAME,
        CommandStarting::NAME,
        CommandFinished::NAME,
    ] {
        registry.listen(name, Arc::new(Events), Delivery::Sync)?;
    }
    Ok(())
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "event-probe:outer {value}",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let data = input.argument("value").unwrap();
                let value = data.as_str().unwrap();
                let trace = app.make::<Trace>().await.unwrap();
                trace.record("handle", "event-probe:outer", Some(value), None);
                match value {
                    "nested" => {
                        input
                            .io(output)
                            .call_named("event-probe:child", &[("value", json!("nested"))])
                            .await
                    }
                    "throw" => Err(ConsoleError::new("fixture failure")),
                    "nonzero" => Ok(7),
                    _ => Ok(0),
                }
            })
        }),
    )?;
    Artisan::command_with(
        "event-probe:child {value}",
        AsyncCommandCallback::new(|app, input, _| {
            Box::pin(async move {
                let data = input.argument("value").unwrap();
                app.make::<Trace>().await.unwrap().record(
                    "handle",
                    "event-probe:child",
                    data.as_str(),
                    None,
                );
                Ok(0)
            })
        }),
    )?;
    Ok(())
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/console-events", || async {
        let status = Artisan::call_captured("event-probe:outer", &["nested".into()])
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?
            .0;
        let trace = App::make::<Trace>().await.unwrap().value();
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"status":status,"trace":trace}),
        )
    })?;
    Route::get("/console-events-shortcuts", || async {
        let (status, output) = Artisan::call_captured("event-probe:outer", &["--help".into()])
            .await
            .map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        let invalid = Artisan::call_captured("event-probe:outer", &[]).await;
        let trace = App::make::<Trace>().await.unwrap().value();
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"status":status,"help":output.stdout.contains("event-probe:outer"),"rejected":invalid.is_err(),"trace":trace}),
        )
    })?;
    Ok(())
}

fn app(root: &Path) -> (Application, Arc<Trace>) {
    let app = Application::configure(root.into())
        .with_events(DiscoverEvents::new(&[listeners]))
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap();
    let trace = Arc::new(Trace::default());
    app.container().instance(trace.clone()).unwrap();
    (app, trace)
}

#[tokio::test]
async fn actual_kernel_events_follow_fixed_source_for_all_command_paths() {
    let root = tempfile::tempdir().unwrap();
    let (app, trace) = app(root.path());
    let mut kernel =
        openrust::foundation::console::Kernel::new(app, openrust::console::Application::new());
    let oracle: Value = serde_json::from_str(REFERENCE).unwrap();
    let lines = [
        "event-probe:outer good",
        "event-probe:outer nonzero",
        "event-probe:outer nested",
        "event-probe:outer throw",
        "event-probe:dynamic",
        "list",
        "help event-probe:outer",
        "event-probe:outer --help",
        "event-probe:outer",
        "--version",
        "",
    ];
    for line in lines {
        let args: Vec<_> = line
            .split_ascii_whitespace()
            .map(std::ffi::OsString::from)
            .collect();
        let status = kernel.handle(&args, &mut Output::default()).await;
        let expected = oracle["statuses"][line].as_i64().unwrap_or(1);
        assert_eq!(i64::from(status), expected, "{line}");
    }
    assert_eq!(trace.value(), oracle["trace"]);
}

#[test]
fn actual_artisan_entry_does_not_bracket_direct_child_calls_with_events() {
    let root = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "isolated_events_cli", "--nocapture"])
        .env("EVENTS_TEST_ROOT", root.path())
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
}

#[tokio::test]
#[ignore = "由父测试执行真实 Artisan 入口进程"]
async fn isolated_events_cli() {
    let root = std::env::var_os("EVENTS_TEST_ROOT").unwrap();
    let (app, trace) = app(Path::new(&root));
    assert_eq!(
        app.handle_command(&["event-probe:outer".into(), "nested".into()])
            .await
            .unwrap(),
        0
    );
    let oracle: Value = serde_json::from_str(REFERENCE).unwrap();
    let values = oracle["trace"].as_array().unwrap();
    let expected = vec![
        values[0].clone(),
        values[7].clone(),
        values[8].clone(),
        values[9].clone(),
        values[10].clone(),
    ];
    assert_eq!(trace.value(), json!(expected));
}

#[tokio::test]
async fn actual_http_facade_call_dispatches_only_its_own_start_and_finish() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn serve(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let (app, _) = app(root.path());
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            serve.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    let response = reqwest::get(format!("http://{address}/console-events"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let received: Value = response.json().await.unwrap();
    let oracle: Value = serde_json::from_str(REFERENCE).unwrap();
    let values = oracle["trace"].as_array().unwrap();
    let expected = vec![
        values[0].clone(),
        values[7].clone(),
        values[8].clone(),
        values[9].clone(),
        values[10].clone(),
    ];
    assert_eq!(received, json!({"status":0,"trace":expected}));
    let shortcuts = reqwest::get(format!("http://{address}/console-events-shortcuts"))
        .await
        .unwrap();
    let body: Value = shortcuts.json().await.unwrap();
    let mut expected: Vec<_> = received["trace"].as_array().unwrap().clone();
    expected.extend_from_slice(&values[21..25]);
    assert_eq!(
        body,
        json!({"status":0,"help":true,"rejected":true,"trace":expected})
    );
    server.abort();
    let _ = server.await;
}
