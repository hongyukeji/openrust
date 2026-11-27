use openrust::database::{
    DatabaseError, DatabaseResult, Row, Value,
    query::{Builder, Record},
};

use super::UsageStore;
use crate::managed_keys::{integer, now, text};

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
}

impl LiveCall {
    fn decode(row: Row) -> DatabaseResult<Self> {
        Ok(Self {
            id: text(&row, "id")?,
            owner: text(&row, "owner")?,
            model: text(&row, "model")?,
            scope: text(&row, "scope")?,
            upstream_id: Some(text(&row, "upstream_id")?),
            expires_at: integer(&row, "expires_at")?.ok_or(DatabaseError::InvalidScalar)?,
        })
    }
}

impl UsageStore {
    pub(crate) async fn reserve_live_call(&self, call: &LiveCall) -> DatabaseResult<()> {
        self.connection
            .table(TABLE)
            .r#where("expires_at", "<=", now())
            .delete()
            .await?;
        let record = Record::from([
            ("id".into(), call.id.as_str().into()),
            ("owner".into(), call.owner.as_str().into()),
            ("model".into(), call.model.as_str().into()),
            ("scope".into(), call.scope.as_str().into()),
            ("expires_at".into(), call.expires_at.into()),
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
            .update(Record::from([("upstream_id".into(), id.into())]))
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
        closed: bool,
    ) -> DatabaseResult<()> {
        let query = self
            .live_call_row(call)
            .r#where("controller", "=", controller);
        if closed {
            query.delete().await?;
        } else {
            query
                .update(Record::from([
                    ("controller".into(), Value::Null),
                    ("controller_until".into(), 0i64.into()),
                ]))
                .await?;
        }
        Ok(())
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
