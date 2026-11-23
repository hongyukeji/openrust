use openrust::{
    contracts::{
        container::Container as _,
        foundation::Application as _,
        queue::{JobHandler, Queue as _},
    },
    foundation::cloud::{CloudError, CloudMacro},
    queue::{JobContext, Payload, QueueFuture, QueueManager, QueueResult},
    support::facades::{App, Cloud, Config, Facade},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
async fn app(hosted: &str, config: Value) -> TestApp {
    app_with_environment(&format!("LARAVEL_CLOUD={hosted}\n"), config).await
}
async fn app_with_environment(environment: &str, config: Value) -> TestApp {
    let mut app = TestApp::create(|builder| {
        builder.with_config(|root, _| {
            Ok([
                ("app".into(), json!({"env":"testing"})),
                (
                    "queue".into(),
                    serde_json::from_slice(&fs::read(root.join("queue.json")).unwrap()).unwrap(),
                ),
            ]
            .into())
        })
    })
    .unwrap();
    fs::write(app.root().join(".env"), environment).unwrap();
    fs::write(app.root().join("queue.json"), config.to_string()).unwrap();
    app.boot().await.unwrap();
    app
}
fn config() -> Value {
    json!({"default":"cloud","connections":{"cloud":{"driver":"cloud","queues":{"default":{},"priority":{}},"connection":{"queue":"default","prefix":"https://sqs.test/account","suffix":"-env","region":"us-east-1","key":"test","secret":"test"}}},"failed":{"driver":"null"}})
}
#[tokio::test]
async fn manager_follows_exact_environment_and_configuration_not_default_driver() {
    for hosted in ["1", "true", "0", ""] {
        let app = app(hosted, config()).await;
        App::set_facade_application(Some(app.app()), async {
            assert_eq!(Cloud::hosted().unwrap(), hosted == "1");
            assert!(Cloud::uses_managed_queues().unwrap());
            if hosted == "1" {
                assert!(Cloud::is_managed_queue("default").unwrap());
                assert!(!Cloud::is_managed_queue("Default").unwrap());
                let queue = Cloud::queue().unwrap();
                assert!(Arc::ptr_eq(&queue, &Cloud::queue().unwrap()));
            } else {
                assert!(Cloud::queue().is_err());
            }
            Config::set("queue.connections.cloud.driver", "sqs".into()).unwrap();
            assert!(!Cloud::uses_managed_queues().unwrap());
            assert!(!Cloud::is_managed_queue("default").unwrap());
            assert!(matches!(Cloud::queue(), Err(CloudError::NotConfigured)));
        })
        .await;
    }
}
#[tokio::test]
async fn queue_metadata_normalization_and_owned_names_follow_original_connection() {
    let app = app("1", config()).await;
    App::set_facade_application(Some(app.app()), async {
        let queue = Cloud::queue().unwrap();
        assert_eq!(
            queue.get_queue(None).unwrap(),
            "https://sqs.test/account/default-env"
        );
        assert_eq!(queue.normalize_queue(None).unwrap(), "default");
        assert_eq!(
            queue.normalize_queue(Some("priority.fifo")).unwrap(),
            "priority.fifo"
        );
        let retained = queue.get_connection_name();
        queue.set_connection_name("renamed");
        assert_eq!(retained, "cloud");
        assert_eq!(queue.get_connection_name(), "renamed");
        assert_eq!(
            queue.get_queue(None).unwrap(),
            "https://sqs.test/account/default-env"
        );
        let mut changed = config()["connections"]["cloud"].clone();
        changed["connection"]["queue"] = "changed".into();
        changed["queues"] = json!(["one", "two"]);
        queue.set_config(changed).unwrap();
        assert_eq!(queue.default_queue(), "default");
        assert_eq!(queue.get_config()["queue"], "changed");
        assert_eq!(
            queue.get_queue(None).unwrap(),
            "https://sqs.test/account/default-env"
        );
        assert_eq!(
            queue.managed_queues().unwrap(),
            vec![json!("one"), json!("two")]
        );
        assert!(queue.set_config(json!({})).is_err());
    })
    .await;
}
#[tokio::test]
async fn macros_are_typed_reentrant_case_sensitive_and_application_owned() {
    let first = app("0", json!({})).await;
    let second = app("0", json!({})).await;
    App::set_facade_application(Some(first.app()), async {
        Cloud::r#macro("hosted", |cloud, (): ()| cloud.hosted()).unwrap();
        assert!(!Cloud::macro_call::<bool, _>("hosted", ()).unwrap());
        Cloud::r#macro("nested", |cloud, value: String| {
            cloud.r#macro("added", |_, (): ()| Ok(7usize));
            Ok(value)
        })
        .unwrap();
        assert_eq!(
            Cloud::macro_call::<String, _>("nested", "hello".to_owned()).unwrap(),
            "hello"
        );
        assert!(Cloud::has_macro("added").unwrap());
        assert!(!Cloud::has_macro("Added").unwrap());
        assert!(matches!(
            Cloud::macro_call::<String, _>("added", ()),
            Err(CloudError::MacroType { .. })
        ));
        assert!(matches!(
            Cloud::macro_call::<usize, _>("added", 1usize),
            Err(CloudError::MacroType { .. })
        ));
        Cloud::mixin([("added", CloudMacro::new(|_, (): ()| Ok(8usize)))], false).unwrap();
        assert_eq!(Cloud::macro_call::<usize, _>("added", ()).unwrap(), 7);
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        assert!(!Cloud::has_macro("added").unwrap());
    })
    .await;
    App::set_facade_application(Some(first.app()), async {
        Cloud::flush_macros().unwrap();
        assert!(!Cloud::has_macro("added").unwrap());
    })
    .await;
}
struct Handler(Arc<AtomicUsize>);
impl JobHandler for Handler {
    fn handle<'a>(
        &'a self,
        _context: &'a mut JobContext<'_>,
        _payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}
#[cfg(unix)]
mod agent {
    use super::*;
    use std::sync::Mutex;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::UnixListener,
    };
    struct Server {
        path: std::path::PathBuf,
        calls: Arc<Mutex<Vec<(String, Value)>>>,
        task: tokio::task::JoinHandle<()>,
        _dir: tempfile::TempDir,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    fn server(responses: Vec<(u16, Value)>) -> Server {
        let root = std::env::var_os("OPENRUST_CLOUD_SOCKET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(std::env::var_os("TMPDIR").unwrap()));
        fs::create_dir_all(&root).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("c")
            .tempdir_in(root)
            .unwrap();
        let path = dir.path().join("s");
        let listener = UnixListener::bind(&path).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let task = tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                while !data.ends_with(b"\r\n\r\n") {
                    data.push(stream.read_u8().await.unwrap());
                }
                let header = String::from_utf8(data).unwrap();
                let size = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                let mut data = vec![0; size];
                stream.read_exact(&mut data).await.unwrap();
                observed.lock().unwrap().push((
                    header.lines().next().unwrap().into(),
                    serde_json::from_slice(&data).unwrap_or(Value::Null),
                ));
                let body = if status == 204 {
                    String::new()
                } else {
                    body.to_string()
                };
                stream.write_all(format!("HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        Server {
            path,
            calls,
            task,
            _dir: dir,
        }
    }
    async fn managed(server: &Server) -> TestApp {
        let mut cfg = config();
        cfg["connections"]["cloud"]["agent"] = json!({"enabled":true,"socket":server.path});
        app("1", cfg).await
    }
    #[tokio::test]
    async fn real_artisan_worker_receives_and_reports_through_the_unix_agent() {
        let payload = Payload::new("cloud-marker", json!({"value":1})).unwrap();
        let server = server(vec![
            (
                200,
                json!({"messageId":"id","receiptHandle":17,"body":serde_json::to_string(&payload).unwrap(),"queueUrl":"https://sqs.test/account/default-env","attributes":{"ApproximateReceiveCount":"2"}}),
            ),
            (200, json!({"ok":true})),
        ]);
        let mut app = managed(&server).await;
        let seen = Arc::new(AtomicUsize::new(0));
        app.app()
            .container()
            .make::<QueueManager>()
            .unwrap()
            .register("cloud-marker", Arc::new(Handler(seen.clone())))
            .unwrap();
        app.artisan([
            "queue:work",
            "cloud",
            "--once",
            "--sleep=0",
            "--tries=3",
            "--force",
        ])
        .await
        .assert_successful();
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        let calls = server.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].0.starts_with("GET /next "));
        assert!(calls[1].0.starts_with("POST /result "));
        assert_eq!(calls[1].1, fixed()["processed"]["body"]);
    }
    struct Releasing;
    impl JobHandler for Releasing {
        fn handle<'a>(
            &'a self,
            context: &'a mut JobContext<'_>,
            _: &'a Payload,
        ) -> QueueFuture<'a, QueueResult<()>> {
            Box::pin(async move {
                context.release(std::time::Duration::from_secs(7));
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn result_server_errors_do_not_retry_or_release_an_already_processed_job() {
        for status in [400, 500] {
            let payload = Payload::new("cloud-marker", json!({})).unwrap();
            let server = server(vec![
                (
                    200,
                    json!({"messageId":"failed-result","receiptHandle":"receipt","body":serde_json::to_string(&payload).unwrap(),"queueUrl":"https://sqs.test/account/default-env"}),
                ),
                (status, json!({"error":"rejected"})),
            ]);
            let mut app = managed(&server).await;
            let seen = Arc::new(AtomicUsize::new(0));
            app.app()
                .container()
                .make::<QueueManager>()
                .unwrap()
                .register("cloud-marker", Arc::new(Handler(seen.clone())))
                .unwrap();
            app.artisan(["queue:work", "cloud", "--once", "--force", "--sleep=0"])
                .await
                .assert_exit_code(1);
            assert_eq!(seen.load(Ordering::SeqCst), 1);
            let calls = server.calls.lock().unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(
                calls[1].1,
                json!({"messageId":"failed-result","receiptHandle":"receipt","status":"processed"})
            );
        }
    }
    #[tokio::test]
    async fn explicit_release_reports_delay_and_the_original_receipt_without_sqs_visibility_calls()
    {
        let payload = Payload::new("release", json!({})).unwrap();
        let server = server(vec![
            (
                200,
                json!({"messageId":"id","receiptHandle":"receipt","body":serde_json::to_string(&payload).unwrap(),"queueUrl":"https://sqs.test/account/default-env"}),
            ),
            (200, json!({})),
        ]);
        let mut app = managed(&server).await;
        app.app()
            .container()
            .make::<QueueManager>()
            .unwrap()
            .register("release", Arc::new(Releasing))
            .unwrap();
        app.artisan(["queue:work", "cloud", "--once", "--force", "--sleep=0"])
            .await
            .assert_successful();
        assert_eq!(
            server.calls.lock().unwrap()[1].1,
            fixed()["released"]["body"]
        );
    }
    #[tokio::test]
    async fn non_array_and_http_poll_failures_exit_through_real_worker() {
        for responses in [vec![(200, json!("bad"))], vec![(503, json!({})); 3]] {
            let expected = responses.len();
            let server = server(responses);
            let mut app = managed(&server).await;
            app.artisan(["queue:work", "cloud", "--once", "--force", "--sleep=0"])
                .await
                .assert_exit_code(1);
            assert_eq!(server.calls.lock().unwrap().len(), expected);
        }
    }
    #[tokio::test]
    async fn empty_malformed_and_retry_statuses_have_the_pinned_polling_behavior() {
        for (responses, expected) in [
            (vec![(204, Value::Null)], 1),
            (vec![(200, json!({"messageId":""}))], 1),
            (vec![(200, json!([]))], 1),
            (
                vec![(500, json!({})), (503, json!({})), (204, Value::Null)],
                3,
            ),
        ] {
            let server = server(responses);
            let mut app = managed(&server).await;
            app.artisan([
                "queue:work",
                "cloud",
                "--once",
                "--sleep=0",
                "--tries=3",
                "--force",
            ])
            .await
            .assert_successful();
            assert_eq!(server.calls.lock().unwrap().len(), expected);
        }
    }
}

#[tokio::test]
async fn real_sqs_protocol_forwards_metrics_publish_reservation_and_settlement() {
    use axum::{body::Bytes, extract::State};
    use std::sync::Mutex;
    #[derive(Default)]
    struct StateData {
        calls: Vec<(String, Value)>,
        body: Option<String>,
    }
    async fn endpoint(
        State(state): State<Arc<Mutex<StateData>>>,
        headers: http::HeaderMap,
        body: Bytes,
    ) -> ([(&'static str, &'static str); 1], String) {
        let action = headers
            .get("x-amz-target")
            .unwrap()
            .to_str()
            .unwrap()
            .split('.')
            .next_back()
            .unwrap()
            .to_owned();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let mut state = state.lock().unwrap();
        state.calls.push((action.clone(), body.clone()));
        ([("content-type","application/x-amz-json-1.0")],match action.as_str(){
            "SendMessage"=>{state.body=Some(body["MessageBody"].as_str().unwrap().into());json!({"MessageId":"sqs-1"})},
            "ReceiveMessage"=>match state.body.take(){Some(body)=>json!({"Messages":[{"MessageId":"sqs-1","ReceiptHandle":"sqs-receipt","Body":body,"Attributes":{"ApproximateReceiveCount":"1"}}]}),None=>json!({})},
            "GetQueueAttributes"=>json!({"Attributes":{"ApproximateNumberOfMessages":"3","ApproximateNumberOfMessagesDelayed":"2","ApproximateNumberOfMessagesNotVisible":"1"}}),
            "DeleteMessage"|"PurgeQueue"=>json!({}),
            _=>panic!("unexpected SQS action {action}"),
        }.to_string())
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = Arc::new(Mutex::new(StateData::default()));
    let router = axum::Router::new()
        .route("/", axum::routing::post(endpoint))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut cfg = config();
    cfg["connections"]["cloud"]["connection"]["endpoint"] = format!("http://{address}").into();
    let mut app = app("1", cfg).await;
    let seen = Arc::new(AtomicUsize::new(0));
    app.app()
        .container()
        .make::<QueueManager>()
        .unwrap()
        .register("cloud-marker", Arc::new(Handler(seen.clone())))
        .unwrap();
    App::set_facade_application(Some(app.app()), async {
        let queue = Cloud::queue().unwrap();
        assert_eq!(queue.size(None).await.unwrap(), 6);
        assert_eq!(queue.total_size().await.unwrap(), 12);
        assert_eq!(queue.total_pending_size().await.unwrap(), 6);
        assert_eq!(queue.total_delayed_size().await.unwrap(), 4);
        assert_eq!(queue.total_reserved_size().await.unwrap(), 2);
        queue
            .push(&Payload::new("cloud-marker", json!({})).unwrap(), None)
            .await
            .unwrap();
    })
    .await;
    app.artisan(["queue:work", "cloud", "--once", "--force", "--sleep=0"])
        .await
        .assert_successful();
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    let calls = &state.lock().unwrap().calls;
    assert!(calls.iter().any(|(name, _)| name == "SendMessage"));
    assert!(calls.iter().any(|(name, _)| name == "ReceiveMessage"));
    assert_eq!(
        calls
            .iter()
            .filter(|(name, _)| name == "DeleteMessage")
            .count(),
        1
    );
    assert!(calls.iter().all(|(_, body)| {
        body["QueueUrl"].as_str().is_some_and(|q| {
            q == "https://sqs.test/account/default-env"
                || q == "https://sqs.test/account/priority-env"
        })
    }));
    task.abort();
}

#[tokio::test]
async fn hosted_json_configuration_applies_nullable_after_commit_and_existing_overflow_defaults() {
    let mut cloud = config()["connections"]["cloud"].clone();
    cloud["connection"]["after_commit"] = Value::Null;
    let environment = format!(
        "LARAVEL_CLOUD=1\nCLOUD_QUEUE_AFTER_COMMIT=(true)\nCLOUD_QUEUE_OVERFLOW_ENABLED=(false)\nCLOUD_QUEUE_OVERFLOW_STORE=private\nLARAVEL_CLOUD_MANAGED_QUEUES_CONFIG='{}'\n",
        cloud
    );
    let app = app_with_environment(
        &environment,
        json!({"default":"sync","connections":{"sync":{"driver":"sync"}}}),
    )
    .await;
    App::set_facade_application(Some(app.app()), async {
        assert!(Cloud::hosted().unwrap());
        assert!(Cloud::uses_managed_queues().unwrap());
        let config = Cloud::queue().unwrap().get_config();
        assert_eq!(config["after_commit"], true);
        assert_eq!(
            config["overflow"],
            json!({"enabled":false,"store":"private","always":false,"delete_after_processing":true})
        );
    })
    .await;
}
#[tokio::test]
async fn macro_factories_skip_retained_entries_and_preserve_partial_progress_on_original_errors() {
    let app = app("0", json!({})).await;
    App::set_facade_application(Some(app.app()), async {
        Cloud::r#macro("existing", |_, (): ()| Ok(1u8)).unwrap();
        let seen = AtomicUsize::new(0);
        Cloud::mixin_using(
            [("existing", || {
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(CloudMacro::new(|_, (): ()| Ok(2u8)))
            })],
            false,
        )
        .unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 0);
        fn good() -> Result<CloudMacro, CloudError> {
            Ok(CloudMacro::new(|_, (): ()| Ok(3u8)))
        }
        fn bad() -> Result<CloudMacro, CloudError> {
            Err(CloudError::callback(std::io::Error::other(
                "original factory failure",
            )))
        }
        let result = Cloud::mixin_using(
            [
                ("first", good as fn() -> Result<CloudMacro, CloudError>),
                ("second", bad),
            ],
            true,
        );
        let error = result.unwrap_err();
        assert!(matches!(error, CloudError::Callback(_)));
        assert!(error.to_string().contains("original factory failure"));
        assert_eq!(Cloud::macro_call::<u8, _>("first", ()).unwrap(), 3);
        assert!(!Cloud::has_macro("second").unwrap());
    })
    .await;
}

fn fixed() -> Value {
    serde_json::from_str(include_str!("fixtures/cloud/agent-contract.json")).unwrap()
}
