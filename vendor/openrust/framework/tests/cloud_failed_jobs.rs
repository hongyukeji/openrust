use futures_util::StreamExt;
use http::{HeaderMap, HeaderValue, StatusCode};
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::cloud::{Events, FailedJobProvider},
    http::client::{Factory, Response},
    queue::{
        Payload, QueueFuture, QueueResult,
        failed::{FailedJob, FailedJobProviderInterface},
    },
    support::facades::{App, Facade},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Default)]
struct Store(Mutex<Vec<FailedJob>>);
impl FailedJobProviderInterface for Store {
    fn log<'a>(&'a self, job: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(job.clone());
            Ok(Some(job.id.clone()))
        })
    }
    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>> {
        Box::pin(async { Ok(self.0.lock().unwrap().clone()) })
    }
    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>> {
        Box::pin(async move {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|job| job.id == id)
                .cloned())
        })
    }
    fn forget<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<bool>> {
        Box::pin(async move {
            let mut rows = self.0.lock().unwrap();
            let n = rows.len();
            rows.retain(|job| job.id != id);
            Ok(n != rows.len())
        })
    }
    fn flush(&self, _: Option<Duration>) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async {
            self.0.lock().unwrap().clear();
            Ok(())
        })
    }
}
async fn app() -> TestApp {
    let mut app=TestApp::create(|builder|builder.with_config(|_,_|Ok([("app".into(),json!({"env":"testing","cipher":"AES-256-CBC","key":"0123456789abcdef0123456789abcdef"}))].into()))).unwrap();
    app.boot().await.unwrap();
    app
}
fn provider(app: &TestApp) -> FailedJobProvider {
    FailedJobProvider::new(
        Arc::new(Store::default()),
        app.app(),
        Arc::new(Events::new("unsupported://no-socket")),
    )
}
fn row(id: &str) -> Value {
    json!({"id":id,"connection":"cloud","queue":"priority","payload":serde_json::to_string(&Payload::new("marker",json!({"id":id})).unwrap()).unwrap(),"exception":"original","failed_at":"2026-11-24 10:00:00"})
}
fn encrypted(app: &TestApp, value: &Value, version: Option<&str>) -> Response {
    let token = app
        .app()
        .container()
        .make::<openrust::encryption::Encrypter>()
        .unwrap()
        .encrypt_string(value.to_string().as_bytes())
        .unwrap();
    let mut headers = HeaderMap::new();
    if let Some(version) = version {
        headers.insert(
            "cloud-payload-version",
            HeaderValue::from_str(version).unwrap(),
        );
    }
    Response::new(StatusCode::OK, headers, token.into_bytes())
}
#[tokio::test]
async fn encrypted_paginated_find_is_lazy_keeps_composite_keys_and_records_only_yielded_jobs() {
    let app = app().await;
    let provider = provider(&app);
    let factory = app.app().container().make::<Factory>().unwrap();
    let first = encrypted(
        &app,
        &json!({"data":[row("one"),row("two")],"links":{"self":"https://cloud.test/page-1","next":"https://cloud.test/page-2"}}),
        Some("1"),
    );
    let second = encrypted(
        &app,
        &json!({"data":[row("three")],"links":{"self":"https://cloud.test/page-2","next":null}}),
        Some("1"),
    );
    let responses = Mutex::new(std::collections::VecDeque::from([first, second]));
    factory.fake(move |_| {
        Some(Ok(responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("each page requested once")))
    });
    App::set_facade_application(Some(app.app()), async {
        let mut rows = provider
            .find("https://cloud.test/jobs")
            .await
            .unwrap()
            .unwrap();
        assert!(factory.recorded().is_empty());
        let (key, job) = rows.next().await.unwrap().unwrap();
        assert_eq!(
            key,
            source_contract()["first_key"]
                .as_str()
                .unwrap()
                .replace("/first:", "/page-1:")
        );
        assert_eq!(job.id, "one");
        assert_eq!(factory.recorded().len(), 1);
        assert!(
            !provider
                .forget("https://cloud.test/page-1:two")
                .await
                .unwrap()
        );
        assert!(!provider.forget(&key).await.unwrap());
        assert!(!provider.forget(&key).await.unwrap());
        let rest: Vec<_> = rows
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rest.len(), 2);
        assert_eq!(rest[1].0, "https://cloud.test/page-2:three");
        assert_eq!(factory.recorded().len(), 2);
        for (request, _) in factory.recorded() {
            assert_eq!(request.headers["cloud-payload-version"], "1");
            assert_eq!(request.headers["cloud-encryption-cipher"], "AES-256-CBC");
            assert!(!request.headers.contains_key("authorization"));
        }
    })
    .await;
}
#[tokio::test]
async fn legacy_single_download_and_empty_first_page_follow_the_fixed_iterator_boundary() {
    let app = app().await;
    let provider = provider(&app);
    let factory = app.app().container().make::<Factory>().unwrap();
    let first = encrypted(&app, &row("legacy"), None);
    let second = encrypted(
        &app,
        &json!({"data":[],"links":{"self":"https://cloud.test/empty","next":"https://cloud.test/must-not-fetch"}}),
        Some("1"),
    );
    let responses = Mutex::new(std::collections::VecDeque::from([first, second]));
    factory.fake(move |_| Some(Ok(responses.lock().unwrap().pop_front().unwrap())));
    let mut stream = provider
        .find("https://cloud.test/legacy")
        .await
        .unwrap()
        .unwrap();
    let (key, job) = stream.next().await.unwrap().unwrap();
    assert_eq!(key, "https://cloud.test/legacy:legacy");
    assert_eq!(job.payload.name(), "marker");
    assert!(stream.next().await.is_none());
    let mut stream = provider
        .find("https://cloud.test/empty")
        .await
        .unwrap()
        .unwrap();
    assert!(stream.next().await.is_none());
    assert_eq!(factory.recorded().len(), 2);
}
#[tokio::test]
async fn forbidden_download_does_not_retry_and_invalid_ciphertext_or_version_does_not_yield_a_job()
{
    for mode in ["forbidden", "ciphertext", "version"] {
        let app = app().await;
        let provider = provider(&app);
        let factory = app.app().container().make::<Factory>().unwrap();
        let response = match mode {
            "forbidden" => Response::new(StatusCode::FORBIDDEN, HeaderMap::new(), Vec::new()),
            "ciphertext" => Response::new(StatusCode::OK, HeaderMap::new(), b"invalid".to_vec()),
            _ => encrypted(&app, &row("bad"), Some("2")),
        };
        factory.fake(move |_| Some(Ok(response.clone())));
        let mut stream = provider
            .find("https://cloud.test/failed")
            .await
            .unwrap()
            .unwrap();
        assert!(stream.next().await.unwrap().is_err());
        assert_eq!(factory.recorded().len(), 1);
    }
}
#[tokio::test]
async fn ordinary_failed_records_and_non_https_lookups_remain_in_the_parent_store() {
    let app = app().await;
    let parent = Arc::new(Store::default());
    let provider = FailedJobProvider::new(
        parent.clone(),
        app.app(),
        Arc::new(Events::new("unsupported://no-socket")),
    );
    let job = FailedJob {
        id: "normal".into(),
        connection: "database".into(),
        queue: "default".into(),
        payload: Payload::new("marker", json!({})).unwrap(),
        exception: "failure".into(),
        failed_at: "2026-11-24 10:00:00".into(),
    };
    assert_eq!(provider.log(&job).await.unwrap().as_deref(), Some("normal"));
    assert_eq!(provider.ids(None).await.unwrap(), vec!["normal"]);
    assert_eq!(provider.all().await.unwrap().len(), 1);
    assert_eq!(provider.count(None, None).await.unwrap(), 0);
    assert_eq!(
        provider
            .prunable()
            .unwrap()
            .prune(jiff::Timestamp::now())
            .await
            .unwrap(),
        0
    );
    assert!(
        provider
            .find("http://cloud.test/no")
            .await
            .unwrap()
            .is_none()
    );
    let mut stream = provider.find("normal").await.unwrap().unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap().1.id, "normal");
    assert!(provider.forget("normal").await.unwrap());
    provider.log(&job).await.unwrap();
    assert!(provider.flush_queue(None, Some("default")).await.is_err());
    assert!(
        provider
            .flush_before(Some(jiff::Timestamp::now()), None)
            .await
            .is_err()
    );
    assert_eq!(parent.all().await.unwrap().len(), 1);
    provider.flush(None).await.unwrap();
    assert!(parent.all().await.unwrap().is_empty());
}

#[tokio::test]
async fn actual_database_failures_keep_filtered_counts_transactional_retry_and_pruning() {
    use openrust::{
        queue::failed::PrunableFailedJobProvider,
        support::facades::{Config, Db, Schema},
    };
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        let url = std::env::var("OPENRUST_CLOUD_TEST_DATABASE_URL").ok();
        assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_CLOUD_DATABASE").is_none());
        let connection = url.map_or_else(
            || json!({"driver":"sqlite","database":":memory:"}),
            |url| json!({"url":url}),
        );
        Config::set("database.default", "cloud-failures".into()).unwrap();
        Config::set("database.connections.cloud-failures", connection).unwrap();
        let table = format!("cloud_failed_{}", openrust::support::Str::uuid7().simple());
        Schema::create(&table, |table| {
            table.id();
            table.string("uuid", 80).unique();
            table.string("connection", 80);
            table.string("queue", 80);
            table.text("payload");
            table.text("exception");
            table.string("failed_at", 40);
        })
        .await
        .unwrap();
        Config::set(
            "queue.failed",
            json!({"driver":"database-uuids","table":table}),
        )
        .unwrap();
        let parent = app
            .app()
            .container()
            .get::<dyn FailedJobProviderInterface>("queue.failer")
            .unwrap();
        let provider = FailedJobProvider::new(
            parent,
            app.app(),
            Arc::new(Events::new("unsupported://no-socket")),
        );
        let mut job = FailedJob {
            id: "one".into(),
            connection: "database".into(),
            queue: "first".into(),
            payload: Payload::new("marker", json!({})).unwrap(),
            exception: "failure".into(),
            failed_at: "2026-01-01 00:00:00".into(),
        };
        provider.log(&job).await.unwrap();
        job.id = "two".into();
        job.queue = "second".into();
        provider.log(&job).await.unwrap();
        assert_eq!(provider.count(None, None).await.unwrap(), 2);
        assert_eq!(provider.count(Some(""), Some("0")).await.unwrap(), 2);
        assert!(provider.ids(Some("")).await.unwrap().is_empty());
        assert_eq!(
            provider
                .count(Some("database"), Some("first"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(provider.count(Some("other"), None).await.unwrap(), 0);
        let database = Db::connection(None).unwrap();
        let mut transaction = database.begin_transaction().await.unwrap();
        assert_eq!(
            provider
                .forget_retry_in_transaction(&job, &mut transaction)
                .await
                .unwrap(),
            Some(true)
        );
        transaction.roll_back(None).await.unwrap();
        assert_eq!(provider.count(None, None).await.unwrap(), 2);
        let mut transaction = database.begin_transaction().await.unwrap();
        assert_eq!(
            provider
                .forget_retry_in_transaction(&job, &mut transaction)
                .await
                .unwrap(),
            Some(true)
        );
        transaction.commit().await.unwrap();
        assert_eq!(provider.count(None, None).await.unwrap(), 1);
        assert_eq!(
            provider
                .prune("2026-11-24T00:00:00Z".parse().unwrap())
                .await
                .unwrap(),
            1
        );
        assert_eq!(provider.count(None, None).await.unwrap(), 0);
        {
            use openrust::{
                console::Output, queue::console::FlushFailedCommand, support::facades::Artisan,
            };
            let provider = Arc::new(provider);
            app.app()
                .container()
                .instance_named(
                    "queue.failer",
                    provider.clone() as Arc<dyn FailedJobProviderInterface>,
                )
                .unwrap();
            provider.log(&job).await.unwrap();
            let mut extra = job.clone();
            extra.id = "future".into();
            extra.queue = "".into();
            extra.failed_at = "2099-01-01 00:00:00".into();
            provider.log(&extra).await.unwrap();
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("queue:flush", &["--queue=second".into()], &mut output)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(provider.count(None, None).await.unwrap(), 1);
            assert!(output.stdout.contains("second"));
            FlushFailedCommand::prohibit(true).unwrap();
            let mut output = Output::default();
            assert_eq!(
                Artisan::call("queue:flush", &[], &mut output)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(provider.count(None, None).await.unwrap(), 1);
            assert!(output.stdout.contains("WARN"));
            FlushFailedCommand::prohibit(false).unwrap();
            assert_eq!(
                Artisan::call_silently("queue:flush", &["--hours=0".into(), "--queue=".into()])
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(provider.count(None, None).await.unwrap(), 0);
            provider.log(&extra).await.unwrap();
            provider.log(&job).await.unwrap();
            assert_eq!(
                Artisan::call_silently("queue:flush", &["--hours=0.0".into()])
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(provider.count(None, None).await.unwrap(), 1);
            let mut near = extra.clone();
            near.id = "near-future".into();
            near.failed_at = jiff::Timestamp::now()
                .checked_add(jiff::SignedDuration::from_mins(30))
                .unwrap()
                .strftime("%Y-%m-%d %H:%M:%S")
                .to_string();
            provider.log(&near).await.unwrap();
            assert_eq!(
                Artisan::call_silently("queue:flush", &["--hours=-1".into()])
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(provider.count(None, None).await.unwrap(), 1);
            let mut old = job.clone();
            old.id = "fraction-old".into();
            old.failed_at = jiff::Timestamp::now()
                .checked_sub(jiff::SignedDuration::from_mins(40))
                .unwrap()
                .strftime("%Y-%m-%d %H:%M:%S")
                .to_string();
            provider.log(&old).await.unwrap();
            let mut fresh = job.clone();
            fresh.id = "fraction-new".into();
            fresh.failed_at = jiff::Timestamp::now()
                .checked_sub(jiff::SignedDuration::from_mins(20))
                .unwrap()
                .strftime("%Y-%m-%d %H:%M:%S")
                .to_string();
            provider.log(&fresh).await.unwrap();
            assert_eq!(
                Artisan::call_silently("queue:flush", &["--hours=0.5".into()])
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(provider.count(None, None).await.unwrap(), 2);
            assert!(provider.find("fraction-old").await.unwrap().is_none());
            assert!(
                Artisan::call_silently("queue:flush", &["--hours=NaN".into()])
                    .await
                    .is_err()
            );
        }
        Schema::drop_if_exists(&table).await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn all_existing_application_ciphers_decrypt_downloads_and_announce_the_selected_cipher() {
    for (cipher, key) in [
        ("AES-128-CBC", "0123456789abcdef"),
        ("AES-256-CBC", "0123456789abcdef0123456789abcdef"),
        ("AES-128-GCM", "0123456789abcdef"),
        ("AES-256-GCM", "0123456789abcdef0123456789abcdef"),
    ] {
        let app = app().await;
        let config = app.app().config().unwrap();
        config.set("app.cipher", cipher.into()).unwrap();
        config.set("app.key", key.into()).unwrap();
        let provider = provider(&app);
        let response = encrypted(&app, &row("cipher"), None);
        let factory = app.app().container().make::<Factory>().unwrap();
        factory.fake(move |_| Some(Ok(response.clone())));
        let mut rows = provider
            .find("https://cloud.test/cipher")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rows.next().await.unwrap().unwrap().1.id, "cipher");
        assert_eq!(
            factory.recorded()[0].0.headers["cloud-encryption-cipher"],
            cipher
        );
    }
}
#[tokio::test]
async fn server_errors_retry_but_the_factory_global_middleware_is_restored_after_download() {
    let app = app().await;
    let provider = provider(&app);
    let response = encrypted(&app, &row("retried"), None);
    let factory = app.app().container().make::<Factory>().unwrap();
    let responses = Mutex::new(std::collections::VecDeque::from([
        Response::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HeaderMap::new(),
            Vec::new(),
        ),
        response,
    ]));
    factory.fake(move |_| {
        Some(Ok(responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("only the specified attempts")))
    });
    factory.global_request_middleware(|_| Err(openrust::http::client::Error::InvalidRequest));
    let mut rows = provider
        .find("https://cloud.test/retry")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rows.next().await.unwrap().unwrap().1.id, "retried");
    assert_eq!(factory.recorded().len(), 2);
    assert!(matches!(
        factory.get("https://cloud.test/still-global").await,
        Err(openrust::http::client::Error::InvalidRequest)
    ));
}

#[tokio::test]
async fn falsy_records_stop_iteration_without_fetching_remaining_rows_or_next_page() {
    for value in [
        Value::Null,
        json!(false),
        json!(0),
        json!(0.0),
        json!(""),
        json!("0"),
        json!([]),
    ] {
        let app = app().await;
        let provider = provider(&app);
        let response = encrypted(
            &app,
            &json!({"data":[value,row("must-not-yield")],"links":{"self":"https://cloud.test/first","next":"https://cloud.test/must-not-fetch"}}),
            Some("1"),
        );
        let factory = app.app().container().make::<Factory>().unwrap();
        factory.fake(move |_| Some(Ok(response.clone())));
        let mut rows = provider
            .find("https://cloud.test/first")
            .await
            .unwrap()
            .unwrap();
        assert!(rows.next().await.is_none());
        assert_eq!(factory.recorded().len(), 1);
    }
}

#[tokio::test]
async fn numeric_remote_ids_keep_the_event_type_and_retry_confirmation_is_consumed_once() {
    use tokio::io::AsyncBufReadExt;
    let app = app().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let received = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = tokio::io::BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    });
    let provider = FailedJobProvider::new(
        Arc::new(Store::default()),
        app.app(),
        Arc::new(Events::new(format!("tcp://{address}"))),
    );
    let mut record = row("placeholder");
    record["id"] = 7.into();
    let response = encrypted(&app, &record, None);
    let factory = app.app().container().make::<Factory>().unwrap();
    factory.fake(move |_| Some(Ok(response.clone())));
    let mut rows = provider
        .find(&format!("https://cloud.test/{}", "numeric"))
        .await
        .unwrap()
        .unwrap();
    let (key, job) = rows.next().await.unwrap().unwrap();
    assert_eq!(key, "https://cloud.test/numeric:7");
    assert_eq!(job.id, "7");
    assert!(provider.forget(&key).await.unwrap());
    assert!(!provider.forget(&key).await.unwrap());
    let event = received.await.unwrap();
    assert_eq!(event["id"], 7);
    assert_eq!(event["queue"], "priority");
    assert_eq!(event["_cloud_event"], "failed_job");
    assert_eq!(event["retried_at"].as_str().unwrap().len(), 26);
}
#[tokio::test]
async fn loaded_retry_records_do_not_cross_application_execution_scopes() {
    let app = app().await;
    let provider = provider(&app);
    let response = encrypted(&app, &row("scoped"), None);
    let factory = app.app().container().make::<Factory>().unwrap();
    factory.fake(move |_| Some(Ok(response.clone())));
    App::set_facade_application(Some(app.app()), async {
        let mut rows = provider
            .find("https://cloud.test/scoped")
            .await
            .unwrap()
            .unwrap();
        assert!(rows.next().await.unwrap().is_ok());
    })
    .await;
    App::set_facade_application(Some(app.app()), async {
        assert!(
            !provider
                .forget("https://cloud.test/scoped:scoped")
                .await
                .unwrap()
        );
    })
    .await;
}
#[tokio::test]
async fn flush_prohibition_is_application_local_and_runs_before_resolving_an_invalid_storage_driver()
 {
    use openrust::{
        console::Output,
        queue::console::FlushFailedCommand,
        support::facades::{Artisan, Config},
    };
    let first = app().await;
    let second = app().await;
    App::set_facade_application(Some(first.app()), async {
        Config::set("queue.failed.driver", "invalid-test-driver".into()).unwrap();
        FlushFailedCommand::prohibit(true).unwrap();
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("queue:flush", &[], &mut output)
                .await
                .unwrap(),
            0
        );
        assert!(output.stdout.contains("WARN"));
        assert_eq!(
            Artisan::call_silently("queue:flush", &["--hours=NaN".into()])
                .await
                .unwrap(),
            0
        );
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        let mut output = Output::default();
        assert_eq!(
            Artisan::call("queue:flush", &[], &mut output)
                .await
                .unwrap(),
            0
        );
        assert!(!output.stdout.contains("WARN"));
    })
    .await;
    App::set_facade_application(Some(first.app()), async {
        FlushFailedCommand::prohibit(false).unwrap();
        assert!(Artisan::call_silently("queue:flush", &[]).await.is_err());
    })
    .await;
}

fn source_contract() -> Value {
    serde_json::from_str(include_str!("fixtures/cloud/failed-jobs-contract.json")).unwrap()
}

#[tokio::test]
async fn real_artisan_retry_downloads_and_requeues_an_encrypted_record_then_real_worker_consumes_it()
 {
    use axum::{body::Bytes, extract::State};
    use openrust::{
        contracts::queue::JobHandler,
        queue::{JobContext, QueueManager},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Default)]
    struct StateData {
        body: Option<String>,
        actions: Vec<String>,
    }
    async fn sqs(
        State(state): State<Arc<Mutex<StateData>>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> ([(&'static str, &'static str); 1], String) {
        let action = headers["x-amz-target"]
            .to_str()
            .unwrap()
            .split('.')
            .next_back()
            .unwrap()
            .to_owned();
        let data: Value = serde_json::from_slice(&body).unwrap();
        let mut state = state.lock().unwrap();
        state.actions.push(action.clone());
        let response = match action.as_str() {
            "SendMessage" => {
                state.body = Some(data["MessageBody"].as_str().unwrap().into());
                json!({"MessageId":"retry-native"})
            }
            "ReceiveMessage" => {
                json!({"Messages":[{"MessageId":"retry-native","ReceiptHandle":"receipt","Body":state.body.take().unwrap(),"Attributes":{"ApproximateReceiveCount":"1"}}]})
            }
            "DeleteMessage" => json!({}),
            _ => panic!("unexpected {action}"),
        };
        (
            [("content-type", "application/x-amz-json-1.0")],
            response.to_string(),
        )
    }
    struct Marker(Arc<AtomicUsize>);
    impl JobHandler for Marker {
        fn handle<'a>(
            &'a self,
            _: &'a mut JobContext<'_>,
            payload: &'a Payload,
        ) -> QueueFuture<'a, QueueResult<()>> {
            Box::pin(async move {
                assert_eq!(payload.data()["id"], "retry");
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = Arc::new(Mutex::new(StateData::default()));
    let router = axum::Router::new()
        .route("/", axum::routing::post(sqs))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut app=TestApp::create(|builder|builder.with_config(|_,env|Ok([
    ("app".into(),json!({"env":"testing","cipher":"AES-256-CBC","key":"0123456789abcdef0123456789abcdef"})),
    ("queue".into(),json!({"default":"cloud","failed":{"driver":"null"},"connections":{"cloud":{"driver":"cloud","queues":["default"],"connection":{"queue":"default","key":"test","secret":"test","region":"us-east-1","endpoint":env.get("CLOUD_TEST_SQS_ENDPOINT"),"prefix":format!("{}/account",env.get("CLOUD_TEST_SQS_ENDPOINT").unwrap())}}}})),
 ].into()))).unwrap();
    std::fs::write(
        app.root().join(".env"),
        format!("LARAVEL_CLOUD=1\nCLOUD_TEST_SQS_ENDPOINT=http://{address}\n"),
    )
    .unwrap();
    app.boot().await.unwrap();
    let mut record = row("retry");
    record["queue"] = "default".into();
    let response = encrypted(&app, &record, None);
    let factory = app.app().container().make::<Factory>().unwrap();
    factory.fake(move |request| {
        request
            .url()
            .as_str()
            .starts_with("https://cloud.test/")
            .then(|| Ok(response.clone()))
    });
    let seen = Arc::new(AtomicUsize::new(0));
    app.app()
        .container()
        .make::<QueueManager>()
        .unwrap()
        .register("marker", Arc::new(Marker(seen.clone())))
        .unwrap();
    let retry = app
        .artisan(["queue:retry", "https://cloud.test/signed-download"])
        .await;
    retry.assert_successful();
    assert!(
        retry
            .stdout
            .contains("https://cloud.test/signed-download:retry")
    );
    assert_eq!(factory.recorded().len(), 1);
    assert!(
        state
            .lock()
            .unwrap()
            .body
            .as_ref()
            .is_some_and(|body| !body.contains("secret") && !body.contains("0123456789abcdef"))
    );
    app.artisan(["queue:work", "cloud", "--once", "--force", "--sleep=0"])
        .await
        .assert_successful();
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert_eq!(
        state.lock().unwrap().actions,
        vec!["SendMessage", "ReceiveMessage", "DeleteMessage"]
    );
    task.abort();
}

#[tokio::test]
async fn previous_application_keys_decrypt_hosted_downloads_only_when_explicitly_configured() {
    let producer = app().await;
    let response = encrypted(&producer, &row("rotated"), None);
    for previous in [true, false] {
        let app = app().await;
        let config = app.app().config().unwrap();
        config
            .set("app.key", "fedcba9876543210fedcba9876543210".into())
            .unwrap();
        if previous {
            config
                .set(
                    "app.previous_keys",
                    json!(["0123456789abcdef0123456789abcdef"]),
                )
                .unwrap();
        }
        let factory = app.app().container().make::<Factory>().unwrap();
        let response = response.clone();
        factory.fake(move |_| Some(Ok(response.clone())));
        let provider = provider(&app);
        let mut rows = provider
            .find("https://cloud.test/rotated")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rows.next().await.unwrap().is_ok(), previous);
    }
}
