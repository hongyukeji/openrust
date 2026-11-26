use openrust::{
    console::{ConsoleError, Output, view::components::Task},
    support::{
        DateError, DateInterval, InteractsWithTime, TimeDelay,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
struct Clock;
impl InteractsWithTime for Clock {
    type Error = DateError;
}
fn parsed(value: TimeDelay) -> Value {
    match value {
        TimeDelay::Seconds(value) => json!(value),
        TimeDelay::Date(value) => json!(value.format("Y-m-d H:i:s.u P").unwrap()),
        TimeDelay::Interval(_) => panic!("解析后不应保留区间"),
    }
}
#[tokio::test]
async fn default_trait_and_owned_task_match_fixed_source_with_calendar_intervals() {
    let rows: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../.agents/project/audits/artisan-tutorial/time-delay-oracle.json"),
        )
        .expect("固定来源夹具须先实际生成"),
    )
    .unwrap();
    let mut differences = vec![];
    for (index, row) in rows.iter().enumerate() {
        let zone = row["zone"].as_str().unwrap().to_owned();
        let mut app = TestApp::create(|b| {
            b.with_config(|root, _| {
                Ok([(
                    "app".into(),
                    serde_json::from_slice(&std::fs::read(root.join("time-config.json")).unwrap())
                        .unwrap(),
                )]
                .into())
            })
        })
        .unwrap();
        std::fs::write(
            app.root().join("time-config.json"),
            json!({"timezone":zone}).to_string(),
        )
        .unwrap();
        app.boot().await.unwrap();
        App::set_facade_application(Some(app.app()),async {
            let now=Date::parse(row["now"].as_str().unwrap(),None).unwrap();
            Date::with_test_now(Some(now),async {
                let delay=match row["kind"].as_str().unwrap() {
                    "seconds"=>TimeDelay::Seconds(row["value"].as_i64().unwrap()),
                    "date"=>TimeDelay::Date(Date::parse(row["value"].as_str().unwrap(),None).unwrap()),
                    _=>TimeDelay::Interval(DateInterval { years:row["value"]["years"].as_i64().unwrap(),months:row["value"]["months"].as_i64().unwrap(),days:row["value"]["days"].as_i64().unwrap(),microseconds:row["value"]["microseconds"].as_i64().unwrap()}),
                };
                let clock=Clock;let mut output=Output::default();let task=Task::new(&mut output);
                assert_eq!(clock.current_time().unwrap(),row["current"]);
                assert_eq!(task.current_time().unwrap(),row["current"]);
                for (name,a,b) in [
                    ("seconds",clock.seconds_until(delay.clone()).map(|v|json!(v)),task.seconds_until(delay.clone()).map(|v|json!(v)).map_err(|_|DateError::OutOfRange)),
                    ("available",clock.available_at(delay.clone()).map(|v|json!(v)),task.available_at(delay.clone()).map(|v|json!(v)).map_err(|_|DateError::OutOfRange)),
                    ("parsed",clock.parse_date_interval(delay.clone()).map(parsed),task.parse_date_interval(delay).map(parsed).map_err(|_|DateError::OutOfRange)),
                ] {
                    assert_eq!(a,b,"{index}/{name}");
                    if a.is_err()==row[format!("{name}_error")].is_null() || a.as_ref().is_ok_and(|v|v!=&row[name]) {
                        differences.push(json!({"index":index,"row":row,"method":name,"actual":a.as_ref().ok(),"error":a.err().map(|e|e.to_string())}));
                    }
                }
            }).await.unwrap();
        }).await;
    }
    assert!(
        differences.is_empty(),
        "{}",
        serde_json::to_string_pretty(&differences).unwrap()
    );
}
#[tokio::test]
async fn inherited_overrides_default_argument_and_runtime_hook_keep_their_scope() {
    struct Shift;
    impl InteractsWithTime for Shift {
        type Error = DateError;
        fn current_time(&self) -> Result<i64, DateError> {
            Ok(10)
        }
    }
    let frozen = Date::parse("2024-01-01T00:00:00.500000Z", None).unwrap();
    Date::with_test_now(Some(frozen.clone()), async {
        assert_eq!(Shift.seconds_until(-7).unwrap(), -7);
        assert_eq!(Shift.seconds_until(frozen).unwrap(), 1704067190);
        assert_eq!(Shift.available_at(0).unwrap(), 1704067200);
        assert_eq!(
            Clock.available_at(i64::MAX).unwrap(),
            1704067200i64.wrapping_add(i64::MAX)
        );
        assert_eq!(
            Clock.parse_date_interval(DateInterval {
                years: i64::MAX,
                ..Default::default()
            }),
            Err(DateError::OutOfRange)
        );
        let mut output = Output::default();
        let mut task = Task::new(&mut output);
        task.set_run_time_formatter(Some(Box::new(|_, _| Ok("custom".into()))));
        assert_eq!(
            InteractsWithTime::run_time_for_humans(&mut task, 0.0, Some(0.1)).unwrap(),
            "custom"
        );
        task.set_run_time_formatter(None);
        assert_eq!(
            InteractsWithTime::run_time_for_humans(&mut task, 0.0, Some(0.1)).unwrap(),
            "100.00ms"
        );
        assert_eq!(
            Clock.run_time_for_humans(0.0, Some(0.1)).unwrap(),
            "100.00ms"
        );
        let error = task.run_time_for_humans(0.0, Some(1e30)).unwrap_err();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .is::<std::num::ParseIntError>()
        );
        assert_eq!(
            Clock.run_time_for_humans(0.0, Some(1e30)),
            Err(DateError::OutOfRange)
        );
    })
    .await
    .unwrap();
}
fn commands() -> Result<(), ConsoleError> {
    openrust::support::facades::Artisan::command_with(
        "time:check {view=normal}",
        openrust::console::AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let view = input
                    .argument("view")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap();
                let now = Date::parse(
                    if view == "later" {
                        "2030-01-01T00:00:00Z"
                    } else {
                        "2024-01-01T00:00:00Z"
                    },
                    None,
                )?;
                Date::with_test_now(Some(now),async {
            let mut task=Task::new(output);
            if view=="overflow" { task.parse_date_interval(DateInterval { years:i64::MAX,..Default::default() })?; }
            let available=task.available_at(60)?;
            let current=task.current_time()?;let remaining=task.seconds_until(-5)?;
            let interval=parsed(task.parse_date_interval(openrust::support::months(1))?);
            let runtime=InteractsWithTime::run_time_for_humans(&mut task,0.0,Some(0.1))?;
            drop(task);
            output.comment(&json!({"current":current,"available":available,"remaining":remaining,"interval":interval,"runtime":runtime}).to_string());
            Ok::<_,ConsoleError>(0)
        }).await?
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行实际Artisan退出码"]
fn time_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(openrust::foundation::configuration::Routing::default().commands(commands))
        .create()
        .unwrap();
    let args = vec![
        "time:check".into(),
        std::env::var("TIME_VIEW").unwrap().into(),
        "--no-ansi".into(),
    ];
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt.block_on(app.handle_command(&args)).unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn actual_artisan_time_defaults_and_overflow_exit() {
    for view in ["normal", "later", "overflow"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "time_cli_driver", "--ignored", "--nocapture"])
            .env("TIME_VIEW", view)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(if view == "overflow" { 1 } else { 0 }),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if view != "overflow" {
            assert!(String::from_utf8_lossy(&result.stdout).contains("\"remaining\":-5"));
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/time", response)?;
    openrust::support::facades::Route::get("/time-zone", zone_response)?;
    Ok(())
}
fn response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let view = request
            .query(Some("view"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "normal".into());
        let mut output = Output::default();
        let result =
            openrust::support::facades::Artisan::call("time:check", &[view.into()], &mut output)
                .await;
        openrust::http::Response::json(
            if result.is_ok() {
                http::StatusCode::OK
            } else {
                http::StatusCode::INTERNAL_SERVER_ERROR
            },
            json!({"output":output.stdout}),
        )
    })
}

fn zone_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let zone = request
            .query(Some("zone"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap();
        let instance = request.date("when", Some("Y-m-d H:i:s"), Some(&zone)).await;
        let facade = request
            .with_container(openrust::support::facades::Request::date(
                "when",
                Some("Y-m-d H:i:s"),
                Some(&zone),
            ))
            .await;
        let value = match (instance, facade) {
            (Ok(Some(value)), Ok(Some(facade))) => {
                assert_eq!(value.timezone(), facade.timezone());
                assert_eq!(value.timestamp(), facade.timestamp());
                value
            }
            (Err(error), Err(_)) => return Err(error),
            _ => panic!("日期实例与门面的成功／错误状态不一致"),
        };
        openrust::http::Response::json(
            http::StatusCode::OK,
            json!({"name":value.timezone(),"timestamp":value.timestamp()}),
        )
    })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_http_time_failures_recover_and_concurrent_clocks_are_isolated() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    use std::sync::Arc;
    async fn transport(
        axum::extract::State(k): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        r: axum::extract::Request,
    ) -> axum::response::Response {
        k.handle_transport(r).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = openrust::foundation::Application::configure(root.path().into())
        .with_routing(
            openrust::foundation::configuration::Routing::default()
                .web(routes)
                .commands(commands),
        )
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
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{address}/time?view=overflow"))
            .send()
            .await
            .unwrap()
            .status(),
        500
    );
    let mut requests = tokio::task::JoinSet::new();
    for index in 0..14 {
        let client = client.clone();
        requests.spawn(async move {
            let view = if index % 2 == 0 { "normal" } else { "later" };
            let response = client
                .get(format!("http://{address}/time?view={view}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let body: Value = response.json().await.unwrap();
            let output: Value =
                serde_json::from_str(body["output"].as_str().unwrap().trim()).unwrap();
            let current = if view == "normal" {
                1704067200i64
            } else {
                1893456000
            };
            assert_eq!(output["current"], current);
            assert_eq!(output["available"], current + 60);
            assert_eq!(output["remaining"], -5);
            assert_eq!(output["runtime"], "100.00ms");
        });
    }
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    let offsets: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/time-offset-oracle.json"
    ))
    .unwrap();
    for row in offsets {
        let response = client
            .get(format!("http://{address}/time-zone"))
            .query(&[
                ("zone", row["input"].as_str().unwrap()),
                ("when", "2024-01-01 00:00:00"),
            ])
            .send()
            .await
            .unwrap();
        if row["error"].is_null() {
            assert_eq!(response.status(), 200, "{row}");
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["name"], row["name"]);
            assert_eq!(
                body["timestamp"],
                1704067200i64 - row["offset"].as_i64().unwrap()
            );
        } else {
            assert_eq!(response.status(), 400, "{row}");
        }
    }
    server.abort();
    let _ = server.await;
}
