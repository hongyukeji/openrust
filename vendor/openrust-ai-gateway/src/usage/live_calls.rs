use openrust::database::{
    DatabaseError, DatabaseResult, Row, Value,
    query::{Builder, Record},
};

use super::{Settlement, TABLE as USAGE_TABLE, UsageStore, finish_usage};
use crate::managed_keys::{integer, now, text};
use serde_json::json;

const TABLE: &str = "ai_gateway_live_calls";
const CONTROL_LEASE_SECONDS: i64 = 60;

#[derive(Clone)]
pub(crate) struct LiveCall {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) model: String,
    pub(crate) scope: String,
    pub(crate) upstream_id: Option<String>,
    pub(crate) expires_at: i64,
    pub(crate) usage_id: Option<String>,
    pub(crate) started_at: Option<i64>,
}

impl LiveCall {
    fn decode(row: Row) -> DatabaseResult<Self> {
        let optional = |column| match row.get(column) {
            Some(Value::Null) => Ok(None),
            Some(Value::Text(value)) => Ok(Some(value.clone())),
            _ => Err(DatabaseError::InvalidScalar),
        };
        Ok(Self {
            id: text(&row, "id")?,
            owner: text(&row, "owner")?,
            model: text(&row, "model")?,
            scope: text(&row, "scope")?,
            upstream_id: optional("upstream_id")?,
            expires_at: integer(&row, "expires_at")?.ok_or(DatabaseError::InvalidScalar)?,
            usage_id: optional("usage_id")?,
            started_at: integer(&row, "started_at")?,
        })
    }
}

impl UsageStore {
    pub(crate) async fn reserve_live_call(&self, call: &LiveCall) -> DatabaseResult<()> {
        let record = Record::from([
            ("id".into(), call.id.as_str().into()),
            ("owner".into(), call.owner.as_str().into()),
            ("model".into(), call.model.as_str().into()),
            ("scope".into(), call.scope.as_str().into()),
            ("expires_at".into(), call.expires_at.into()),
            (
                "usage_id".into(),
                call.usage_id.as_deref().map_or(Value::Null, Into::into),
            ),
        ]);
        if !self.connection.table(TABLE).insert(&[record]).await? {
            return Err(DatabaseError::InvalidQuery("Live 会话登记失败"));
        }
        Ok(())
    }

    pub(crate) async fn attach_live_call(&self, call: &LiveCall, id: &str) -> DatabaseResult<()> {
        let changed = self
            .live_call_row(call)
            .where_null("upstream_id")
            .update(Record::from([
                ("upstream_id".into(), id.into()),
                ("started_at".into(), now().into()),
            ]))
            .await?;
        if changed != 1 {
            return Err(DatabaseError::InvalidQuery("Live 会话登记状态已变更"));
        }
        Ok(())
    }

    pub(crate) async fn live_call(
        &self,
        owner: &str,
        id: &str,
    ) -> DatabaseResult<Option<LiveCall>> {
        let record = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", id)
            .r#where("owner", "=", owner)
            .r#where("expires_at", ">", now())
            .where_not_null("upstream_id")
            .first()
            .await?
            .map(LiveCall::decode)
            .transpose()?;
        Ok(record.filter(|row| row.id == id && row.owner == owner))
    }

    pub(crate) async fn claim_live_control(
        &self,
        call: &LiveCall,
        controller: &str,
    ) -> DatabaseResult<bool> {
        let timestamp = now();
        Ok(self
            .live_call_row(call)
            .r#where("expires_at", ">", timestamp)
            .r#where("controller_until", "<=", timestamp)
            .update(Record::from([
                ("controller".into(), controller.into()),
                ("observing".into(), false.into()),
                ("handoff_until".into(), 0i64.into()),
                (
                    "controller_until".into(),
                    (timestamp + CONTROL_LEASE_SECONDS).into(),
                ),
            ]))
            .await?
            == 1)
    }

    pub(crate) async fn renew_live_control(
        &self,
        call: &LiveCall,
        controller: &str,
    ) -> DatabaseResult<bool> {
        let timestamp = now();
        Ok(self
            .live_call_row(call)
            .r#where("controller", "=", controller)
            .r#where("controller_until", ">", timestamp)
            .r#where("expires_at", ">", timestamp)
            .update(Record::from([(
                "controller_until".into(),
                (timestamp + CONTROL_LEASE_SECONDS).into(),
            )]))
            .await?
            == 1)
    }

    pub(crate) async fn release_live_control(
        &self,
        call: &LiveCall,
        controller: &str,
    ) -> DatabaseResult<()> {
        self.live_call_row(call)
            .r#where("controller", "=", controller)
            .update(Record::from([
                ("controller".into(), Value::Null),
                ("controller_until".into(), 0i64.into()),
                ("observing".into(), false.into()),
            ]))
            .await?;
        Ok(())
    }

    pub(crate) async fn request_live_handoff(&self, call: &LiveCall) -> DatabaseResult<bool> {
        let timestamp = now();
        Ok(self
            .live_call_row(call)
            .r#where("observing", "=", true)
            .r#where("controller_until", ">", timestamp)
            .update(Record::from([(
                "handoff_until".into(),
                (timestamp + 10).into(),
            )]))
            .await?
            != 0)
    }

    pub(crate) async fn live_observation_candidates(
        &self,
        limit: u64,
    ) -> DatabaseResult<Vec<LiveCall>> {
        let timestamp = now();
        self.connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("controller_until", "<=", timestamp)
            .r#where("handoff_until", "<=", timestamp)
            .where_nested(|query| {
                query
                    .where_not_null("upstream_id")
                    .or_where("expires_at", "<=", timestamp)
            })
            .order_by("expires_at", "asc")
            .order_by("id", "asc")
            .limit(limit)
            .get()
            .await?
            .into_iter()
            .map(LiveCall::decode)
            .collect()
    }

    pub(crate) async fn claim_live_observer(
        &self,
        call: &LiveCall,
        controller: &str,
    ) -> DatabaseResult<bool> {
        let timestamp = now();
        Ok(self
            .live_call_row(call)
            .r#where("handoff_until", "<=", timestamp)
            .r#where("controller_until", "<=", timestamp)
            .update(Record::from([
                ("controller".into(), controller.into()),
                ("observing".into(), true.into()),
                (
                    "controller_until".into(),
                    (timestamp + CONTROL_LEASE_SECONDS).into(),
                ),
            ]))
            .await?
            == 1)
    }

    pub(crate) async fn live_observer_should_yield(
        &self,
        call: &LiveCall,
        controller: &str,
    ) -> DatabaseResult<bool> {
        Ok(self
            .live_call_row(call)
            .use_write_pdo()
            .r#where("controller", "=", controller)
            .r#where("handoff_until", "<=", now())
            .first()
            .await?
            .is_none())
    }

    pub(crate) async fn delay_live_observer(
        &self,
        call: &LiveCall,
        controller: &str,
    ) -> DatabaseResult<()> {
        self.live_call_row(call)
            .r#where("controller", "=", controller)
            .r#where("handoff_until", "<=", now())
            .update(Record::from([("handoff_until".into(), (now() + 5).into())]))
            .await?;
        Ok(())
    }

    pub(crate) async fn finish_live_call(
        &self,
        call: &LiveCall,
        controller: &str,
        outcome: &'static str,
    ) -> DatabaseResult<()> {
        let call = call.clone();
        let controller = controller.to_owned();
        self.connection
            .transaction(
                move |transaction| {
                    let call = call.clone();
                    let controller = controller.clone();
                    Box::pin(async move {
                        let deleted = transaction
                            .table(TABLE)
                            .r#where("id", "=", call.id.as_str())
                            .r#where("owner", "=", call.owner.as_str())
                            .r#where("controller", "=", controller)
                            .delete()
                            .await?;
                        if deleted != 1 {
                            return Ok(());
                        }
                        if let Some(id) = &call.usage_id {
                            let row = transaction
                                .table(USAGE_TABLE)
                                .r#where("id", "=", id.as_str())
                                .first()
                                .await?
                                .ok_or(DatabaseError::InvalidScalar)?;
                            let started = integer(&row, "dispatch_started_at")?.is_some();
                            let mut context = openrust::log::LogContext::new();
                            if let Some(at) = call.started_at {
                                let duration = now()
                                    .min(call.expires_at)
                                    .saturating_sub(at)
                                    .max(0)
                                    .saturating_mul(1000);
                                context.insert(
                                    "resource_usage".into(),
                                    json!({"session_duration_ms":duration}),
                                );
                            }
                            if let Some(status) = integer(&row, "status")? {
                                context.insert("status".into(), json!(status));
                            }
                            context.insert(
                                "upstream_endpoint".into(),
                                json!(text(&row, "upstream_endpoint")?),
                            );
                            finish_usage(
                                transaction.table(USAGE_TABLE),
                                id,
                                &Settlement::from_context(outcome, &context, started),
                            )
                            .await?;
                        }
                        Ok(())
                    })
                },
                3,
            )
            .await
    }

    pub(crate) async fn forget_pending_live_call(&self, call: &LiveCall) -> DatabaseResult<()> {
        self.live_call_row(call)
            .where_null("upstream_id")
            .delete()
            .await?;
        Ok(())
    }

    fn live_call_row(&self, call: &LiveCall) -> Builder<'static> {
        self.connection
            .table(TABLE)
            .r#where("id", "=", call.id.as_str())
            .r#where("owner", "=", call.owner.as_str())
    }
}
