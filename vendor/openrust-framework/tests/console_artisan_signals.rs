#![cfg(unix)]

use std::{
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

use openrust::{
    console::{AsyncCommandCallback, CommandSignal, ConsoleError},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::Routing},
    support::facades::{Artisan, Route},
};
use serde_json::{Value, json};
use tokio::sync::Notify;

const ORACLE: &str =
    include_str!("../../../.agents/project/audits/artisan-tutorial/artisan-signals-oracle.json");

#[derive(Default)]
struct Observed {
    events: Mutex<Vec<(&'static str, String)>>,
    changed: Notify,
}

impl Observed {
    fn callback(
        self: &Arc<Self>,
        label: &'static str,
    ) -> impl Fn(CommandSignal) + Send + Sync + 'static {
        let this = Arc::clone(self);
        move |signal| {
            this.events
                .lock()
                .unwrap()
                .push((label, format!("{signal:?}")));
            this.changed.notify_one();
        }
    }

    async fn deliver(&self, name: &str, expected: usize) {
        let target = std::process::id().to_string();
        assert!(
            Command::new("kill")
                .args(["-s", name, &target])
                .status()
                .unwrap()
                .success()
        );
        tokio::time::timeout(Duration::from_secs(4), async {
            while self.events.lock().unwrap().len() < expected {
                self.changed.notified().await;
            }
        })
        .await
        .expect("命令必须收到真实 OS 信号");
    }

    fn value(&self) -> Value {
        json!(*self.events.lock().unwrap())
    }
}

fn commands() -> Result<(), ConsoleError> {
    use CommandSignal::{Quit, Terminate};
    Artisan::command_with(
        "signal:inner",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let observed = app.make::<Observed>().await.unwrap();
                let io = input.io(output);
                io.trap_many([Terminate, Quit], observed.callback("inner-many"))?;
                io.trap(Terminate, observed.callback("inner-last"))?;
                observed.deliver("TERM", 3).await;
                observed.deliver("QUIT", 4).await;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "signal:outer",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let observed = app.make::<Observed>().await.unwrap();
                let mut io = input.io(output);
                io.trap(Terminate, observed.callback("outer"))?;
                let code = io.call("signal:inner", &[]).await?;
                observed.deliver("TERM", 5).await;
                io.untrap();
                io.trap(Terminate, observed.callback("replacement"))?;
                observed.deliver("TERM", 6).await;
                io.line(&observed.value().to_string())?;
                Ok(code)
            })
        }),
    )?;
    Artisan::command_with(
        "signal:after",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let observed = app.make::<Observed>().await.unwrap();
                let mut io = input.io(output);
                let before = observed.events.lock().unwrap().len();
                assert!(
                    io.trap_many(
                        [Terminate, CommandSignal::Raw(libc::SIGKILL)],
                        observed.callback("invalid")
                    )
                    .is_err()
                );
                io.trap(
                    CommandSignal::Raw(libc::SIGTERM),
                    observed.callback("after"),
                )?;
                observed.deliver("TERM", before + 1).await;
                assert_eq!(observed.events.lock().unwrap().len(), before + 1);
                io.line(&observed.value().to_string())?;
                Ok(0)
            })
        }),
    )?;
    Artisan::command_with(
        "signal:cancel",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let observed = app.make::<Observed>().await.unwrap();
                input
                    .io(output)
                    .trap(Terminate, observed.callback("cancelled"))?;
                observed.changed.notify_one();
                std::future::pending().await
            })
        }),
    )?;
    Artisan::command_with(
        "signal:error",
        AsyncCommandCallback::new(|app, input, output| {
            Box::pin(async move {
                let observed = app.make::<Observed>().await.unwrap();
                input
                    .io(output)
                    .trap(Terminate, observed.callback("error"))?;
                Err(ConsoleError::new("预期的错误退出"))
            })
        }),
    )?;
    Ok(())
}

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/signals", || async {
        let (status, output) =
            Artisan::call_captured("signal:outer", &[])
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
        let observed: Value = serde_json::from_str(&output.stdout).unwrap();
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"status":status,"observed":observed}),
        )
    })?;
    Ok(())
}

fn application(root: &Path) -> Application {
    let app = Application::configure(root.into())
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap();
    app.container()
        .instance(Arc::new(Observed::default()))
        .unwrap();
    app
}

#[test]
fn real_artisan_and_http_match_nested_os_signal_oracle() {
    let binary = std::env::current_exe().unwrap();
    for scenario in ["cli", "http"] {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(&binary)
            .args([
                "--ignored",
                "--exact",
                "isolated_signal_driver",
                "--nocapture",
            ])
            .env("SIGNAL_SCENARIO", scenario)
            .env("SIGNAL_TEST_ROOT", root.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{scenario}: {output:?}");
        println!("已验真实 {scenario} 信号消费者");
    }
}

#[tokio::test]
#[ignore = "由父测试独立执行以隔离进程信号"]
async fn isolated_signal_driver() {
    let root = std::env::var_os("SIGNAL_TEST_ROOT").unwrap();
    let app = application(Path::new(&root));
    let oracle: Value = serde_json::from_str(ORACLE).unwrap();
    match std::env::var("SIGNAL_SCENARIO").unwrap().as_str() {
        "cli" => {
            use openrust::support::facades::{App, Facade as _};
            let observed = app.make::<Observed>().await.unwrap();
            let status = app.handle_command(&["signal:outer".into()]).await.unwrap();
            assert_eq!(status, oracle["status"]);
            assert_eq!(observed.value(), oracle["observed"]);

            let mut followup = application(Path::new(&root));
            followup.boot().await.unwrap();
            let next_observation = followup.make::<Observed>().await.unwrap();
            App::set_facade_application(Some(&followup), async {
                let mut call = Box::pin(Artisan::call_captured("signal:cancel", &[]));
                tokio::select! {
                    _ = &mut call => panic!("取消夹具不应完成"),
                    _ = next_observation.changed.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(4)) => panic!("未注册取消夹具"),
                }
                drop(call);
                assert!(Artisan::call_captured("signal:error", &[]).await.is_err());
                let (status, _) = Artisan::call_captured("signal:after", &[]).await.unwrap();
                assert_eq!(status, 0);
            })
            .await;
            assert_eq!(
                next_observation.value(),
                json!([["after", format!("Raw({})", libc::SIGTERM)]])
            );
            assert_eq!(observed.value(), oracle["observed"]);
        }
        "http" => {
            use axum::handler::Handler as _;
            use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
            async fn transport(
                axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
                request: axum::extract::Request,
            ) -> axum::response::Response {
                kernel.handle_transport(request).await
            }
            let mut kernel = Kernel::new(app);
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
            let response = reqwest::get(format!("http://{address}/signals"))
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["status"], oracle["status"]);
            assert_eq!(body["observed"], oracle["observed"]);
            server.abort();
            let _ = server.await;
        }
        other => panic!("未知夹具 {other}"),
    }
}
