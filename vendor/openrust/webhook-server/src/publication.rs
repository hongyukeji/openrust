use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, HeaderValue, Method, header::CONTENT_TYPE};
use openrust::database::{Transaction, query::Builder};
use serde_json::{Value, json};

use crate::{
    DeliveryStore, Endpoint, EndpointState, Error, Result, TargetPolicy, WebhookCall,
    body::encode_json,
    delivery_store::{check_text, hash, settle, text},
    endpoints::{ENDPOINTS, endpoint},
    extensions,
    snapshot::{MessageReference, Snapshot},
};

pub(crate) const MESSAGES: &str = "webhook_messages";
const SUBSCRIPTIONS: &str = "webhook_subscriptions";
pub(crate) const MAX_ENDPOINTS: usize = 32;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub struct EventType {
    name: String,
    schema_version: u32,
}

impl EventType {
    pub fn new(name: &str, schema_version: u32) -> Result<Self> {
        extensions::validate_name("publication.event_type", name)?;
        if schema_version == 0 {
            return Err(Error::InvalidConfig("publication.schema_version"));
        }
        Ok(Self {
            name: name.into(),
            schema_version,
        })
    }

    fn key(&self) -> String {
        hash(format!("webhook-topic:{}:{}", self.schema_version, self.name).as_bytes())
    }
}

#[derive(serde::Serialize)]
pub struct PublishOptions {
    pub lifetime_seconds: u32,
    pub connection: Option<String>,
    pub queue: Option<String>,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            lifetime_seconds: 3600,
            connection: None,
            queue: None,
        }
    }
}

#[derive(serde::Serialize)]
pub struct PublishedMessage {
    pub message_id: String,
    pub delivery_ids: Vec<String>,
}

impl DeliveryStore {
    /// 将具名端点订阅到一个明确的事件类型和版本。
    pub async fn subscribe(&self, topic: &EventType, selected: &Endpoint) -> Result<()> {
        let Some(current) = self.endpoints().find(selected.name()).await? else {
            return Err(Error::EndpointRevoked);
        };
        if current.binding.id != selected.binding.id
            || current.revision() != selected.revision()
            || current.state() == EndpointState::Deleted
        {
            return Err(Error::EndpointRevoked);
        }
        let topic = topic.key();
        let key = subscription_id(&topic, selected);
        self.connection
            .table(SUBSCRIPTIONS)
            .insert_or_ignore(&[[
                ("id".into(), key.into()),
                ("scope_key".into(), self.scope_key.as_str().into()),
                ("topic_key".into(), topic.into()),
                ("endpoint_id".into(), selected.binding.id.as_str().into()),
            ]
            .into_iter()
            .collect()])
            .await?;
        Ok(())
    }

    pub async fn unsubscribe(&self, topic: &EventType, selected: &Endpoint) -> Result<bool> {
        let id = subscription_id(&topic.key(), selected);
        Ok(self
            .connection
            .table(SUBSCRIPTIONS)
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("id", "=", id)
            .delete()
            .await?
            == 1)
    }

    /// 一次提交固定全部订阅目标，已有 Scheduler 可恢复提交后的派发。
    pub async fn publish(
        &self,
        key: &str,
        topic: &EventType,
        payload: Value,
        options: &PublishOptions,
        policy: &TargetPolicy,
    ) -> Result<PublishedMessage> {
        let input = Publication::new(key, topic, payload, options, &self.scope_key)?;
        let mut tx = self.connection.begin_transaction().await?;
        let result = self
            .persist_publication(&mut tx, input, options, policy)
            .await;
        settle(tx, result).await
    }

    pub async fn publish_in_transaction(
        &self,
        tx: &mut Transaction,
        key: &str,
        topic: &EventType,
        payload: Value,
        options: &PublishOptions,
        policy: &TargetPolicy,
    ) -> Result<PublishedMessage> {
        let parent = tx.transaction_level();
        if !tx.belongs_to(&self.connection) || parent == 0 {
            return Err(Error::InvalidConfig("publication.transaction"));
        }
        let input = Publication::new(key, topic, payload, options, &self.scope_key)?;
        tx.begin_transaction().await?;
        match self.persist_publication(tx, input, options, policy).await {
            Ok(message) => {
                tx.commit().await?;
                Ok(message)
            }
            Err(error) => {
                tx.roll_back(Some(parent)).await?;
                Err(error)
            }
        }
    }

    async fn persist_publication(
        &self,
        tx: &mut Transaction,
        input: Publication<'_>,
        options: &PublishOptions,
        policy: &TargetPolicy,
    ) -> Result<PublishedMessage> {
        let now = self.time_in(tx).await?;
        let inserted = tx
            .table(MESSAGES)
            .insert_or_ignore(&[[
                ("id".into(), input.id.as_str().into()),
                ("scope_key".into(), self.scope_key.as_str().into()),
                ("topic".into(), input.topic.name.as_str().into()),
                (
                    "schema_version".into(),
                    i64::from(input.topic.schema_version).into(),
                ),
                ("created_at".into(), now.into()),
                ("fingerprint".into(), input.fingerprint.as_str().into()),
                ("body".into(), input.body.into()),
                ("body_digest".into(), input.body_digest.as_str().into()),
                ("delivery_ids".into(), "[]".into()),
            ]
            .into_iter()
            .collect()])
            .await?;
        if inserted == 0 {
            let row = tx
                .table(MESSAGES)
                .r#where("scope_key", "=", self.scope_key.as_str())
                .r#where("id", "=", input.id.as_str())
                .select(["fingerprint", "delivery_ids"])
                .shared_lock()
                .first()
                .await?
                .ok_or(Error::InvalidRecord)?;
            if text(&row, "fingerprint")? != input.fingerprint {
                return Err(Error::IdempotencyConflict);
            }
            let delivery_ids = serde_json::from_str(text(&row, "delivery_ids")?)
                .map_err(|_| Error::InvalidRecord)?;
            return Ok(PublishedMessage {
                message_id: input.id,
                delivery_ids,
            });
        }

        let targets = tx
            .table(ENDPOINTS)
            .join(
                SUBSCRIPTIONS,
                "webhook_endpoints.id",
                "=",
                "webhook_subscriptions.endpoint_id",
            )
            .r#where(
                "webhook_subscriptions.scope_key",
                "=",
                self.scope_key.as_str(),
            )
            .r#where("webhook_endpoints.scope_key", "=", self.scope_key.as_str())
            .r#where("webhook_subscriptions.topic_key", "=", input.topic.key())
            .r#where("webhook_endpoints.state", "<>", "deleted")
            .select(["webhook_endpoints.*"])
            .order_by("webhook_endpoints.id", "asc")
            .limit((MAX_ENDPOINTS + 1) as u64)
            .get()
            .await?;
        if targets.len() > MAX_ENDPOINTS {
            return Err(Error::FanoutLimit);
        }
        let mut delivery_ids = Vec::with_capacity(targets.len());
        for row in &targets {
            let target = endpoint(row)?;
            let id = hash(
                format!(
                    "webhook-fanout:{}:{}:{}",
                    input.id,
                    target.binding.id,
                    target.revision()
                )
                .as_bytes(),
            );
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            let mut builder = WebhookCall::create()?
                .use_endpoint(&target)
                .uuid(&input.id)
                .use_http_verb(Method::POST)
                .with_headers(headers)
                .send_raw_body(Vec::new());
            if options.connection.is_some() {
                builder = builder.on_connection(options.connection.clone());
            }
            if options.queue.is_some() {
                builder = builder.on_queue(options.queue.clone());
            }
            let mut snapshot = builder.prepare(policy)?.freeze(options.lifetime_seconds)?;
            snapshot.message_ref = Some(MessageReference {
                id: input.id.clone(),
                digest: input.body_digest.clone(),
            });
            if self.insert_delivery(tx, &id, &snapshot).await?.0 != 1 {
                return Err(Error::IdempotencyConflict);
            }
            delivery_ids.push(id);
        }
        let encoded = serde_json::to_string(&delivery_ids).map_err(|_| Error::InvalidRecord)?;
        tx.table(MESSAGES)
            .r#where("id", "=", input.id.as_str())
            .r#where("scope_key", "=", self.scope_key.as_str())
            .update(
                [("delivery_ids".into(), encoded.into())]
                    .into_iter()
                    .collect(),
            )
            .await?;
        Ok(PublishedMessage {
            message_id: input.id,
            delivery_ids,
        })
    }

    pub(crate) async fn hydrate_message(
        &self,
        mut snapshot: Snapshot,
        query: Builder<'_>,
    ) -> Result<Snapshot> {
        let Some(reference) = &snapshot.message_ref else {
            return Ok(snapshot);
        };
        let row = query
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("id", "=", reference.id.as_str())
            .select(["body", "body_digest"])
            .first()
            .await?
            .ok_or(Error::InvalidRecord)?;
        let body = text(&row, "body")?;
        if reference.id != snapshot.uuid
            || reference.digest != text(&row, "body_digest")?
            || body.len() > MAX_MESSAGE_BYTES.div_ceil(3) * 4
            || hash(body.as_bytes()) != reference.digest
        {
            return Err(Error::InvalidRecord);
        }
        snapshot.hydrated_body = Some(body.into());
        Ok(snapshot)
    }
}

struct Publication<'a> {
    topic: &'a EventType,
    id: String,
    fingerprint: String,
    body: String,
    body_digest: String,
}

impl<'a> Publication<'a> {
    fn new(
        key: &str,
        topic: &'a EventType,
        payload: Value,
        options: &PublishOptions,
        scope: &str,
    ) -> Result<Self> {
        check_text("publication.idempotency_key", key)?;
        if !(1..=2_592_000).contains(&options.lifetime_seconds) {
            return Err(Error::InvalidConfig("publication.lifetime"));
        }
        let envelope =
            json!({"type": topic.name, "schema_version": topic.schema_version, "data": payload});
        let body = STANDARD.encode(encode_json(&envelope, MAX_MESSAGE_BYTES)?);
        let body_digest = hash(body.as_bytes());
        let fingerprint = hash(
            &serde_json::to_vec(&(&topic.name, topic.schema_version, &body_digest, options))
                .map_err(|_| Error::InvalidRecord)?,
        );
        let id = hash(format!("webhook-publication:{scope}:{key}").as_bytes());
        Ok(Self {
            topic,
            id,
            fingerprint,
            body,
            body_digest,
        })
    }
}

fn subscription_id(topic: &str, endpoint: &Endpoint) -> String {
    hash(format!("webhook-subscription:{topic}:{}", endpoint.binding.id).as_bytes())
}
