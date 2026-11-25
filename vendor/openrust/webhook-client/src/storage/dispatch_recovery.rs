use super::{DatabaseWebhookStore, scope_key, text};
use crate::{Error, Result};
use openrust::{
    database::{
        Row, Transaction,
        query::{Builder, Expression, UpdateValue},
    },
    support::Str,
};

const TABLE: &str = "webhook_dispatch_intents";
const LEASE_SECONDS: u32 = 60;

impl DatabaseWebhookStore {
    pub(super) async fn record_dispatch_intent(
        &self,
        transaction: &mut Transaction,
        id: &str,
        route: String,
    ) -> Result<()> {
        let clock = self.clock();
        let timestamp = transaction
            .select_one(&format!("select {clock} as available_at"), &[])
            .await?
            .and_then(|row| row.get("available_at").cloned())
            .ok_or(Error::InvalidRecord)?;
        transaction
            .table(TABLE)
            .insert(&[[
                ("call_id".into(), id.into()),
                ("scope_key".into(), scope_key(&self.scope).into()),
                ("route".into(), route.into()),
                ("available_at".into(), timestamp),
            ]
            .into_iter()
            .collect()])
            .await?;
        Ok(())
    }

    pub(crate) async fn due_dispatch_intents(&self, limit: u32) -> Result<Vec<Row>> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("recover.limit"));
        }
        Ok(self
            .due_dispatches()
            .use_write_pdo()
            .select(["call_id", "route"])
            .order_by("available_at", "asc")
            .order_by("call_id", "asc")
            .limit(u64::from(limit))
            .get()
            .await?)
    }

    pub(crate) async fn claim_dispatch(&self, id: &str) -> Result<Option<String>> {
        let token = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
        let changed = self
            .due_dispatches()
            .r#where("call_id", "=", id)
            .update_with(
                [
                    (
                        "claim_token".into(),
                        UpdateValue::Operand(token.clone().into()),
                    ),
                    (
                        "available_at".into(),
                        UpdateValue::Operand(
                            Expression::new(format!("{} + {LEASE_SECONDS}", self.clock())).into(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        Ok((changed == 1).then_some(token))
    }

    pub(crate) async fn finish_dispatch(&self, id: &str, token: &str) -> Result<bool> {
        Ok(self
            .dispatch_intents()
            .r#where("call_id", "=", id)
            .r#where("claim_token", "=", token)
            .delete()
            .await?
            == 1)
    }

    pub(crate) async fn dispatch_metadata(
        &self,
        id: &str,
    ) -> Result<Option<(String, String, String)>> {
        let Some(row) = self.row(id, &["scope", "name", "revision"]).await? else {
            return Ok(None);
        };
        Ok(Some((
            self.scope.clone(),
            text(&row, "name")?.to_owned(),
            text(&row, "revision")?.to_owned(),
        )))
    }

    fn dispatch_intents(&self) -> Builder<'static> {
        self.connection
            .table(TABLE)
            .r#where("scope_key", "=", scope_key(&self.scope))
    }

    fn due_dispatches(&self) -> Builder<'static> {
        self.dispatch_intents()
            .where_raw(&format!("available_at <= {}", self.clock()), vec![])
    }
}
