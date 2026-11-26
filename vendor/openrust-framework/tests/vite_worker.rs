use openrust::{
    contracts::{container::Container as _, foundation::Application as _, queue::JobHandler},
    database::DatabaseManager,
    queue::{DispatchOptions, Payload, QueueFuture, QueueResult},
    support::facades::{App, Facade, Queue, Vite},
    testing::TestApp,
};
use serde_json::json;
use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

struct ViteJob {
    sequence: AtomicUsize,
    completed: Arc<Mutex<Vec<String>>>,
}
impl JobHandler for ViteJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        _: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async {
            let result = (|| -> openrust::foundation::vite::ViteResult<String> {
                assert!(
                    Vite::csp_nonce()?.is_none(),
                    "下一任务不能继承前一任务的nonce"
                );
                assert!(
                    Vite::preloaded_assets()?.is_empty(),
                    "下一任务不能继承预加载状态"
                );
                let id = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
                let nonce = format!("job-{id}");
                Vite::use_csp_nonce(Some(&nonce))?;
                Vite::use_script_tag_attributes(
                    [
                        ("data-job".into(), id.into()),
                        (format!("data-job-{id}"), true.into()),
                    ]
                    .into(),
                )?;
                if id == 1 {
                    Vite::prefetch(Some(2), Some("job:ready"))?;
                }
                assert_eq!(
                    Vite::macro_call::<Option<String>, _>("worker_nonce", ())?,
                    Some(nonce.clone())
                );
                let html = Vite::render(["resources/js/app.js"], None)?
                    .to_html()
                    .to_owned();
                assert_eq!(
                    html.contains("window.addEventListener('job:ready'"),
                    id == 1
                );
                assert!(html.contains(&format!("nonce=\"{nonce}\"")));
                assert!(html.contains(&format!("data-job=\"{id}\"")));
                if id > 1 {
                    assert!(!html.contains("data-job-1"), "下一任务不能继承标签属性");
                }
                let font = Vite::fonts(Some(&["mono"]))?;
                assert!(font.to_html().contains("mono.woff2"));
                assert!(font.to_html().contains(&format!("nonce=\"{nonce}\"")));
                assert!(
                    !Vite::fonts(Some(&["mono"]))?
                        .to_html()
                        .contains("mono.woff2")
                );
                assert!(!Vite::preloaded_assets()?.is_empty());
                Ok(html)
            })()
            .map_err(openrust::queue::QueueError::execution)?;
            self.completed.lock().unwrap().push(result);
            Ok(())
        })
    }
}
async fn worker(driver: &str, url: Option<String>, port: Option<u16>) {
    let id = format!("{:016x}", fastrand::u64(..));
    let table = format!("vite_jobs_{id}");
    let config = json!({"app":{"env":"testing","url":"https://app.test"},"database":{"default":"acceptance","connections":{"acceptance":{"url":url}},"redis":{"options":{"prefix":format!("vite-worker:{id}:")},"default":{"port":port.unwrap_or(6379)}}},"queue":{"default":driver,"connections":{"database":{"driver":"database","table":table,"retry_after":30},"redis":{"driver":"redis","retry_after":30,"block_for":0}},"failed":{"driver":"null"}}});
    let mut app = TestApp::create(|b| {
        b.with_config(|root, _| {
            Ok(serde_json::from_slice(&fs::read(root.join("worker.json")).unwrap()).unwrap())
        })
    })
    .unwrap();
    fs::write(app.root().join("worker.json"), config.to_string()).unwrap();
    fs::create_dir_all(app.root().join("public/build")).unwrap();
    fs::write(
        app.root().join("public/build/manifest.json"),
        include_bytes!("fixtures/vite/manifest.json"),
    )
    .unwrap();
    fs::write(
        app.root().join("public/build/fonts-manifest.json"),
        include_bytes!("fixtures/vite/fonts.json"),
    )
    .unwrap();
    app.boot().await.unwrap();
    if driver == "database" {
        app.app()
            .container()
            .make::<DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap()
            .get_schema_builder()
            .create(&table, |t| {
                t.id();
                t.string("queue", 255).index();
                t.long_text("payload");
                t.unsigned_integer("attempts");
                t.unsigned_big_integer("reserved_at").nullable();
                t.unsigned_big_integer("available_at");
                t.unsigned_big_integer("created_at");
            })
            .await
            .unwrap();
    }
    let completed = Arc::new(Mutex::new(vec![]));
    App::set_facade_application(Some(app.app()), async {
        Vite::r#macro("worker_nonce", |vite, (): ()| vite.csp_nonce()).unwrap();
        Queue::register(
            "vite-render",
            Arc::new(ViteJob {
                sequence: AtomicUsize::new(0),
                completed: completed.clone(),
            }),
        )
        .unwrap();
        Queue::without_interruption_polling().unwrap();
        for _ in 0..2 {
            Queue::dispatch(
                Payload::new("vite-render", ()).unwrap(),
                DispatchOptions {
                    queue: Some("isolated-vite".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        }
    })
    .await;
    app.artisan([
        "queue:work",
        driver,
        "--queue=isolated-vite",
        "--stop-when-empty",
        "--sleep=0",
        "--timeout=0",
        "--tries=1",
    ])
    .await
    .assert_successful();
    {
        let values = completed.lock().unwrap();
        assert_eq!(values.len(), 2);
        assert!(values[0].contains(r#"nonce="job-1""#));
        assert!(!values[1].contains(r#"nonce="job-1""#));
    }
    App::set_facade_application(Some(app.app()), async {
        assert!(Vite::csp_nonce().unwrap().is_none());
        assert!(Vite::preloaded_assets().unwrap().is_empty());
    })
    .await;
    if driver == "database" {
        app.app()
            .container()
            .make::<DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap()
            .get_schema_builder()
            .drop_if_exists(&table)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn mysql_worker_keeps_vite_runtime_separate_per_job() {
    match std::env::var("OPENRUST_TEST_MYSQL_URL") {
        Ok(url) => worker("database", Some(url), None).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "缺少MySQL验收夹具"
        ),
    }
}
#[tokio::test]
async fn redis_worker_keeps_vite_runtime_separate_per_job() {
    match std::env::var("OPENRUST_TEST_REDIS_PORT") {
        Ok(port) => worker("redis", None, Some(port.parse().unwrap())).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_QUEUE_REDIS").is_none(),
            "缺少Redis验收夹具"
        ),
    }
}
