//! 新库Files安装→真实migrate→组合Source及业务命令写删的实际调用链。
use crate::app::providers::translation_sources::TranslationSourcesProvider;
use crate::modules::example::services::translation_source::{
    TranslationSourceExample, TranslationSourceExampleProvider,
};
use openrust::database::{
    DatabaseServiceProvider,
    sea_orm_migration::{MigrationTrait, MigratorTrait},
};
use openrust::foundation::{Application, ProviderBuilder};
use openrust::testing::TestDatabase;
use openrust::{Error, Result};
struct Empty;
impl MigratorTrait for Empty {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}
async fn app(database: &TestDatabase, files: &std::path::Path, mode: &str) -> Result<Application> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(TranslationSourcesProvider)
        .provider(TranslationSourceExampleProvider);
    Application::from_provider_runtime(builder.start(figment::Figment::from(figment::providers::Serialized::defaults(
        serde_json::json!({"app":{"env":"testing"},"database":{"url":database.url()},"translation_sources":{"mode":mode,"install_database":true,"file_root":files,"namespaces":{"example-source":files.parent().ok_or_else(|| Error::custom("test_source_fixture", "测试来源缺少父目录"))?.join("example-source")}}})
    ))).await?)
}
#[tokio::test]
async fn fresh_install_migration_and_real_example_commands_share_the_selected_source() -> Result<()>
{
    let database = if let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") {
        TestDatabase::create::<Empty>(&url).await?
    } else {
        #[cfg(feature = "db-sqlite")]
        {
            TestDatabase::sqlite_file::<Empty>().await?
        }
        #[cfg(not(feature = "db-sqlite"))]
        {
            eprintln!("未执行Source安装实库验收：需OPENRUST_TEST_DATABASE_URL或db-sqlite");
            return Ok(());
        }
    };
    let temporary = tempfile::tempdir().map_err(Error::internal)?;
    let files = temporary.path().join("lang");
    std::fs::create_dir(&files).map_err(Error::internal)?;
    std::fs::write(files.as_path().join("en.json"), "{}").map_err(Error::internal)?;
    let namespace = temporary.path().join("example-source");
    std::fs::create_dir(&namespace).map_err(Error::internal)?;
    std::fs::write(namespace.join("en.json"), r#"{"label":"file fallback"}"#)
        .map_err(Error::internal)?;
    std::fs::write(namespace.join("fr.json"), r#"{"label":"fr exact"}"#)
        .map_err(Error::internal)?;
    let outcome: Result<()> = async {
        let file_app = app(&database, files.as_path(), "files").await?;
        let install: Result<()> = file_app.run_command("migrate", Vec::new()).await;
        file_app
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        install?;
        let selected = app(&database, files.as_path(), "composite").await?;
        let result: Result<()> = async {
            let service = selected
                .state()
                .container()
                .resolve::<TranslationSourceExample>()
                .await?;
            assert_eq!(service.show(None)?, "file fallback");
            selected
                .run_command(
                    "example:translation",
                    vec!["set".into(), "en".into(), "database".into()],
                )
                .await?;
            assert_eq!(service.show(None)?, "database");
            assert_eq!(service.show(Some("fr"))?, "fr exact");
            selected
                .run_command(
                    "example:translation",
                    vec!["set".into(), "en".into(), String::new()],
                )
                .await?;
            assert_eq!(service.show(None)?, "");
            selected
                .run_command("example:translation", vec!["disable".into(), "en".into()])
                .await?;
            assert_eq!(service.show(None)?, "file fallback");
            selected
                .run_command("example:translation", vec!["delete".into(), "en".into()])
                .await?;
            assert_eq!(service.show(None)?, "file fallback");
            Ok(())
        }
        .await;
        selected
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await?;
        result
    }
    .await;
    database.close().await?;
    outcome
}

#[cfg(feature = "queue")]
mod queued_source_notifications {
    use super::*;
    use openrust::config::QueueConfig;
    use openrust::database::{
        DatabaseTransaction, Db,
        sea_orm::{ConnectionTrait, Statement},
    };
    use openrust::events::{Dispatcher, EventsServiceProvider};
    use openrust::foundation::RegisterContext;
    use openrust::queue::{DispatchOptions, Job, JobContext, Queue, QueueDriver};
    use openrust::support::{BoxFuture, ServiceProvider};
    use openrust::translation::{Locale, TranslationResource};
    use openrust_translation_database::{
        DatabaseTranslationLoader, TranslationMutation, TranslationSourceChanged,
        TranslationSourceMigration,
    };
    use serde_json::{Value, json};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct Migrations;
    impl MigratorTrait for Migrations {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(TranslationSourceMigration)]
        }
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    struct NoticeJob {
        revision: u64,
    }
    impl Job for NoticeJob {
        const NAME: &'static str = "example.translation-source.notice.v1";
        fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
            Box::pin(async {
                Err(Error::custom(
                    "source_notice_unexpected_worker",
                    "此回归仅捕获派发，不启动worker",
                ))
            })
        }
    }
    struct QueueBinding(Queue);
    impl ServiceProvider for QueueBinding {
        fn register<'a>(
            &'a self,
            context: &'a mut RegisterContext<'_>,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { context.instance(Arc::new(self.0.clone())).await })
        }
    }
    type CapturedNotice = (String, String, Value, u64);
    #[derive(Clone)]
    struct CaptureQueueDriver {
        db: Db,
        fail: Arc<AtomicBool>,
        attempts: Arc<Mutex<Vec<CapturedNotice>>>,
        transaction_calls: Arc<AtomicUsize>,
    }
    impl CaptureQueueDriver {
        fn captured(&self) -> Result<Vec<CapturedNotice>> {
            self.attempts
                .lock()
                .map(|attempts| attempts.clone())
                .map_err(|_| Error::custom("source_notice_capture_lock", "通知捕获锁失败"))
        }
    }
    impl QueueDriver for CaptureQueueDriver {
        fn dispatch<'a>(
            &'a self,
            queue: &'a str,
            job_name: &'a str,
            payload: Value,
            _options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                // 直接从主库连接池读取，避开任何调用方transaction句柄；必须已提交才可见。
                let connection = self.db.primary_connection()?;
                let statement = Statement::from_string(
                    self.db.backend()?,
                    format!(
                        "SELECT revision FROM {} WHERE id=1",
                        self.db.quoted_table_name("translation_revisions")?
                    ),
                );
                let row = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    connection.query_one_raw(statement),
                )
                .await
                .map_err(Error::internal)?
                .map_err(Error::internal)?
                .ok_or_else(|| {
                    Error::custom("source_notice_missing_revision", "提交后修订行缺失")
                })?;
                let revision = u64::try_from(
                    row.try_get::<i64>("", "revision")
                        .map_err(Error::internal)?,
                )
                .map_err(Error::internal)?;
                assert_eq!(payload["revision"].as_u64(), Some(revision));
                self.attempts
                    .lock()
                    .map_err(|_| Error::custom("source_notice_capture_lock", "通知捕获锁失败"))?
                    .push((queue.into(), job_name.into(), payload, revision));
                if self.fail.load(Ordering::SeqCst) {
                    return Err(Error::custom(
                        "source_notice_queue_unavailable",
                        "测试队列拒绝派发",
                    ));
                }
                Ok(format!("source-notice-{revision}"))
            })
        }
        fn dispatch_in_transaction<'a>(
            &'a self,
            _transaction: &'a DatabaseTransaction,
            _queue: &'a str,
            _job_name: &'a str,
            _payload: Value,
            _options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            self.transaction_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Err(Error::custom(
                    "source_notice_transaction_unexpected",
                    "来源通知在提交之后，不得再使用原事务",
                ))
            })
        }
        fn run_next<'a>(&'a self, _queue: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }
        fn retry_failed<'a>(&'a self, _failed_id: &'a str) -> BoxFuture<'a, Result<String>> {
            Box::pin(async {
                Err(Error::custom(
                    "source_notice_worker_retry_unexpected",
                    "测试不使用worker重试",
                ))
            })
        }
        fn reset_shutdown(&self) {}
        fn request_shutdown(&self) {}
        fn is_shutdown_requested(&self) -> bool {
            false
        }
    }
    fn put(value: &str) -> Result<TranslationMutation> {
        Ok(TranslationMutation::Put {
            resource: TranslationResource {
                locale: Locale::new("en")?,
                group: "*".into(),
                namespace: None,
            },
            key: "notice".into(),
            value: json!(value),
            enabled: true,
        })
    }
    fn expected_notice(revision: u64) -> CapturedNotice {
        (
            "source-notices".into(),
            NoticeJob::NAME.into(),
            json!({"revision":revision}),
            revision,
        )
    }
    #[tokio::test]
    async fn committed_source_revision_uses_public_queued_events_and_notification_retry()
    -> Result<()> {
        let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!(
                "未执行Source提交后队列通知实库回归：需OPENRUST_TEST_DATABASE_URL；无数据库不计实库通过"
            );
            return Ok(());
        };
        let database = TestDatabase::create::<Migrations>(&url).await?;
        let outcome: Result<()> = async {
            let db = Db::default();
            db.initialize(database.connection()?)?;
            let driver = CaptureQueueDriver {
                db: db.clone(),
                fail: Arc::new(AtomicBool::new(false)),
                attempts: Arc::new(Mutex::new(Vec::new())),
                transaction_calls: Arc::new(AtomicUsize::new(0)),
            };
            let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
            let mut events = EventsServiceProvider::new();
            events.listen_queued::<TranslationSourceChanged, NoticeJob, _>(
                "source-notices",
                |event| {
                    Ok(NoticeJob {
                        revision: event.revision,
                    })
                },
            );
            let mut builder = ProviderBuilder::new();
            builder
                .provider(openrust::events::EventServiceProvider)
                .provider(QueueBinding(queue))
                .provider(events);
            let runtime = builder.start(figment::Figment::new()).await?;
            let result: Result<()> = async {
                let events = runtime.container().resolve::<Dispatcher>().await?;
                let loader =
                    DatabaseTranslationLoader::new(db.clone()).with_events((*events).clone());
                let first = loader.write(vec![put("first")?]).await?;
                assert_eq!(first.revision, 1);
                assert!(first.notifications_delivered);
                assert_eq!(driver.captured()?, vec![expected_notice(1)]);
                assert_eq!(driver.transaction_calls.load(Ordering::SeqCst), 0);
                driver.fail.store(true, Ordering::SeqCst);
                let second = loader.write(vec![put("second")?]).await?;
                assert_eq!(second.revision, 2);
                assert!(!second.notifications_delivered);
                let attempts = driver.captured()?;
                assert_eq!(attempts.len(), 2);
                assert_eq!(attempts[1], expected_notice(2));
                assert_eq!(
                    db.table("translation_revisions")?
                        .where_eq("id", 1_i32)?
                        .value::<i64>("revision")
                        .await?,
                    Some(2)
                );
                driver.fail.store(false, Ordering::SeqCst);
                let retry = loader.retry_notifications(second.revision).await?;
                assert_eq!(retry.revision, 2);
                assert!(retry.notifications_delivered);
                let attempts = driver.captured()?;
                assert_eq!(attempts.len(), 3);
                assert_eq!(attempts[2], expected_notice(2));
                assert_eq!(
                    db.table("translation_revisions")?
                        .where_eq("id", 1_i32)?
                        .value::<i64>("revision")
                        .await?,
                    Some(2)
                );
                assert_eq!(driver.transaction_calls.load(Ordering::SeqCst), 0);
                Ok(())
            }
            .await;
            runtime
                .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                .await?;
            result
        }
        .await;
        database.close().await?;
        outcome
    }
}
