use openrust::{
    database::{DB, DatabaseError, Transaction, Value},
    pipeline::Pipeline,
    testing::TestApp,
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn transactional_pipeline_defers_execution_and_commits_or_rolls_back_on_selected_connection()
-> TestResult {
    let mut app = TestApp::create(|builder| {
        builder.with_config(|_,_| {
        let connection = if let Ok(url)=std::env::var("OPENRUST_TEST_PIPELINE_URL") { json!({"url":url}) } else {
            assert!(std::env::var_os("OPENRUST_REQUIRE_PIPELINE").is_none(),"必须提供隔离数据库连接");
            json!({"driver":"sqlite","database":":memory:"})
        };
        Ok([("app".into(),json!({"env":"testing","timezone":"UTC"})),("database".into(),json!({"default":"primary","connections":{"primary":connection.clone(),"audit":connection}}))].into())
    })
    })?;
    app.freeze_time_for(async |_, _| {
        let connection = DB::connection(Some("audit"))?;
        connection
            .statement("create table pipeline_rows (id integer primary key)", &[])
            .await?;
        let order = Arc::new(Mutex::new(Vec::new()));
        let begins = Arc::new(AtomicUsize::new(0));
        let counter = begins.clone();
        connection.beforeStartingTransaction(move |connection, _| {
            assert_eq!(connection.get_name(), "audit");
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        });
        let pipes = order.clone();
        let finalizer = order.clone();
        let pipeline = Pipeline::<_, DatabaseError, _, Transaction>::send(1)
            .withinTransaction(Some("audit"))
            .pipe(move |value, mut next| {
                Box::pin(async move {
                    assert_eq!(next.context().getConnection().get_name(), "audit");
                    pipes.lock().unwrap().push("pipe");
                    next.context()
                        .statement(
                            "insert into pipeline_rows values (?)",
                            &[Value::Integer(value)],
                        )
                        .await?;
                    next.run(value + 1).await
                })
            })
            .finally(move |original| {
                assert_eq!(original, 1);
                finalizer.lock().unwrap().push("finally");
            });
        assert_eq!(begins.load(Ordering::SeqCst), 0);
        assert!(order.lock().unwrap().is_empty());
        assert_eq!(
            connection
                .scalar("select count(*) from pipeline_rows", &[])
                .await?,
            Value::Integer(0)
        );
        let destination = order.clone();
        let result = pipeline
            .then(move |value, transaction| {
                Box::pin(async move {
                    destination.lock().unwrap().push("destination");
                    // 嵌套管道明确复用当前事务，避免另取池连接。
                    Pipeline::<_, DatabaseError, _, Transaction>::send(value)
                        .thenWith(transaction, |value, transaction| {
                            Box::pin(async move {
                                transaction
                                    .statement(
                                        "insert into pipeline_rows values (?)",
                                        &[Value::Integer(value)],
                                    )
                                    .await?;
                                Ok(value + 1)
                            })
                        })
                        .await
                })
            })
            .await?;
        assert_eq!(result, 3);
        assert_eq!(begins.load(Ordering::SeqCst), 1);
        assert_eq!(*order.lock().unwrap(), ["pipe", "destination", "finally"]);
        assert_eq!(
            connection
                .scalar("select count(*) from pipeline_rows", &[])
                .await?,
            Value::Integer(2)
        );
        let finalized = Arc::new(AtomicUsize::new(0));
        let copy = finalized.clone();
        let failed = Pipeline::<_, DatabaseError, _, Transaction>::send(3)
            .withinTransaction(Some("audit"))
            .pipe(|value, mut next| {
                Box::pin(async move {
                    next.context()
                        .statement(
                            "insert into pipeline_rows values (?)",
                            &[Value::Integer(value)],
                        )
                        .await?;
                    next.run(value).await
                })
            })
            .finally(move |_| {
                copy.fetch_add(1, Ordering::SeqCst);
            })
            .then(|_, transaction| {
                Box::pin(async move {
                    transaction
                        .statement("insert into pipeline_rows values (1)", &[])
                        .await?;
                    Ok(())
                })
            })
            .await;
        assert!(failed.is_err());
        assert_eq!(finalized.load(Ordering::SeqCst), 1);
        assert_eq!(
            connection
                .scalar("select count(*) from pipeline_rows", &[])
                .await?,
            Value::Integer(2)
        );
        assert_eq!(
            Pipeline::send(8)
                .withinTransaction(Some("audit"))
                .thenReturn()
                .await?,
            8
        );
        assert_eq!(
            Pipeline::<_, DatabaseError, _, Transaction>::send(9)
                .withinTransaction(None)
                .then(|value, transaction| Box::pin(async move {
                    assert_eq!(transaction.getConnection().get_name(), "primary");
                    Ok(value)
                }))
                .await?,
            9
        );
        let missing_finalized = Arc::new(AtomicUsize::new(0));
        let copy = missing_finalized.clone();
        let error = Pipeline::send(0)
            .finally(move |_| {
                copy.fetch_add(1, Ordering::SeqCst);
            })
            .withinTransaction(Some("missing"))
            .thenReturn()
            .await;
        assert!(matches!(error, Err(DatabaseError::UnknownConnection(_))));
        assert_eq!(missing_finalized.load(Ordering::SeqCst), 1);
        let copy = finalized.clone();
        let cancelled = Pipeline::<_, DatabaseError, _, Transaction>::send(4)
            .withinTransaction(Some("audit"))
            .finally(move |_| {
                copy.fetch_add(1, Ordering::SeqCst);
            })
            .then(|value, transaction| {
                Box::pin(async move {
                    transaction
                        .statement(
                            "insert into pipeline_rows values (?)",
                            &[Value::Integer(value)],
                        )
                        .await?;
                    std::future::pending::<Result<(), DatabaseError>>().await
                })
            });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), cancelled)
                .await
                .is_err()
        );
        assert_eq!(finalized.load(Ordering::SeqCst), 2);
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                connection.scalar("select count(*) from pipeline_rows", &[])
            )
            .await??,
            Value::Integer(2)
        );
        connection
            .statement("drop table pipeline_rows", &[])
            .await?;
        Ok(())
    })
    .await
}
