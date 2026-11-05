//! 固定物理管理连接；绝不进入 pool，也不在断线或取消后重连。
//!
//! 每次操作先从槽位取走连接，仅完整成功或白名单权限拒绝才归还。future 被取消或不明 SQL 出错时，
//! 局部连接随 RAII 析构，槽位保持 None，未知结果不能被后续命令继续使用。

use sea_orm::{ConnectionTrait, DbBackend, DbErr, ExecResult, QueryResult, Statement};
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
use sea_orm::{sea_query::Value, sqlx};
use tokio::sync::Mutex;

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
enum RawConnection {
    #[cfg(feature = "db-mysql")]
    Mysql(sqlx::MySqlConnection),
    #[cfg(feature = "db-postgres")]
    Postgres(sqlx::PgConnection),
}
#[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
type RawConnection = ();

/// 串行使用一条原生管理连接；失效后只能由调用方另建新的租约。
pub(super) struct AdminSession {
    backend: DbBackend,
    connection: Mutex<Option<RawConnection>>,
}

impl std::fmt::Debug for AdminSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminSession")
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

fn unavailable() -> DbErr {
    DbErr::Custom("test_database_session_unavailable".to_owned())
}

impl AdminSession {
    /// 直接建立原生物理连接，不记录 URL，不创建 pool 或自动重连策略。
    #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
    pub(super) async fn connect(admin_url: &str) -> Result<Self, DbErr> {
        #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
        {
            use sqlx::Connection;
            #[cfg(feature = "db-mysql")]
            if admin_url.starts_with("mysql://") {
                let options = admin_url
                    .parse::<sqlx::mysql::MySqlConnectOptions>()
                    .map_err(|_| unavailable())?
                    .ssl_mode(sqlx::mysql::MySqlSslMode::Required);
                let raw = sqlx::MySqlConnection::connect_with(&options)
                    .await
                    .map_err(|_| unavailable())?;
                return Ok(Self {
                    backend: DbBackend::MySql,
                    connection: Mutex::new(Some(RawConnection::Mysql(raw))),
                });
            }
            #[cfg(feature = "db-postgres")]
            if admin_url.starts_with("postgres://") || admin_url.starts_with("postgresql://") {
                let raw = sqlx::PgConnection::connect(admin_url)
                    .await
                    .map_err(|_| unavailable())?;
                return Ok(Self {
                    backend: DbBackend::Postgres,
                    connection: Mutex::new(Some(RawConnection::Postgres(raw))),
                });
            }
        }
        let _ = admin_url;
        Err(unavailable())
    }

    #[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
    pub(super) fn connect(admin_url: &str) -> std::future::Ready<Result<Self, DbErr>> {
        let _ = admin_url;
        std::future::ready(Err(unavailable()))
    }

    /// 从槽位永久移除并关闭连接；关闭取消或失败也不会恢复此会话。
    pub(super) async fn close(&self) -> Result<(), DbErr> {
        let raw = self.connection.lock().await.take();
        #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
        if let Some(raw) = raw {
            use sqlx::Connection;
            return match raw {
                #[cfg(feature = "db-mysql")]
                RawConnection::Mysql(connection) => {
                    connection.close().await.map_err(|_| unavailable())
                }
                #[cfg(feature = "db-postgres")]
                RawConnection::Postgres(connection) => {
                    connection.close().await.map_err(|_| unavailable())
                }
            };
        }
        #[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
        let _ = raw;
        Ok(())
    }
}

// SQL仅来自本模块固定语句或已校验数据库标识符；动态数据保持绑定。
// 只桥接本清理协议需要的字符串和 i64（含其 NULL），绝不通过 SQL 文本替换参数。
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
macro_rules! bound_query {
    ($statement:ident) => {{
        let mut query = sqlx::query(sqlx::AssertSqlSafe($statement.sql.as_str()));
        if let Some(values) = &$statement.values {
            for value in &values.0 {
                query = match value {
                    Value::String(value) => query.bind(value.as_deref()),
                    Value::BigInt(value) => query.bind(*value),
                    _ => {
                        return Err(DbErr::Custom(
                            "test_database_session_unsupported_value".to_owned(),
                        ));
                    }
                };
            }
        }
        query
    }};
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
macro_rules! perform {
    ($session:ident, $statement:ident, $operation:ident, $convert:expr) => {{
        let mut slot = $session.connection.lock().await;
        let mut raw = slot.take().ok_or_else(unavailable)?;
        if $statement.db_backend != $session.backend {
            return Err(DbErr::Custom(
                "test_database_session_backend_mismatch".to_owned(),
            ));
        }
        let output = match &mut raw {
            #[cfg(feature = "db-mysql")]
            RawConnection::Mysql(connection) => bound_query!($statement)
                .$operation(connection)
                .await
                .map($convert),
            #[cfg(feature = "db-postgres")]
            RawConnection::Postgres(connection) => bound_query!($statement)
                .$operation(connection)
                .await
                .map($convert),
        };
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                if definite_permission_error(&error) {
                    *slot = Some(raw);
                }
                return Err(operation_error(&error));
            }
        };
        *slot = Some(raw);
        Ok(output)
    }};
}

#[async_trait::async_trait]
impl ConnectionTrait for AdminSession {
    fn get_database_backend(&self) -> DbBackend {
        self.backend
    }

    async fn execute_raw(&self, statement: Statement) -> Result<ExecResult, DbErr> {
        #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
        {
            perform!(self, statement, execute, ExecResult::from)
        }
        #[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
        {
            let _ = statement;
            Err(unavailable())
        }
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
        {
            let mut slot = self.connection.lock().await;
            let mut raw = slot.take().ok_or_else(unavailable)?;
            let output = match &mut raw {
                #[cfg(feature = "db-mysql")]
                RawConnection::Mysql(connection) => sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .execute(connection)
                    .await
                    .map(ExecResult::from),
                #[cfg(feature = "db-postgres")]
                RawConnection::Postgres(connection) => sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .execute(connection)
                    .await
                    .map(ExecResult::from),
            };
            let output = match output {
                Ok(output) => output,
                Err(error) => {
                    if definite_permission_error(&error) {
                        *slot = Some(raw);
                    }
                    return Err(operation_error(&error));
                }
            };
            *slot = Some(raw);
            Ok(output)
        }
        #[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
        {
            let _ = sql;
            Err(unavailable())
        }
    }

    async fn query_one_raw(&self, statement: Statement) -> Result<Option<QueryResult>, DbErr> {
        #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
        {
            perform!(self, statement, fetch_optional, |row| row
                .map(QueryResult::from))
        }
        #[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
        {
            let _ = statement;
            Err(unavailable())
        }
    }

    async fn query_all_raw(&self, statement: Statement) -> Result<Vec<QueryResult>, DbErr> {
        #[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
        {
            perform!(self, statement, fetch_all, |rows| rows
                .into_iter()
                .map(QueryResult::from)
                .collect())
        }
        #[cfg(not(any(feature = "db-mysql", feature = "db-postgres")))]
        {
            let _ = statement;
            Err(unavailable())
        }
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
fn operation_error(error: &sqlx::Error) -> DbErr {
    #[cfg(feature = "db-mysql")]
    if let sqlx::Error::Database(error) = error {
        #[cfg(feature = "db-mysql")]
        if error
            .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
            .is_some_and(|error| error.number() == 1049)
        {
            return DbErr::Custom("test_database_mysql_unknown_database".to_owned());
        }
    }
    let _ = error;
    unavailable()
}

// 完整权限拒绝已结束语句且不终止会话；仅此白名单保留原锁，以回收刚CREATE、未迁移的库。
// 传输/协议错误、取消、shutdown及其他SQL错误仍永久失效，绝不重连补锁。
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
fn definite_permission_error(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(error) = error else {
        return false;
    };
    #[cfg(feature = "db-mysql")]
    if error
        .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
        .is_some_and(|error| matches!(error.number(), 1044 | 1142 | 1227 | 1370))
    {
        return true;
    }
    #[cfg(feature = "db-postgres")]
    if error
        .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
        .is_some_and(|error| error.code() == "42501")
    {
        return true;
    }
    false
}
