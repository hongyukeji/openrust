use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use futures_util::stream;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::DatabaseManager,
    foundation::Application,
    http::HttpFuture,
    queue::{
        Payload, QueueError, QueueFuture, QueueManager, QueueResult,
        failed::{FailedJob, FailedJobProviderInterface, FailedJobRetryStream},
    },
    support::{ProviderResult, ServiceProvider},
};
use serde_json::{Value, json};

struct CollectionProvider(PathBuf);

impl CollectionProvider {
    fn record(&self, action: &str, key: &str) -> QueueResult<()> {
        let result = (|| {
            let mut journal = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.0.join("journal"))?;
            writeln!(journal, "{action}:{key}")
        })();
        result.map_err(|source| QueueError::Execution {
            source: Box::new(source),
        })
    }

    fn job(key: &str) -> QueueResult<FailedJob> {
        Ok(FailedJob {
            id: format!("display-{key}"),
            connection: if key == "bad" { "missing" } else { "database" }.into(),
            queue: "retries".into(),
            payload: Payload::new("probe", key)?,
            exception: "original failure".into(),
            failed_at: "2000-01-01 00:00:00".into(),
        })
    }
}

impl FailedJobProviderInterface for CollectionProvider {
    fn log<'a>(&'a self, _: &'a FailedJob) -> QueueFuture<'a, QueueResult<Option<String>>> {
        Box::pin(async { Ok(None) })
    }

    fn all(&self) -> QueueFuture<'_, QueueResult<Vec<FailedJob>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn ids<'a>(&'a self, queue: Option<&'a str>) -> QueueFuture<'a, QueueResult<Vec<String>>> {
        Box::pin(async move {
            Ok(if queue.is_none_or(|queue| queue == "retries") {
                vec!["group".into(), "empty".into(), "single".into()]
            } else {
                Vec::new()
            })
        })
    }

    fn find<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<Option<FailedJob>>> {
        Box::pin(async move { (id == "single").then(|| Self::job(id)).transpose() })
    }

    #[allow(non_snake_case, reason = "Implements the retry stream contract")]
    fn findForRetry<'a>(
        &'a self,
        id: &'a str,
    ) -> QueueFuture<'a, QueueResult<Option<FailedJobRetryStream<'a>>>> {
        Box::pin(async move {
            let keys: &[&str] = match id {
                "group" => &["external:first", "external:second"],
                "single" => &["single"],
                "empty" => &[],
                "broken" => &["external:first", "bad", "unread"],
                "read-error" => &["fault", "unread"],
                _ => return Ok(None),
            };
            let entries: FailedJobRetryStream<'a> =
                Box::pin(stream::iter(keys.iter().map(|&key| {
                    self.record("fetch", key)?;
                    if key == "fault" {
                        return Err(QueueError::Execution {
                            source: Box::new(std::io::Error::other("failure stream unavailable")),
                        });
                    }
                    Ok((key.to_owned(), Self::job(key)?))
                })));
            Ok(Some(entries))
        })
    }

    fn forget<'a>(&'a self, id: &'a str) -> QueueFuture<'a, QueueResult<bool>> {
        Box::pin(async move {
            self.record("forget", id)?;
            Ok(true)
        })
    }

    fn flush(&self, _: Option<Duration>) -> QueueFuture<'_, QueueResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

struct Provider(PathBuf);

impl ServiceProvider for Provider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = self.0.clone();
            app.container()
                .singletonNamed::<dyn FailedJobProviderInterface, _>("queue.failer", move |_| {
                    Ok(Arc::new(CollectionProvider(root.clone())))
                })?;
            Ok(())
        })
    }
}

fn application(root: &Path) -> Application {
    Application::configure(root.into())
        .withConfig(|base, _| {
            let database = if base.join("mysql").is_file() {
                json!({"url":std::env::var("OPENRUST_TEST_WORKER_MYSQL_URL").unwrap()})
            } else {
                json!({"driver":"sqlite", "database":base.join("fixture.sqlite")})
            };
            let Value::Object(config) = json!({
                "database": {"default":"fixture", "connections":{"fixture":database}},
                "queue": {"default":"database", "connections":{"database":{"driver":"database","table":"collection_retry_jobs"}}}
            }) else {
                unreachable!();
            };
            Ok(config.into_iter().collect())
        })
        .withProviders(vec![Box::new(Provider(root.into()))])
        .create()
        .unwrap()
}

#[tokio::test]
#[ignore = "Spawned by the Artisan integration test"]
async fn collection_artisan_fixture() {
    let root = PathBuf::from(std::env::var_os("QUEUE_COLLECTION_ROOT").unwrap());
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("QUEUE_COLLECTION_ARGS").unwrap()).unwrap();
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let status = application(&root).handleCommand(&args).await.unwrap();
    std::process::exit(status);
}

async fn retry(root: &Path, selectors: &[&str]) -> std::process::Output {
    let args = std::iter::once("queue:retry")
        .chain(selectors.iter().copied())
        .collect::<Vec<_>>();
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "collection_artisan_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("QUEUE_COLLECTION_ROOT", root)
        .env(
            "QUEUE_COLLECTION_ARGS",
            serde_json::to_string(&args).unwrap(),
        )
        .kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .unwrap()
        .unwrap()
}

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn exercise(mysql: bool) {
    let root = TempRoot(std::env::temp_dir().join(format!(
        "openrust-retry-collection-{:016x}",
        fastrand::u64(..)
    )));
    fs::create_dir(&root.0).unwrap();
    if mysql {
        fs::File::create(root.0.join("mysql")).unwrap();
    } else {
        fs::File::create(root.0.join("fixture.sqlite")).unwrap();
    }
    let mut app = application(&root.0);
    app.boot().await.unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    let mut schema = db.getSchemaBuilder();
    schema.dropIfExists("collection_retry_jobs").await.unwrap();
    schema
        .create("collection_retry_jobs", |table| {
            table.id();
            table.string("queue", 255).index();
            table.longText("payload");
            table.unsignedInteger("attempts");
            table.unsignedBigInteger("reserved_at").nullable();
            table.unsignedBigInteger("available_at");
            table.unsignedBigInteger("created_at");
        })
        .await
        .unwrap();

    let output = retry(&root.0, &["missing", "group", "empty", "single"]).await;
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    for key in ["external:first", "external:second", "single"] {
        assert!(
            stdout.contains(&format!("Queued failed job {key}.")),
            "{stdout}"
        );
    }
    assert!(
        stderr.contains("Unable to find failed job with ID [missing]."),
        "{stderr}"
    );
    assert!(
        stderr.contains("Unable to find any failed jobs with ID [empty]."),
        "{stderr}"
    );
    let journal = root.0.join("journal");
    assert_eq!(
        fs::read_to_string(&journal).unwrap(),
        "fetch:external:first\nforget:external:first\nfetch:external:second\nforget:external:second\nfetch:single\nforget:single\n",
    );
    let manager = app.container().make::<QueueManager>().unwrap();
    let queue = manager.connection(None).unwrap();
    for key in ["external:first", "external:second", "single"] {
        let mut job = queue.pop(Some("retries")).await.unwrap().unwrap();
        assert_eq!(job.payload().data(), key);
        job.delete().await.unwrap();
    }

    for selector in ["all", "--queue=retries"] {
        fs::remove_file(&journal).unwrap();
        let output = retry(&root.0, &[selector]).await;
        assert!(output.status.success(), "{output:?}");
        assert_eq!(queue.size(Some("retries")).await.unwrap(), 3);
        assert!(
            fs::read_to_string(&journal)
                .unwrap()
                .ends_with("fetch:single\nforget:single\n")
        );
        db.table("collection_retry_jobs").delete().await.unwrap();
    }

    fs::remove_file(&journal).unwrap();
    let output = retry(&root.0, &["read-error"]).await;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(fs::read_to_string(&journal).unwrap(), "fetch:fault\n");
    assert_eq!(queue.size(Some("retries")).await.unwrap(), 0);

    fs::remove_file(&journal).unwrap();
    let output = retry(&root.0, &["broken"]).await;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        fs::read_to_string(&journal).unwrap(),
        "fetch:external:first\nforget:external:first\nfetch:bad\n"
    );
    assert_eq!(queue.size(Some("retries")).await.unwrap(), 1);
    schema.dropIfExists("collection_retry_jobs").await.unwrap();
}

#[tokio::test]
async fn artisan_retries_collections_lazily_by_storage_key() {
    exercise(false).await;
    if std::env::var_os("OPENRUST_TEST_WORKER_MYSQL_URL").is_some() {
        exercise(true).await;
    }
}
