//! 仅接modules/example测试模块；真实ArticleWriter服务与生产文章事件工厂。
use super::{http::requests::article::ArticleInput, services::article_write::ArticleWriter};
use figment::{Figment, providers::Serialized};
use futures_util::FutureExt as _;
use openrust::{
    Error, Result,
    database::{
        DatabaseServiceProvider, Db, TransactionBeforeCommit,
        sea_orm::{ConnectionTrait, DbBackend, Statement},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    events::{Events, EventsServiceProvider},
    foundation::ProviderBuilder,
    testing::TestDatabase,
};
use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use tracing::instrument::WithSubscriber as _;

struct ArticleMigrations;
impl MigratorTrait for ArticleMigrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
fn failure(code: &'static str) -> Error {
    Error::custom(code, "文章context测试拒绝")
}
fn finish(outcome: std::thread::Result<Result<()>>, closed: Result<()>) -> Result<()> {
    match outcome {
        Ok(result) => {
            if result.is_err()
                && let Err(error) = &closed
            {
                eprintln!("article context cleanup failed: {}", error.code());
            }
            result.and(closed)
        }
        Err(payload) => {
            if let Err(error) = closed {
                eprintln!("article context cleanup failed: {}", error.code());
            }
            std::panic::resume_unwind(payload)
        }
    }
}
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("capture poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Capture {
    fn text(&self) -> Result<String> {
        String::from_utf8(
            self.0
                .lock()
                .map_err(|_| failure("capture_poisoned"))?
                .clone(),
        )
        .map_err(Error::internal)
    }
}
fn events(mode: Arc<AtomicU8>) -> EventsServiceProvider {
    let mut events = super::events::article::provider();
    events.listen::<TransactionBeforeCommit,_>(move |_,context|{
        let mode=Arc::clone(&mode);
        Box::pin(async move {
            let tx=context.transaction().ok_or_else(||failure("article_listener_missing_tx"))?;
            let sql=match mode.load(Ordering::SeqCst){
                0=>return Ok(()),
                1=>"DELETE FROM example_articles WHERE title='Context delete' AND owner_id=1",
                2=>"UPDATE example_articles SET owner_id=2 WHERE title='Context owner' AND owner_id=1",
                _=>return Err(failure("article_mode_invalid")),
            };
            let changed=tx.execute_raw(Statement::from_string(DbBackend::MySql,sql.to_owned())).await.map_err(Error::internal)?;
            assert_eq!(changed.rows_affected(),1);
            // callback中的文章事件已经真实落同事务；监听器不是假失败注入。
            assert_eq!(counts(tx).await?.1,2);
            #[cfg(feature="queue")]
            assert_eq!(job_count(tx).await?,4);
            #[cfg(feature = "queue")]
            assert_job_kinds(tx, 2).await?;
            Ok(())
        })
    });
    events
}
async fn counts(connection: &impl ConnectionTrait) -> Result<(i64, i64)> {
    let mut values = Vec::new();
    for table in ["example_articles", "example_article_events"] {
        values.push(
            connection
                .query_one_raw(Statement::from_string(
                    DbBackend::MySql,
                    format!("SELECT COUNT(*) AS n FROM {table}"),
                ))
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| failure("article_count_missing"))?
                .try_get::<i64>("", "n")
                .map_err(Error::internal)?,
        );
    }
    Ok((values[0], values[1]))
}
#[cfg(feature = "queue")]
async fn job_count(connection: &impl ConnectionTrait) -> Result<i64> {
    connection
        .query_one_raw(Statement::from_string(
            DbBackend::MySql,
            "SELECT COUNT(*) AS n FROM jobs".to_owned(),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| failure("jobs_count_missing"))?
        .try_get("", "n")
        .map_err(Error::internal)
}
async fn outcomes(
    writer: &ArticleWriter,
    reader: &impl ConnectionTrait,
    mode: &AtomicU8,
) -> Result<()> {
    let created = writer
        .create(
            1,
            ArticleInput {
                title: "Context success".into(),
                body: "Body".into(),
            },
        )
        .await?;
    assert!(created.id > 0);
    assert_eq!(created.owner_id, 1);
    assert_eq!(counts(reader).await?, (1, 1));
    #[cfg(feature = "queue")]
    assert_eq!(job_count(reader).await?, 2);
    #[cfg(feature = "queue")]
    assert_job_kinds(reader, 1).await?;
    for (current, title) in [(1, "Context delete"), (2, "Context owner")] {
        mode.store(current, Ordering::SeqCst);
        let capture = Capture::default();
        let captured = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || captured.clone())
            .finish();
        let rejected = writer
            .create(
                1,
                ArticleInput {
                    title: title.into(),
                    body: "Body".into(),
                },
            )
            .with_subscriber(subscriber)
            .await;
        assert!(
            matches!(rejected,Err(error) if error.code()=="example_article_changed_before_commit" && error.status().as_u16()==409)
        );
        let trace = capture.text()?;
        assert!(trace.contains("outcome=\"conflict\""));
        assert!(!trace.contains("outcome=\"success\""));
        assert_eq!(counts(reader).await?, (1, 1));
        #[cfg(feature = "queue")]
        assert_eq!(job_count(reader).await?, 2);
        #[cfg(feature = "queue")]
        assert_job_kinds(reader, 1).await?;
    }
    mode.store(0, Ordering::SeqCst);
    Ok(())
}
async fn exercise(database: &TestDatabase) -> Result<()> {
    let mode = Arc::new(AtomicU8::new(0));
    let mut builder = ProviderBuilder::new();
    builder
        .provider(events(Arc::clone(&mode)))
        .provider(DatabaseServiceProvider);
    #[cfg(feature = "queue")]
    {
        let queue = openrust::queue::QueueServiceProvider::new();
        queue
            .contributions()
            .register_job::<super::jobs::article_index::ArticleIndexJob>()?;
        builder
            .provider(queue)
            .provider(openrust::notifications::NotificationsServiceProvider);
    }
    let runtime=builder.start(Figment::from(Serialized::defaults(serde_json::json!({
        "database":{"url":database.url(),"min_connections":1,"max_connections":5},"queue":{"default":"database"}
    })))).await?;
    let result=std::panic::AssertUnwindSafe(async {
        let db=runtime.container().resolve::<Db>().await?;
        let reader=database.connection()?;
        reader.execute_unprepared("INSERT INTO users(id,email,password_hash,email_verified_at,created_at,updated_at) VALUES(1,'context1@example.test','unused',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP),(2,'context2@example.test','unused',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.map_err(Error::internal)?;
        let events = runtime.container().resolve::<Events>().await?;
        #[cfg(feature = "queue")]
        let writer = ArticleWriter::new(Arc::clone(&db), events, runtime.container().resolve::<openrust::notifications::Notifier>().await?);
        #[cfg(not(feature = "queue"))]
        let writer = ArticleWriter::new(Arc::clone(&db), events);
        outcomes(&writer,&reader,&mode).await
    }).catch_unwind().await;
    finish(
        result,
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
            .await,
    )
}
#[tokio::test]
async fn mysql_article_create_context_rejects_listener_delete_or_owner_change() -> Result<()> {
    let admin = std::env::var("OPENRUST_TEST_DATABASE_URL").map_err(Error::internal)?;
    if !admin.starts_with("mysql://") {
        return Err(failure("mysql_required"));
    }
    let database = TestDatabase::create::<ArticleMigrations>(&admin).await?;
    let result = std::panic::AssertUnwindSafe(exercise(&database))
        .catch_unwind()
        .await;
    finish(result, database.close().await)
}

#[cfg(feature = "queue")]
async fn assert_job_kinds(connection: &impl ConnectionTrait, each: i64) -> Result<()> {
    use openrust::queue::Job as _;
    use std::collections::BTreeMap;
    let rows = connection
        .query_all_raw(Statement::from_string(
            DbBackend::MySql,
            "SELECT job_name FROM jobs".to_owned(),
        ))
        .await
        .map_err(Error::internal)?;
    // 数据库collation可能忽略大小写；在Rust中按实际名称精确累计。
    let mut actual = BTreeMap::<String, i64>::new();
    for row in rows {
        let name: String = row.try_get("", "job_name").map_err(Error::internal)?;
        *actual.entry(name).or_default() += 1;
    }
    assert_eq!(
        actual,
        BTreeMap::from([
            (
                super::jobs::article_index::ArticleIndexJob::NAME.to_owned(),
                each
            ),
            ("openrust.notification.email.v1".to_owned(), each),
        ])
    );
    Ok(())
}
