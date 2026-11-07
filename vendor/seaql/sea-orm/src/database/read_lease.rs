//! 仅只读查询的成熟物理lease；由实际driver能力签发，不创建事务。
#[cfg(feature = "sqlx-sqlite")]
use crate::DatabaseConnectionType;
use crate::{ConnectionTrait, DatabaseConnection, DbErr, QueryResult, Statement};

/// 同一物理连接上的串行只读查询载体；无公开构造、Clone或事务/写接口。
///
/// 此类型在没有SQLxSQLite的构建中仍可命名，此时普通查询工厂不会产出lease实例。
/// 只暴露query接口，不解析SQL或授予只读权限；调用者须传读取语句。
pub struct DatabaseReadLease {
    #[cfg(feature = "sqlx-sqlite")]
    owner: crate::SqlxSqlitePoolConnection,
    #[cfg(feature = "sqlx-sqlite")]
    connection: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    _private: (),
}

impl std::fmt::Debug for DatabaseReadLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatabaseReadLease")
            .finish_non_exhaustive()
    }
}

impl DatabaseConnection {
    /// 执行首条真实读取；支持的实际driver同时签发同一物理连接lease。
    ///
    /// SQLxSQLite在标准query_all span内先debug/build、acquire、fetch/metric一次；
    /// 其它driver沿原query_all_raw读取并返回真实rows/None，不得再次重放首条。
    pub async fn query_all_with_read_lease(
        &self,
        stmt: Statement,
    ) -> Result<(Vec<QueryResult>, Option<DatabaseReadLease>), DbErr> {
        #[cfg(feature = "sqlx-sqlite")]
        if let DatabaseConnectionType::SqlxSqlitePoolConnection(owner) = &self.inner {
            return crate::database::tracing_spans::with_db_span!(
                "sea_orm.query_all",
                crate::DbBackend::Sqlite,
                stmt.sql.as_str(),
                record_stmt = owner.record_stmt_in_spans,
                async {
                    let (rows, connection) = owner.query_all_with_connection(stmt).await?;
                    Ok((
                        rows,
                        Some(DatabaseReadLease {
                            owner: owner.clone(),
                            connection,
                            _private: (),
                        }),
                    ))
                }
            );
        }
        self.query_all_raw(stmt).await.map(|rows| (rows, None))
    }
}

impl DatabaseReadLease {
    /// 以同一物理连接读取全部行，复用driver原query/bind/metric/span/error内核。
    pub async fn query_all_raw(&mut self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        #[cfg(feature = "sqlx-sqlite")]
        {
            self.owner.query_all_on(&mut self.connection, stmt).await
        }
        #[cfg(not(feature = "sqlx-sqlite"))]
        {
            let _ = (&self._private, stmt);
            Err(crate::error::conn_err(
                "physical read lease is unavailable in this build",
            ))
        }
    }
    /// 同一物理连接读取一行；无行返回None，不重新acquire。
    pub async fn query_one_raw(&mut self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        #[cfg(feature = "sqlx-sqlite")]
        {
            self.owner.query_one_on(&mut self.connection, stmt).await
        }
        #[cfg(not(feature = "sqlx-sqlite"))]
        {
            let _ = (&self._private, stmt);
            Err(crate::error::conn_err(
                "physical read lease is unavailable in this build",
            ))
        }
    }
}

#[cfg(all(
    test,
    feature = "sqlx-sqlite",
    feature = "tracing-spans",
    not(feature = "sync")
))]
mod tests {
    use crate::{
        ConnectionTrait, DatabaseConnection, DatabaseConnectionType, DbErr, Statement, Value,
    };
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    use tracing::{
        Id, Subscriber,
        field::{Field, Visit},
        instrument::WithSubscriber as _,
        span::{Attributes, Record},
    };
    use tracing_subscriber::{
        Layer,
        layer::{Context, SubscriberExt as _},
    };

    type SpanFields = BTreeMap<String, String>;
    type SpanRecords = Vec<(u64, String, SpanFields)>;
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<SpanRecords>>);
    struct Visitor<'a>(&'a mut BTreeMap<String, String>);
    impl Visit for Visitor<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name().into(), format!("{value:?}"));
        }
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name().into(), value.into());
        }
    }
    impl<S: Subscriber> Layer<S> for Capture {
        fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, _: Context<'_, S>) {
            if !matches!(
                attributes.metadata().name(),
                "sea_orm.query_one" | "sea_orm.query_all"
            ) {
                return;
            }
            let mut fields = BTreeMap::new();
            attributes.record(&mut Visitor(&mut fields));
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((id.into_u64(), attributes.metadata().name().into(), fields));
        }
        fn on_record(&self, id: &Id, values: &Record<'_>, _: Context<'_, S>) {
            if let Some((_, _, fields)) = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter_mut()
                .rev()
                .find(|(candidate, _, _)| *candidate == id.into_u64())
            {
                values.record(&mut Visitor(fields));
            }
        }
    }
    fn statement(alias: &str) -> Statement {
        Statement::from_sql_and_values(
            crate::DbBackend::Sqlite,
            format!("select ? as {alias}"),
            [7_i32.into()],
        )
    }
    async fn queries(connection: &DatabaseConnection) -> Result<(), DbErr> {
        let one = connection
            .query_one_raw(statement("value"))
            .await?
            .ok_or_else(|| crate::error::conn_err("missing pool row"))?;
        assert_eq!(one.try_get::<i32>("", "value")?, 7);
        assert_eq!(connection.query_all_raw(statement("value")).await?.len(), 1);
        let (first, lease) = connection
            .query_all_with_read_lease(statement("first"))
            .await?;
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].try_get::<i32>("", "first")?, 7);
        let mut lease = lease.ok_or_else(|| crate::error::conn_err("missing read capability"))?;
        let one = lease
            .query_one_raw(statement("second"))
            .await?
            .ok_or_else(|| crate::error::conn_err("missing leased row"))?;
        assert_eq!(one.try_get::<i32>("", "second")?, 7);
        assert_eq!(lease.query_all_raw(statement("value")).await?.len(), 1);
        assert!(matches!(
            lease
                .query_one_raw(Statement::from_string(
                    crate::DbBackend::Sqlite,
                    "select missing_schema_column".to_owned()
                ))
                .await,
            Err(DbErr::Query(_))
        ));
        Ok::<(), DbErr>(())
    }
    #[tokio::test]
    #[ignore = "发布前SQLite trace/metric专跑：需显式启用tracing-spans"]
    async fn pool_and_physical_queries_preserve_one_span_metric_binding_and_errors()
    -> Result<(), DbErr> {
        for record_statement in [true, false] {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .map_err(crate::driver::sqlx_conn_acquire_err)?;
            let mut connection: DatabaseConnection = pool.clone().into();
            if let DatabaseConnectionType::SqlxSqlitePoolConnection(owner) = &mut connection.inner {
                owner.record_stmt_in_spans = record_statement;
            }
            let metrics = Arc::new(Mutex::new(Vec::new()));
            let metric_log = Arc::clone(&metrics);
            connection.set_metric_callback(move |info| {
                metric_log
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((info.statement.clone(), info.failed));
            });
            let capture = Capture::default();
            let subscriber = tracing_subscriber::registry().with(capture.clone());
            queries(&connection).with_subscriber(subscriber).await?;
            let metrics = metrics
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            assert_eq!(metrics.len(), 6);
            assert!(metrics[..5].iter().all(|(stmt, failed)| {
                !failed
                    && stmt
                        .values
                        .as_ref()
                        .is_some_and(|values| values.0 == [Value::Int(Some(7))])
            }));
            assert!(metrics[5].1);
            assert_eq!(metrics[2].0.sql, "select ? as first");
            assert_eq!(metrics[3].0.sql, "select ? as second");
            let spans = capture
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            assert_eq!(spans.len(), 6);
            assert_eq!(
                spans
                    .iter()
                    .map(|(_, name, _)| name.as_str())
                    .collect::<Vec<_>>(),
                [
                    "sea_orm.query_one",
                    "sea_orm.query_all",
                    "sea_orm.query_all",
                    "sea_orm.query_one",
                    "sea_orm.query_all",
                    "sea_orm.query_one"
                ]
            );
            if record_statement {
                assert_eq!(
                    spans[2].2.get("db.statement").map(String::as_str),
                    Some("select ? as first")
                );
                assert_eq!(
                    spans[3].2.get("db.statement").map(String::as_str),
                    Some("select ? as second")
                );
            }
            assert_eq!(
                spans
                    .iter()
                    .filter(|(_, name, _)| name == "sea_orm.query_one")
                    .count(),
                3
            );
            assert_eq!(
                spans
                    .iter()
                    .filter(|(_, _, fields)| fields
                        .get("otel.status_code")
                        .is_some_and(|status| status == "ERROR"))
                    .count(),
                1
            );
            assert!(
                spans
                    .iter()
                    .all(|(_, _, fields)| fields.contains_key("db.statement") == record_statement)
            );
            pool.close().await;
        }
        Ok(())
    }
    type Metrics = Arc<Mutex<Vec<(Statement, bool)>>>;
    fn configured(
        pool: sqlx::SqlitePool,
        record_statement: bool,
        metrics: &Metrics,
    ) -> DatabaseConnection {
        let mut connection: DatabaseConnection = pool.into();
        if let DatabaseConnectionType::SqlxSqlitePoolConnection(owner) = &mut connection.inner {
            owner.record_stmt_in_spans = record_statement;
        }
        let observed = Arc::clone(metrics);
        connection.set_metric_callback(move |info| {
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((info.statement.clone(), info.failed));
        });
        connection
    }
    fn one_error_span(capture: &Capture, record_statement: bool, sql: &str) {
        let spans = capture
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].1, "sea_orm.query_all");
        assert_eq!(
            spans[0].2.get("otel.status_code").map(String::as_str),
            Some("ERROR")
        );
        assert_eq!(
            spans[0].2.get("db.statement").map(String::as_str),
            record_statement.then_some(sql)
        );
    }
    #[tokio::test]
    #[ignore = "发布前SQLite acquire失败trace专跑：需显式启用tracing-spans"]
    async fn factory_closed_pool_has_one_error_span_and_no_fetch_metric() -> Result<(), DbErr> {
        for record_statement in [true, false] {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .map_err(crate::driver::sqlx_conn_acquire_err)?;
            let metrics = Metrics::default();
            let connection = configured(pool.clone(), record_statement, &metrics);
            pool.close().await;
            let capture = Capture::default();
            let subscriber = tracing_subscriber::registry().with(capture.clone());
            let outcome = connection
                .query_all_with_read_lease(statement("value"))
                .with_subscriber(subscriber)
                .await;
            assert!(matches!(
                outcome,
                Err(DbErr::ConnectionAcquire(
                    crate::ConnAcquireErr::ConnectionClosed
                ))
            ));
            assert!(
                metrics
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_empty()
            );
            one_error_span(&capture, record_statement, "select ? as value");
        }
        Ok(())
    }
    #[tokio::test]
    #[ignore = "发布前SQLite首fetch失败trace专跑：需显式启用tracing-spans"]
    async fn factory_first_fetch_error_has_one_span_and_one_failed_metric() -> Result<(), DbErr> {
        for record_statement in [true, false] {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .map_err(crate::driver::sqlx_conn_acquire_err)?;
            let metrics = Metrics::default();
            let connection = configured(pool.clone(), record_statement, &metrics);
            let capture = Capture::default();
            let subscriber = tracing_subscriber::registry().with(capture.clone());
            let sql = "select missing_schema_column";
            let outcome = connection
                .query_all_with_read_lease(Statement::from_string(
                    crate::DbBackend::Sqlite,
                    sql.to_owned(),
                ))
                .with_subscriber(subscriber)
                .await;
            assert!(matches!(outcome, Err(DbErr::Query(_))));
            let metrics = metrics
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            assert_eq!(metrics.len(), 1);
            assert!(metrics[0].1);
            one_error_span(&capture, record_statement, sql);
            pool.close().await;
        }
        Ok(())
    }
}
