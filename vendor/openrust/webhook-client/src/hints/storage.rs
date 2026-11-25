use super::WebhookHint;
use crate::{
    Error, Result,
    storage::{database_clock, scope_key},
};
use openrust::{
    database::{
        Connection, Value,
        query::{Builder, Record},
    },
    queue::{DispatchOptions, DispatchResult, Payload, QueueError, QueueManager},
    support::Str,
};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct HintStore {
    database: Connection,
    scope: String,
    profile: String,
}

impl HintStore {
    pub(super) fn new(database: Connection, scope: &str, profile: &str) -> Self {
        Self {
            database,
            scope: scope.to_owned(),
            profile: profile.to_owned(),
        }
    }

    pub(super) fn scope(&self) -> &str {
        &self.scope
    }
    pub(super) fn profile(&self) -> &str {
        &self.profile
    }

    pub(super) fn limiter_key(&self) -> String {
        format!(
            "webhook-client:hint:{}:{}",
            scope_key(&self.scope),
            scope_key(&self.profile)
        )
    }

    pub(super) fn new_hint(&self, revision: String, task_id: String) -> Result<WebhookHint> {
        Ok(WebhookHint {
            id: Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string(),
            scope: self.scope.clone(),
            profile: self.profile.clone(),
            revision,
            task_id,
        })
    }

    pub(super) async fn save_and_enqueue(
        &self,
        hint: WebhookHint,
        queue: Arc<QueueManager>,
        task: Payload,
        destination: DispatchOptions,
    ) -> Result<()> {
        let mut transaction = self.database.begin_transaction().await?;
        let result = async {
            let clock = database_clock(&self.database);
            let expiry = transaction
                .select_one(&format!("select {clock} + 300 as expires_at"), &[])
                .await?
                .and_then(|row| row.get("expires_at").cloned())
                .ok_or(Error::InvalidRecord)?;
            let row: Record = [
                ("id".into(), hint.id.into()),
                ("scope_key".into(), scope_key(&hint.scope).into()),
                ("scope".into(), hint.scope.into()),
                ("profile".into(), hint.profile.into()),
                ("revision".into(), hint.revision.into()),
                ("task_id".into(), hint.task_id.into()),
                ("status".into(), "pending".into()),
                ("expires_at".into(), expiry),
            ]
            .into_iter()
            .collect();
            transaction.table("webhook_hints").insert(&[row]).await?;
            match queue
                .dispatch_in_database_transaction(task, destination, &mut transaction)
                .await?
            {
                DispatchResult::Queued(_) => Ok(()),
                _ => Err(QueueError::InvalidConfiguration.into()),
            }
        }
        .await;
        match result {
            Ok(()) => Ok(transaction.commit().await?),
            Err(error) => {
                transaction.roll_back(None).await?;
                Err(error)
            }
        }
    }

    fn receipt(&self, id: &str, revision: &str) -> Builder<'static> {
        self.database
            .table("webhook_hints")
            .r#where("id", "=", id)
            .r#where("scope_key", "=", scope_key(&self.scope))
            .where_exact_text("scope", &self.scope)
            .where_exact_text("profile", &self.profile)
            .where_exact_text("revision", revision)
    }

    pub(super) async fn claim(&self, id: &str, revision: &str) -> Result<Option<WebhookHint>> {
        let clock = database_clock(&self.database);
        let changed = self
            .receipt(id, revision)
            .r#where("status", "=", "pending")
            .where_raw(&format!("expires_at > {clock}"), vec![])
            .update(
                [("status".to_owned(), Value::from("running"))]
                    .into_iter()
                    .collect(),
            )
            .await?;
        if changed == 0 {
            self.receipt(id, revision)
                .r#where("status", "=", "pending")
                .where_raw(&format!("expires_at <= {clock}"), vec![])
                .update(
                    [("status".to_owned(), Value::from("expired"))]
                        .into_iter()
                        .collect(),
                )
                .await?;
            return Ok(None);
        }
        let row = self
            .receipt(id, revision)
            .use_write_pdo()
            .select(["task_id"])
            .first()
            .await?
            .ok_or(Error::InvalidRecord)?;
        let Some(Value::Text(task_id)) = row.get("task_id") else {
            return Err(Error::InvalidRecord);
        };
        Ok(Some(WebhookHint {
            id: id.to_owned(),
            scope: self.scope.clone(),
            profile: self.profile.clone(),
            revision: revision.to_owned(),
            task_id: task_id.clone(),
        }))
    }

    pub(super) async fn finish(&self, hint: &WebhookHint, status: &str) -> Result<()> {
        let changed = self
            .receipt(hint.id(), hint.revision())
            .r#where("status", "=", "running")
            .update(
                [("status".to_owned(), Value::from(status))]
                    .into_iter()
                    .collect(),
            )
            .await?;
        if changed != 1 {
            return Err(Error::InvalidRecord);
        }
        Ok(())
    }
}
