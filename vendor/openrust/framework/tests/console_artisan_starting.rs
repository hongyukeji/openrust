use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use openrust::{
    console::{ConsoleError, events::ArtisanStarting},
    contracts::events::{Event, Listener},
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult, ListenerRegistry,
        ListenerResponse,
    },
    foundation::{Application, configuration::Routing, events::DiscoverEvents},
    support::facades::{Artisan, Route},
};
use serde_json::{Value, json};

#[derive(Default)]
struct Attempts(AtomicUsize);

struct Register;
impl Listener for Register {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event.downcast_ref::<ArtisanStarting>().unwrap();
            let attempts = openrust::support::facades::App::make::<Attempts>()
                .await
                .unwrap();
            let round = attempts.0.fetch_add(1, Ordering::SeqCst) + 1;
            assert!(
                event
                    .with_artisan(|artisan| {
                        assert!(artisan.get_command_helper_set("event:configured").is_err());
                        artisan.command("event:dynamic", move |output| {
                            output.stdout.push_str(&format!("starting={round}\n"));
                        })?;
                        Ok(())
                    })
                    .is_ok()
            );
            let nested = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                Artisan::call_captured("event:dynamic", &[]),
            )
            .await
            .expect("监听器调用不得悬挂");
            assert!(nested.is_err());
            let retry = openrust::support::facades::App::base_path("retry").unwrap();
            if retry.exists() && round == 1 {
                return Err(EventError::Listener {
                    source: Box::new(ConsoleError::new("首轮拒绝注册")),
                });
            }
            Ok(None)
        })
    }
}

fn listeners(registry: &mut ListenerRegistry) -> EventResult<()> {
    registry.listen(ArtisanStarting::NAME, Arc::new(Register), Delivery::Sync)
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command("event:configured", |output| {
        output.stdout.push_str("normal command\n")
    })?;
    Ok(())
}

fn application(root: &Path) -> Application {
    let app = Application::configure(root.into())
        .with_events(DiscoverEvents::new(&[listeners]))
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap();
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    app.container()
        .instance(Arc::new(Attempts::default()))
        .unwrap();
    app
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/event-fixture", || async {
        let (left, right) = tokio::join!(
            Artisan::call_captured("event:dynamic", &[]),
            Artisan::call_captured("event:configured", &[])
        );
        let (left, left_output) = left.map_err(|source| openrust::http::HttpError::Internal {
            source: Box::new(source),
        })?;
        let (right, right_output) =
            right.map_err(|source| openrust::http::HttpError::Internal {
                source: Box::new(source),
            })?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!([
                [left, left_output.stdout.trim()],
                [right, right_output.stdout.trim()]
            ]),
        )
    })?;
    Ok(())
}

#[test]
fn actual_artisan_resolves_commands_added_during_its_startup_event() {
    let root = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "isolated_starting_cli",
            "--nocapture",
        ])
        .env("STARTING_TEST_ROOT", root.path())
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stdout).contains("starting=1"));
}

#[tokio::test]
#[ignore = "由父测试执行真实 Artisan 入口进程"]
async fn isolated_starting_cli() {
    let root = std::env::var_os("STARTING_TEST_ROOT").unwrap();
    let app = application(Path::new(&root));
    let status = app.handle_command(&["event:dynamic".into()]).await.unwrap();
    assert_eq!(status, 0);
}

#[tokio::test]
async fn real_http_boots_each_registry_once_and_can_retry_a_listener_failure() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    async fn serve(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }

    for retry in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if retry {
            std::fs::write(root.path().join("retry"), []).unwrap();
        }
        let mut kernel = Kernel::new(application(root.path()));
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
        let url = format!("http://{address}/event-fixture");
        if retry {
            assert_eq!(reqwest::get(&url).await.unwrap().status(), 500);
        }
        for _ in 0..2 {
            let response = reqwest::get(&url).await.unwrap();
            assert_eq!(response.status(), 200);
            let parsed: Value = response.json().await.unwrap();
            let attempt = if retry { 2 } else { 1 };
            assert_eq!(
                parsed,
                json!([[0, format!("starting={attempt}")], [0, "normal command"]])
            );
        }
        server.abort();
        let _ = server.await;
    }
}
