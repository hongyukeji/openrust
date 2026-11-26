use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{Stream, TryStreamExt as _};
use tiberius::{Client, ColumnData, Config, FromSql as _, Query, error::Error};
use tokio::{
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt as _};

use super::{DatabaseError, DatabaseResult, Row, Value, connection::TransactionCommand};

type TdsClient = Client<Compat<TcpStream>>;

#[derive(Clone)]
pub(super) struct Pool(Arc<PoolState>);

struct PoolState {
    config: Config,
    permits: Arc<Semaphore>,
    idle: Mutex<Vec<TdsClient>>,
    timeout: Duration,
}

impl Pool {
    pub(super) fn new(config: Config, max: u32, timeout: Duration) -> Self {
        Self(Arc::new(PoolState {
            config,
            permits: Arc::new(Semaphore::new(max as usize)),
            idle: Mutex::default(),
            timeout,
        }))
    }

    pub(super) fn retire(&self) {
        self.0.permits.close();
        self.0
            .idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(super) async fn acquire(&self) -> DatabaseResult<Lease> {
        tokio::time::timeout(self.0.timeout, async {
            let permit = self
                .0
                .permits
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| DatabaseError::driver("connect")(sqlx::Error::PoolClosed))?;
            let idle = self
                .0
                .idle
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop();
            let client = match idle {
                Some(client) => client,
                None => {
                    let socket = TcpStream::connect(self.0.config.get_addr())
                        .await
                        .map_err(|source| DatabaseError::driver("connect")(source.into()))?;
                    socket
                        .set_nodelay(true)
                        .map_err(|source| DatabaseError::driver("connect")(source.into()))?;
                    Client::connect(self.0.config.clone(), socket.compat_write())
                        .await
                        .map_err(|source| error("connect", source))?
                }
            };
            Ok(Lease {
                client: Some(client),
                pool: self.clone(),
                _permit: permit,
                dirty: false,
                retire: false,
                depth: 0,
            })
        })
        .await
        .map_err(|_| DatabaseError::driver("connect")(sqlx::Error::PoolTimedOut))?
    }
}

pub(super) struct Lease {
    client: Option<TdsClient>,
    pool: Pool,
    _permit: OwnedSemaphorePermit,
    pub(super) dirty: bool,
    pub(super) retire: bool,
    pub(super) depth: u32,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.dirty || self.retire || self.depth != 0 {
            return;
        }
        let mut idle = self
            .pool
            .0
            .idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.pool.0.permits.is_closed()
            && let Some(client) = self.client.take()
        {
            idle.push(client);
        }
    }
}

impl Lease {
    pub(super) async fn fetch_all<'a>(
        &mut self,
        sql: &'a str,
        bindings: impl Iterator<Item = &'a Value>,
    ) -> DatabaseResult<Vec<Row>> {
        Ok(self
            .select(sql, bindings)
            .await?
            .into_iter()
            .flatten()
            .collect())
    }

    pub(super) async fn fetch_optional<'a>(
        &mut self,
        sql: &'a str,
        bindings: impl Iterator<Item = &'a Value>,
    ) -> DatabaseResult<Option<Row>> {
        Ok(self.fetch_all(sql, bindings).await?.into_iter().next())
    }

    pub(super) async fn statement<'a>(
        &mut self,
        sql: &'a str,
        bindings: impl Iterator<Item = &'a Value>,
    ) -> DatabaseResult<super::connection::StatementResult> {
        Ok(super::connection::StatementResult {
            affected: self.execute(sql, bindings).await?,
            insert_id: Value::Null,
        })
    }

    fn start(&mut self) -> DatabaseResult<&mut TdsClient> {
        if self.dirty {
            return Err(DatabaseError::TransactionInterrupted);
        }
        self.dirty = true;
        Ok(self
            .client
            .as_mut()
            .expect("active TDS lease owns a client"))
    }

    fn finish<T>(&mut self, op: &'static str, result: Result<T, Error>) -> DatabaseResult<T> {
        self.dirty = result.as_ref().err().is_some_and(|source| {
            !matches!(source, Error::Server(_)) || (self.depth > 0 && source.is_deadlock())
        });
        result.map_err(|source| error(op, source))
    }

    pub(super) async fn raw(&mut self, sql: &str) -> DatabaseResult<bool> {
        let result = match self.start()?.simple_query(sql).await {
            Ok(stream) => stream.into_results().await.map(|_| true),
            Err(source) => Err(source),
        };
        self.finish("execute", result)
    }

    pub(super) async fn control(&mut self, command: TransactionCommand) -> DatabaseResult<()> {
        let sql = match command {
            TransactionCommand::Begin if self.depth > 0 => {
                format!("save transaction openrust_{}", self.depth + 1)
            }
            TransactionCommand::Begin => "begin transaction".into(),
            TransactionCommand::Commit if self.depth > 1 => {
                if self.dirty {
                    return Err(DatabaseError::TransactionInterrupted);
                }
                self.depth -= 1;
                return Ok(());
            }
            TransactionCommand::Commit => "commit transaction".into(),
            TransactionCommand::RollBack => return self.rollback().await,
            TransactionCommand::BeginImmediate => {
                return Err(DatabaseError::UnsupportedOperation(
                    "SQL Server immediate transaction",
                ));
            }
        };
        self.raw(&sql).await?;
        if matches!(command, TransactionCommand::Begin) {
            self.depth += 1;
        } else {
            self.depth -= 1;
        }
        Ok(())
    }

    async fn rollback(&mut self) -> DatabaseResult<()> {
        if self.depth <= 1 {
            self.raw("if xact_state() <> 0 rollback transaction")
                .await?;
            self.depth = 0;
            return Ok(());
        }

        // A doomed transaction cannot roll back to a savepoint. State zero
        // means the server already rolled back the entire transaction.
        let sql = format!(
            "declare @state smallint = xact_state(); \
             if @state = -1 rollback transaction; \
             else if @state = 1 rollback transaction openrust_{}; \
             select @state as [state]",
            self.depth
        );
        let row = self.fetch_optional(&sql, std::iter::empty()).await?;
        self.depth = match row.as_ref().and_then(|row| row.get("state")) {
            Some(Value::Integer(1)) => self.depth - 1,
            Some(Value::Integer(-1 | 0)) => 0,
            _ => {
                self.dirty = true;
                return Err(DatabaseError::TransactionInterrupted);
            }
        };
        Ok(())
    }

    pub(super) async fn select<'a>(
        &mut self,
        sql: &'a str,
        bindings: impl Iterator<Item = &'a Value>,
    ) -> DatabaseResult<Vec<Vec<Row>>> {
        let query = query(sql, bindings);
        let result = match query.query(self.start()?).await {
            Ok(stream) => stream.into_results().await,
            Err(source) => Err(source),
        };
        self.finish("select", result)?
            .into_iter()
            .map(|rows| rows.into_iter().map(decode).collect())
            .collect()
    }

    pub(super) fn cursor<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: impl Iterator<Item = &'a Value> + Send + 'a,
    ) -> impl Stream<Item = DatabaseResult<Row>> + Send + 'a {
        async_stream::try_stream! {
            let result = {
                let query = query(sql, bindings);
                let client = self.start()?;
                match query.query(client).await {
                    Ok(stream) => {
                        let mut rows = stream.into_row_stream();
                        loop {
                            match rows.try_next().await {
                                Ok(Some(row)) => yield decode(row)?,
                                Ok(None) => break Ok(()),
                                Err(error) => break Err(error),
                            }
                        }
                    }
                    Err(error) => Err(error),
                }
            };
            self.finish("cursor", result)?;
        }
    }

    pub(super) async fn execute<'a>(
        &mut self,
        sql: &'a str,
        bindings: impl Iterator<Item = &'a Value>,
    ) -> DatabaseResult<u64> {
        let result = query(sql, bindings).execute(self.start()?).await;
        self.finish("execute", result).map(|result| result.total())
    }
}

fn query<'a>(sql: &'a str, bindings: impl Iterator<Item = &'a Value>) -> Query<'a> {
    let mut query = Query::new(sql);
    for value in bindings {
        let data = match value {
            Value::Null => ColumnData::String(None),
            Value::Bool(value) => ColumnData::Bit(Some(*value)),
            Value::Integer(value) => ColumnData::I64(Some(*value)),
            Value::Unsigned(value) => match i64::try_from(*value) {
                Ok(value) => ColumnData::I64(Some(value)),
                Err(_) => ColumnData::Numeric(Some(tiberius::numeric::Numeric::new_with_scale(
                    i128::from(*value),
                    0,
                ))),
            },
            Value::Real(value) => ColumnData::F64(Some(*value)),
            Value::Text(value) => ColumnData::String(Some(Cow::Borrowed(value))),
            Value::Bytes(value) => ColumnData::Binary(Some(Cow::Borrowed(value))),
            Value::Json(value) => ColumnData::String(Some(Cow::Owned(value.to_string()))),
        };
        query.bind(data);
    }
    query
}

fn decode(row: tiberius::Row) -> DatabaseResult<Row> {
    let names = row
        .columns()
        .iter()
        .map(|column| column.name().to_owned())
        .collect::<Vec<_>>();
    let values = row
        .into_iter()
        .map(|value| {
            let text = |result: Result<Option<String>, Error>| -> DatabaseResult<Value> {
                Ok(result
                    .map_err(|source| error("decode", source))?
                    .map_or(Value::Null, Value::Text))
            };
            Ok(match value {
                ColumnData::U8(value) => value.map_or(Value::Null, |value| i64::from(value).into()),
                ColumnData::I16(value) => {
                    value.map_or(Value::Null, |value| i64::from(value).into())
                }
                ColumnData::I32(value) => {
                    value.map_or(Value::Null, |value| i64::from(value).into())
                }
                ColumnData::I64(value) => value.map_or(Value::Null, Value::Integer),
                ColumnData::F32(value) => {
                    value.map_or(Value::Null, |value| Value::Real(f64::from(value)))
                }
                ColumnData::F64(value) => value.map_or(Value::Null, Value::Real),
                ColumnData::Bit(value) => value.map_or(Value::Null, Value::Bool),
                ColumnData::String(value) => {
                    value.map_or(Value::Null, |value| value.into_owned().into())
                }
                ColumnData::Guid(value) => {
                    value.map_or(Value::Null, |value| value.to_string().into())
                }
                ColumnData::Binary(value) => {
                    value.map_or(Value::Null, |value| Value::Bytes(value.into_owned()))
                }
                ColumnData::Numeric(value) => {
                    value.map_or(Value::Null, |value| value.to_string().into())
                }
                ColumnData::Xml(value) => {
                    value.map_or(Value::Null, |value| value.as_ref().as_ref().into())
                }
                ColumnData::DateTime(_)
                | ColumnData::SmallDateTime(_)
                | ColumnData::DateTime2(_) => {
                    return text(
                        time::PrimitiveDateTime::from_sql(&value)
                            .map(|value| value.map(super::value::timestamp_text)),
                    );
                }
                ColumnData::DateTimeOffset(_) => {
                    return text(time::OffsetDateTime::from_sql(&value).map(|value| {
                        value.map(|value| {
                            let seconds = value.offset().whole_seconds();
                            let sign = if seconds < 0 { '-' } else { '+' };
                            let seconds = seconds.unsigned_abs();
                            format!(
                                "{} {}{:02}:{:02}",
                                super::value::timestamp_text(time::PrimitiveDateTime::new(
                                    value.date(),
                                    value.time()
                                )),
                                sign,
                                seconds / 3600,
                                seconds / 60 % 60,
                            )
                        })
                    }));
                }
                ColumnData::Date(_) => {
                    return text(
                        time::Date::from_sql(&value)
                            .map(|value| value.map(|value| value.to_string())),
                    );
                }
                ColumnData::Time(_) => {
                    return text(
                        time::Time::from_sql(&value)
                            .map(|value| value.map(super::value::time_text)),
                    );
                }
            })
        })
        .collect::<DatabaseResult<Vec<_>>>()?;
    Ok(Row::from_columns(names.into_iter().zip(values).collect()))
}

fn error(operation: &'static str, source: Error) -> DatabaseError {
    if let Error::Io { kind, message } = source {
        return DatabaseError::driver(operation)(sqlx::Error::Io(std::io::Error::new(
            kind, message,
        )));
    }
    DatabaseError::SqlServer { operation, source }
}
