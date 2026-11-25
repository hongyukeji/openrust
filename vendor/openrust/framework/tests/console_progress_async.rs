use openrust::{
    console::{AsyncCommandCallback, CommandDefinition, ConsoleError, Output},
    foundation::{Application, configuration::Routing},
    support::{ArrayKey, facades::Artisan},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
#[tokio::test]
async fn numeric_and_readonly_helpers_await_callbacks_and_preserve_original_values() {
    let input = CommandDefinition::from_signature("progress")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut io = input.io(&mut output);
    io.with_progress_bar_async(2, |bar| {
        Box::pin(async move {
            assert_eq!(bar.get_progress(), 0);
            tokio::task::yield_now().await;
            bar.advance_by(2)?;
            Ok(17)
        })
    })
    .await
    .unwrap();
    let values = BTreeMap::from([
        ("first".to_owned(), String::from("A")),
        ("second".to_owned(), String::from("B")),
    ]);
    let returned = io
        .with_progress_bar_iter_async(&values, |value, bar, key| {
            Box::pin(async move {
                assert_eq!(
                    key,
                    if value == "A" {
                        "first".into()
                    } else {
                        "second".into()
                    }
                );
                assert_eq!(bar.get_progress(), if value == "A" { 0 } else { 1 });
                tokio::task::yield_now().await;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert!(std::ptr::eq(returned, &values));
    drop(io);
    assert_eq!(output.stdout.matches("100%").count(), 2);
}
#[test]
fn mutable_helpers_preserve_keys_and_update_every_supported_native_collection() {
    use openrust::console::ProgressIterableMut;
    fn update<I: ProgressIterableMut<Item = i32>>(mut values: I) {
        let input = CommandDefinition::from_signature("progress")
            .unwrap()
            .parse(&[])
            .unwrap();
        let mut output = Output::default();
        let count = values.progress_iter().len();
        let keys: Vec<_> = values.progress_iter().map(|(key, _)| key).collect();
        let mut seen = Vec::new();
        input
            .io(&mut output)
            .with_progress_bar_iter_mut(&mut values, |value, bar, key| {
                assert_eq!(bar.get_progress() as usize, seen.len());
                assert_eq!(bar.get_max_steps() as usize, count);
                *value += 10;
                seen.push(key);
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, keys);
        assert!(values.progress_iter().all(|(_, v)| *v >= 10));
    }
    update(vec![1, 2]);
    update([1, 2]);
    let mut slice = [1, 2];
    update(&mut slice[..]);
    update(BTreeMap::from([("first", 1), ("last", 2)]));
    update(indexmap::IndexMap::from([("first", 1), ("last", 2)]));
    let mut base = openrust::support::Collection::from_entries([("first", 1), ("last", 2)]);
    update(&mut base);
    assert_eq!(base.get("last"), Some(&12));
}
#[tokio::test]
async fn failure_and_cancellation_do_not_complete_or_lose_borrowed_changes() {
    let input = CommandDefinition::from_signature("progress")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let mut values = vec![0, 0];
    {
        let mut io = input.io(&mut output);
        let error = io
            .with_progress_bar_iter_mut_async(&mut values, |value, bar, _| {
                Box::pin(async move {
                    *value += 1;
                    assert_eq!(bar.get_progress(), 0);
                    tokio::task::yield_now().await;
                    Err::<(), _>(ConsoleError::from_source(std::io::Error::other(
                        "task failed",
                    )))
                })
            })
            .await
            .unwrap_err();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .is::<std::io::Error>()
        );
    }
    assert_eq!(values, [1, 0]);
    assert!(!output.stdout.contains("100%"));
    let started = Arc::new(tokio::sync::Notify::new());
    let notify = started.clone();
    {
        let mut io = input.io(&mut output);
        let future = io.with_progress_bar_iter_mut_async(&mut values, move |value, _, _| {
            let notify = notify.clone();
            Box::pin(async move {
                *value += 1;
                notify.notify_one();
                std::future::pending::<()>().await;
                Ok(())
            })
        });
        let mut future = Box::pin(future);
        tokio::select! {_=&mut future=>panic!("取消前不应完成"), _=started.notified()=>{}}
    }
    assert_eq!(values, [2, 0]);
    assert!(!output.stdout.contains("100%"));
    input
        .io(&mut output)
        .with_progress_bar_iter_mut_async(&mut values, |value, _, _| {
            Box::pin(async move {
                *value += 1;
                tokio::task::yield_now().await;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(values, [3, 1]);
    assert!(output.stdout.contains("100%"));
}
struct Contact(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for Contact {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("progress_contacts");
        &META
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
}
#[tokio::test]
async fn queried_non_clone_models_save_through_the_awaited_mutable_callback() {
    use openrust::{
        config::Repository,
        database::{
            DatabaseManager,
            eloquent::{Collection, Model},
        },
    };
    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").ok();
    let mysql = url.as_ref().is_some_and(|s| s.starts_with("mysql:"));
    assert!(
        mysql || std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
        "MySQL验收要求独立测试URL"
    );
    let connection = if mysql {
        json!({"driver":"mysql","url":url.unwrap()})
    } else {
        json!({"driver":"sqlite","database":":memory:"})
    };
    let repository = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"progress","connections":{"progress":connection}}),
    )]));
    let db = DatabaseManager::new(Arc::new(repository), ".")
        .connection(None)
        .unwrap();
    db.statement("CREATE TABLE progress_contacts (id INTEGER PRIMARY KEY, name VARCHAR(64), created_at VARCHAR(64), updated_at VARCHAR(64))",&[]).await.unwrap();
    db.insert(
        "INSERT INTO progress_contacts (id,name) VALUES (1,'Ada'),(2,'Li')",
        &[],
    )
    .await
    .unwrap();
    let models = Contact::query_on(&db)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    let models = Collection::from_entries(
        models
            .into_iter()
            .enumerate()
            .map(|(i, m)| (if i == 0 { "first" } else { "second" }, m)),
    );
    let original = models.all().as_ptr();
    let input = CommandDefinition::from_signature("progress")
        .unwrap()
        .parse(&[])
        .unwrap();
    let mut output = Output::default();
    let models = input
        .io(&mut output)
        .with_progress_bar_iter_mut_async(models, |model, bar, key| {
            Box::pin(async move {
                let index = if key == ArrayKey::from("first") { 0 } else { 1 };
                assert_eq!(bar.get_progress(), index);
                model
                    .state_mut()
                    .set_attribute("name", if index == 0 { "Ada saved" } else { "Li saved" });
                assert!(model.save().await.map_err(ConsoleError::from_source)?);
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(models.all().as_ptr(), original);
    assert!(models.get_by_key("first").is_some());
    let names = Contact::query_on(&db)
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(names.all()[0].to_array().unwrap()["name"], "Ada saved");
    assert_eq!(names.all()[1].to_array().unwrap()["name"], "Li saved");
    assert_eq!(output.stdout.matches("100%").count(), 1);
    let mut mutable = models;
    for entry in mutable.entries_mut() {
        assert!(entry.1.state().exists());
    }
    db.statement("DROP TABLE progress_contacts", &[])
        .await
        .unwrap();
}
fn register() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "progress:async {mode=success}",
        AsyncCommandCallback::new(|_, input, output| {
            Box::pin(async move {
                let failure = input.argument("mode").unwrap().as_str() == Some("failure");
                let mut values = vec![1, 2];
                let mut io = input.io(output);
                io.with_progress_bar_iter_mut_async(&mut values, move |value, bar, _| {
                    Box::pin(async move {
                        bar.min_seconds_between_redraws(1000.0);
                        tokio::task::yield_now().await;
                        *value *= 2;
                        if failure {
                            return Err(ConsoleError::new("异步任务失败"));
                        }
                        Ok(())
                    })
                })
                .await?;
                io.new_line(1)?;
                io.info(&format!("{}:{}", values[0], values[1]))?;
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
#[test]
#[ignore = "由实际Artisan子进程驱动"]
fn async_cli_fixture() {
    let app = Application::configure(std::env::var("PROGRESS_ASYNC_ROOT").unwrap().into())
        .with_routing(Routing::default().commands(register))
        .create()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let args = vec![
        "progress:async".into(),
        std::env::var("PROGRESS_ASYNC_MODE").unwrap().into(),
        "--no-ansi".into(),
    ];
    let code = runtime.block_on(app.handle_command(&args)).unwrap();
    std::process::exit(code);
}
#[test]
fn actual_artisan_failure_and_next_success_keep_exit_codes_and_streams() {
    let root = tempfile::tempdir().unwrap();
    for mode in ["success", "failure", "success"] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "async_cli_fixture", "--ignored", "--nocapture"])
            .env("PROGRESS_ASYNC_ROOT", root.path())
            .env("PROGRESS_ASYNC_MODE", mode)
            .output()
            .unwrap();
        let stderr = String::from_utf8(child.stderr).unwrap();
        let stdout = String::from_utf8(child.stdout).unwrap();
        assert_eq!(
            child.status.code(),
            Some(if mode == "failure" { 1 } else { 0 })
        );
        if mode == "failure" {
            assert!(stderr.contains("异步任务失败"));
            assert!(!stderr.contains("100%"));
        } else {
            assert!(stdout.contains("2:4"));
            assert!(stderr.contains("100%"));
        }
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    use openrust::support::facades::Route;
    for (path, mode) in [("/async", "success"), ("/async/fail", "failure")] {
        Route::get(path, move || async move {
            let mut output = Output::default();
            Artisan::call("progress:async", &[mode.into()], &mut output)
                .await
                .map_err(|source| openrust::http::HttpError::Internal {
                    source: Box::new(source),
                })?;
            openrust::http::Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
        })?;
    }
    Ok(())
}
#[tokio::test]
async fn actual_http_callbacks_finish_or_fail_then_recover() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(register).web(routes))
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
    for (path, status) in [("/async", 200), ("/async/fail", 500), ("/async", 200)] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == 200 {
            assert!(
                response.json::<Value>().await.unwrap()["output"]
                    .as_str()
                    .unwrap()
                    .contains("2:4")
            );
        }
    }
    server.abort();
    let _ = server.await;
}
