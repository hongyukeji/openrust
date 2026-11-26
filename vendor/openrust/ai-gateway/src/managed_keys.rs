use std::time::SystemTime;

use openrust::database::{Connection, DatabaseError, DatabaseResult, Row, Value, query::Record};
use serde::Serialize;
use sha2::{Digest, Sha256};

const TABLE: &str = "ai_gateway_keys";
pub(crate) const PREFIX: &str = "orgw_";
const PUBLIC_COLUMNS: [&str; 8] = [
    "id",
    "name",
    "group_name",
    "requests_per_minute",
    "created_at",
    "updated_at",
    "expires_at",
    "revoked_at",
];

pub(crate) struct KeyStore {
    connection: Connection,
}

#[derive(Serialize)]
pub(crate) struct KeyRecord {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) group: String,
    pub(crate) requests_per_minute: Option<i64>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) expires_at: Option<i64>,
    pub(crate) revoked_at: Option<i64>,
}

impl KeyStore {
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub(crate) async fn authenticate(&self, token: &str) -> DatabaseResult<Option<KeyRecord>> {
        let Some(secret) = token.strip_prefix(PREFIX) else {
            return Ok(None);
        };
        if secret.len() != 64
            || !secret
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Ok(None);
        }
        let row = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .select(PUBLIC_COLUMNS)
            .r#where("digest", "=", digest(token))
            .where_null("revoked_at")
            .first()
            .await?;
        let record = row.map(decode).transpose()?;
        Ok(record.filter(|record| {
            record
                .expires_at
                .is_none_or(|expires_at| expires_at > now())
        }))
    }

    pub(crate) async fn create(&self, record: &KeyRecord, token: &str) -> DatabaseResult<()> {
        let created = self
            .connection
            .table(TABLE)
            .insert(&[Record::from([
                ("id".into(), record.id.as_str().into()),
                ("name".into(), record.name.as_str().into()),
                ("group_name".into(), record.group.as_str().into()),
                ("digest".into(), digest(token).into()),
                (
                    "requests_per_minute".into(),
                    nullable(record.requests_per_minute),
                ),
                ("created_at".into(), record.created_at.into()),
                ("updated_at".into(), record.updated_at.into()),
                ("expires_at".into(), nullable(record.expires_at)),
                ("revoked_at".into(), Value::Null),
            ])])
            .await?;
        if !created {
            return Err(DatabaseError::InvalidQuery("AI 网关 Key 未保存"));
        }
        Ok(())
    }

    pub(crate) async fn rotate(&self, id: &str, token: &str) -> DatabaseResult<bool> {
        let Some(row) = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .select(["digest", "expires_at"])
            .r#where("id", "=", id)
            .where_null("revoked_at")
            .first()
            .await?
        else {
            return Ok(false);
        };
        let updated_at = now();
        if integer(&row, "expires_at")?.is_some_and(|expires_at| expires_at <= updated_at) {
            return Ok(false);
        }
        let previous = text(&row, "digest")?;
        let changed = self
            .connection
            .table(TABLE)
            .r#where("id", "=", id)
            .r#where("digest", "=", previous)
            .where_null("revoked_at")
            .update(Record::from([
                ("digest".into(), digest(token).into()),
                ("updated_at".into(), updated_at.into()),
            ]))
            .await?;
        Ok(changed == 1)
    }

    pub(crate) async fn revoke(&self, id: &str) -> DatabaseResult<bool> {
        let revoked_at = now();
        let changed = self
            .connection
            .table(TABLE)
            .r#where("id", "=", id)
            .where_null("revoked_at")
            .update(Record::from([
                ("digest".into(), Value::Null),
                ("updated_at".into(), revoked_at.into()),
                ("revoked_at".into(), revoked_at.into()),
            ]))
            .await?;
        Ok(changed == 1)
    }

    pub(crate) async fn list(
        &self,
        before: Option<&str>,
        limit: u64,
    ) -> DatabaseResult<Vec<KeyRecord>> {
        let mut query = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .select(PUBLIC_COLUMNS)
            .order_by_desc("id")
            .limit(limit);
        if let Some(before) = before {
            query = query.r#where("id", "<", before);
        }
        query.get().await?.into_iter().map(decode).collect()
    }
}

fn decode(row: Row) -> DatabaseResult<KeyRecord> {
    let requests_per_minute = integer(&row, "requests_per_minute")?;
    if requests_per_minute.is_some_and(|limit| limit <= 0) {
        return Err(DatabaseError::InvalidScalar);
    }
    Ok(KeyRecord {
        id: text(&row, "id")?,
        name: text(&row, "name")?,
        group: text(&row, "group_name")?,
        requests_per_minute,
        created_at: integer(&row, "created_at")?.ok_or(DatabaseError::InvalidScalar)?,
        updated_at: integer(&row, "updated_at")?.ok_or(DatabaseError::InvalidScalar)?,
        expires_at: integer(&row, "expires_at")?,
        revoked_at: integer(&row, "revoked_at")?,
    })
}

fn text(row: &Row, column: &str) -> DatabaseResult<String> {
    match row.get(column) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(DatabaseError::InvalidScalar),
    }
}

fn integer(row: &Row, column: &str) -> DatabaseResult<Option<i64>> {
    match row.get(column) {
        Some(Value::Integer(value)) => Ok(Some(*value)),
        Some(Value::Null) => Ok(None),
        _ => Err(DatabaseError::InvalidScalar),
    }
}

fn nullable(value: Option<i64>) -> Value {
    value.map_or(Value::Null, Into::into)
}

fn digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token))
}

pub(crate) fn now() -> i64 {
    SystemTime::UNIX_EPOCH
        .elapsed()
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
