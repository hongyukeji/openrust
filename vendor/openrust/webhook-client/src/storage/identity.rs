use crate::{Error, Result, VerifiedWebhook};
use aws_lc_rs::digest::{Context, SHA256};
use openrust::database::{Connection, Value, query::Record};

pub(super) struct EventIdentity {
    key: String,
    fingerprint: String,
}

impl EventIdentity {
    pub(super) fn new(scope: &str, event_id: &str, webhook: &VerifiedWebhook<'_>) -> Self {
        let request = webhook.request();
        let mut key = Context::new(&SHA256);
        for value in [scope, webhook.config().name(), event_id] {
            hash_field(&mut key, value.as_bytes());
        }
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
        Self {
            key: hex(key),
            fingerprint: hex(fingerprint),
        }
    }

    pub(super) fn record(&self, call_id: &str) -> Record {
        [
            ("event_key".into(), self.key.as_str().into()),
            ("fingerprint".into(), self.fingerprint.as_str().into()),
            ("call_id".into(), call_id.into()),
        ]
        .into_iter()
        .collect()
    }

    pub(super) async fn already_received(&self, connection: &Connection) -> Result<bool> {
        let known = connection
            .table("webhook_event_receipts")
            .use_write_pdo()
            .select(["fingerprint"])
            .r#where("event_key", "=", self.key.as_str())
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
