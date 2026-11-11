use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{
    contracts::session::SessionHandler,
    database::{
        Connection, Value,
        query::{Builder, Record},
    },
};

use super::{SessionError, SessionFuture, SessionResult};

const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_ENCODED_BYTES: usize = MAX_PAYLOAD_BYTES.div_ceil(3) * 4;

/// Request metadata persisted with a database session. A missing field leaves its column untouched.
#[derive(Clone, Default)]
pub struct DatabaseSessionMetadata {
    pub user_id: Option<Value>,
    pub ip_address: Option<Option<String>>,
    pub user_agent: Option<String>,
}

/// Stores sessions in the configured database table on the write connection.
pub struct DatabaseSessionHandler {
    connection: Connection,
    table: String,
    lifetime: Duration,
    metadata: DatabaseSessionMetadata,
}

#[allow(non_snake_case, reason = "Matches Laravel's database session API")]
impl DatabaseSessionHandler {
    pub fn new(
        connection: Connection,
        table: impl Into<String>,
        minutes: u64,
    ) -> SessionResult<Self> {
        let table = table.into();
        if table.trim().is_empty() {
            return Err(SessionError::Configuration);
        }
        let seconds = minutes
            .checked_mul(60)
            .filter(|seconds| i64::try_from(*seconds).is_ok())
            .ok_or(SessionError::Configuration)?;
        Ok(Self {
            connection,
            table,
            lifetime: Duration::from_secs(seconds),
            metadata: DatabaseSessionMetadata::default(),
        })
    }

    pub fn setMetadata(&mut self, metadata: DatabaseSessionMetadata) -> &mut Self {
        self.metadata = metadata;
        self
    }

    /// Checks row existence, including expired rows that have not been collected.
    pub async fn validateId(&self, id: &str) -> SessionResult<bool> {
        self.query()
            .select(["id"])
            .r#where("id", "=", id)
            .first()
            .await
            .map(|row| row.is_some())
            .map_err(|_| SessionError::Storage)
    }

    fn query(&self) -> Builder<'static> {
        self.connection.table(&self.table).useWritePdo()
    }

    fn record(&self, id: &str, payload: &[u8]) -> SessionResult<Record> {
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(SessionError::PayloadTooLarge);
        }
        let mut row = Record::from([
            ("id".into(), id.into()),
            ("last_activity".into(), now()?.into()),
            ("payload".into(), STANDARD.encode(payload).into()),
        ]);
        if let Some(user_id) = &self.metadata.user_id {
            row.insert("user_id".into(), user_id.clone());
        }
        if let Some(ip) = &self.metadata.ip_address {
            row.insert(
                "ip_address".into(),
                ip.as_deref().map_or(Value::Null, Value::from),
            );
        }
        if let Some(agent) = &self.metadata.user_agent {
            row.insert(
                "user_agent".into(),
                agent.chars().take(500).collect::<String>().into(),
            );
        }
        Ok(row)
    }
}

impl SessionHandler for DatabaseSessionHandler {
    fn read<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            let row = self
                .query()
                .select(["payload", "last_activity"])
                .r#where("id", "=", id)
                .first()
                .await
                .map_err(|_| SessionError::Storage)?;
            let Some(row) = row else { return Ok(None) };
            let activity = match row.get("last_activity") {
                Some(Value::Integer(value)) => *value,
                Some(Value::Unsigned(value)) => {
                    i64::try_from(*value).map_err(|_| SessionError::Storage)?
                }
                _ => return Err(SessionError::Storage),
            };
            if activity < cutoff(self.lifetime)? {
                return Ok(None);
            }
            let payload = match row.get("payload") {
                Some(Value::Text(value)) => value.as_bytes(),
                Some(Value::Bytes(value)) => value.as_slice(),
                _ => return Err(SessionError::Encoding),
            };
            if payload.len() > MAX_ENCODED_BYTES {
                return Err(SessionError::PayloadTooLarge);
            }
            let bytes = STANDARD
                .decode(payload)
                .map_err(|_| SessionError::Encoding)?;
            if bytes.len() > MAX_PAYLOAD_BYTES {
                return Err(SessionError::PayloadTooLarge);
            }
            Ok(Some(bytes))
        })
    }

    fn write<'a>(
        &'a mut self,
        id: &'a str,
        payload: &'a [u8],
    ) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            let row = self.record(id, payload)?;
            let columns: Vec<_> = row
                .keys()
                .filter(|key| key.as_str() != "id")
                .cloned()
                .collect();
            let updates: Vec<_> = columns.iter().map(String::as_str).collect();
            self.query()
                .upsert(&[row], &["id"], Some(&updates))
                .await
                .map(|_| ())
                .map_err(|_| SessionError::Storage)
        })
    }

    fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            self.query()
                .r#where("id", "=", id)
                .delete()
                .await
                .map(|_| ())
                .map_err(|_| SessionError::Storage)
        })
    }

    fn gc(&mut self, max_lifetime: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
        Box::pin(async move {
            self.query()
                .r#where("last_activity", "<=", cutoff(max_lifetime)?)
                .delete()
                .await
                .map(Some)
                .map_err(|_| SessionError::Storage)
        })
    }
}

fn cutoff(lifetime: Duration) -> SessionResult<i64> {
    let lifetime = i64::try_from(lifetime.as_secs()).map_err(|_| SessionError::Configuration)?;
    Ok(now()?.saturating_sub(lifetime))
}

fn now() -> SessionResult<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SessionError::Clock)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| SessionError::Clock)
}
