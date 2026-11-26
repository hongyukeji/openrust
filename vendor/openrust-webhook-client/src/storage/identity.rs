use crate::{Error, Result, VerifiedWebhook};
use aws_lc_rs::digest::{Context, SHA256};
use openrust::database::{Connection, Transaction, Value, query::Record};

pub(super) struct EventIdentity {
    key: String,
    fingerprint: String,
    window_seconds: Option<u32>,
}

impl EventIdentity {
    pub(super) fn from_verified(scope: &str, webhook: &VerifiedWebhook<'_>) -> Option<Self> {
        let event_id = webhook.event_id();
        let window_seconds = webhook.body_deduplication_window();
        if event_id.is_none() && window_seconds.is_none() {
            return None;
        }
        let request = webhook.request();
        let mut fingerprint = Context::new(&SHA256);
        hash_field(&mut fingerprint, request.method().as_str().as_bytes());
        hash_field(
            &mut fingerprint,
            request
                .uri()
                .path_and_query()
                .map_or("", |value| value.as_str())
                .as_bytes(),
        );
        hash_field(&mut fingerprint, request.body());
        for value in request.headers().get_all(http::header::CONTENT_TYPE) {
            hash_field(&mut fingerprint, value.as_bytes());
        }
        let fingerprint = hex(fingerprint);
        let mut key = Context::new(&SHA256);
        for value in [scope, webhook.config().name()] {
            hash_field(&mut key, value.as_bytes());
        }
        if let Some(event_id) = event_id {
            // 不改变已经落库的稳定事件键。
            hash_field(&mut key, event_id.as_bytes());
        } else {
            hash_field(&mut key, b"body-digest");
            hash_field(&mut key, fingerprint.as_bytes());
        }
        Some(Self {
            key: hex(key),
            fingerprint,
            window_seconds,
        })
    }

    pub(super) async fn record(
        &self,
        transaction: &mut Transaction,
        call_id: &str,
        clock: &str,
    ) -> Result<Record> {
        let expires_at = if let Some(seconds) = self.window_seconds {
            let expired = format!("expires_at <= {clock}");
            let stale = transaction
                .table("webhook_event_receipts")
                .select(["event_key"])
                .r#where("event_key", "=", self.key.as_str())
                .where_raw(&expired, vec![])
                .first()
                .await?
                .is_some();
            // 缺失键直接竞争唯一索引，避免无记录时的删除间隙锁。
            if stale {
                transaction
                    .table("webhook_event_receipts")
                    .r#where("event_key", "=", self.key.as_str())
                    .where_raw(&expired, vec![])
                    .delete()
                    .await?;
            }
            let row = transaction
                .table("webhook_calls")
                .select_raw(
                    &format!("{clock} + ? as deduplication_expiry"),
                    vec![i64::from(seconds).into()],
                )
                .r#where("id", "=", call_id)
                .first()
                .await?
                .ok_or(Error::InvalidRecord)?;
            row.get("deduplication_expiry")
                .cloned()
                .ok_or(Error::InvalidRecord)?
        } else {
            Value::Null
        };
        Ok([
            ("event_key".into(), self.key.as_str().into()),
            ("fingerprint".into(), self.fingerprint.as_str().into()),
            ("call_id".into(), call_id.into()),
            ("expires_at".into(), expires_at),
        ]
        .into_iter()
        .collect())
    }

    pub(super) async fn already_received(
        &self,
        connection: &Connection,
        clock: &str,
    ) -> Result<bool> {
        let known = connection
            .table("webhook_event_receipts")
            .use_write_pdo()
            .select(["fingerprint"])
            .r#where("event_key", "=", self.key.as_str())
            .where_nested(|query| {
                query
                    .where_null("expires_at")
                    .or_where_raw(&format!("expires_at > {clock}"), vec![])
            })
            .first()
            .await?;
        match known.as_ref().and_then(|row| row.get("fingerprint")) {
            Some(Value::Text(fingerprint)) if fingerprint == &self.fingerprint => Ok(true),
            Some(Value::Text(_)) => Err(Error::VerifiedEventConflict),
            None if known.is_none() => Ok(false),
            _ => Err(Error::InvalidRecord),
        }
    }
}

fn hash_field(context: &mut Context, bytes: &[u8]) {
    context.update(&(bytes.len() as u64).to_be_bytes());
    context.update(bytes);
}

fn hex(context: Context) -> String {
    context
        .finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
