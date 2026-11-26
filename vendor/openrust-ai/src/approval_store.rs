use std::collections::BTreeMap;

use openrust::{
    database::{
        Connection, DatabaseError, DatabaseResult, Row, Transaction, Value,
        query::{Builder, Record},
    },
    support::Str,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value as Json};

use crate::{
    AiError, AiResult, ConversationParticipant, ToolResult, conversation_store::now,
    messages::PromptAttachmentArchive, tools::ApprovalCheckpoint,
};

const TABLE: &str = "agent_tool_approvals";
const TTL_SECONDS: i64 = 86_400;

pub(crate) struct ApprovalStore {
    connection: Connection,
    disk: String,
}

pub(crate) struct ApprovalScope<'a> {
    pub(crate) conversation_id: &'a str,
    pub(crate) participant: &'a ConversationParticipant,
    pub(crate) agent: &'a str,
}

pub(crate) enum SavedApproval<T> {
    Pending { snapshot: T, revision: i64 },
    Expired,
    Consumed,
}

impl ApprovalStore {
    pub(crate) fn new(connection: Connection, disk: String) -> Self {
        Self { connection, disk }
    }

    pub(crate) fn disk(&self) -> &str {
        &self.disk
    }

    fn query(&self, scope: &ApprovalScope<'_>, id: &str) -> Builder<'static> {
        scoped(self.connection.table(TABLE).use_write_pdo(), scope, id)
    }

    pub(crate) async fn insert(
        &self,
        scope: &ApprovalScope<'_>,
        id: &str,
        snapshot: &impl Serialize,
    ) -> DatabaseResult<()> {
        let timestamp = now();
        let row = Record::from([
            ("id".into(), id.into()),
            ("conversation_id".into(), scope.conversation_id.into()),
            (
                "participant_key".into(),
                scope.participant.key(scope.agent).into(),
            ),
            ("state".into(), "pending".into()),
            ("snapshot".into(), encode(snapshot)?.into()),
            ("revision".into(), 0_i64.into()),
            (
                "expires_at".into(),
                timestamp.saturating_add(TTL_SECONDS).into(),
            ),
            ("created_at".into(), timestamp.into()),
            ("updated_at".into(), timestamp.into()),
        ]);
        if !self.connection.table(TABLE).insert(&[row]).await? {
            return Err(DatabaseError::InvalidQuery("AI 工具审批记录未保存"));
        }
        Ok(())
    }

    pub(crate) async fn load<T: DeserializeOwned>(
        &self,
        scope: &ApprovalScope<'_>,
        id: &str,
    ) -> DatabaseResult<Option<SavedApproval<T>>> {
        let Some(row) = self
            .query(scope, id)
            .select(["state", "expires_at", "snapshot", "revision"])
            .first()
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(match text(&row, "state")? {
            "pending" if integer(&row, "expires_at")? <= now() => SavedApproval::Expired,
            "pending" => SavedApproval::Pending {
                snapshot: serde_json::from_str(text(&row, "snapshot")?)
                    .map_err(|_| DatabaseError::InvalidScalar)?,
                revision: integer(&row, "revision")?,
            },
            _ => SavedApproval::Consumed,
        }))
    }

    pub(crate) async fn migrate_attachments(&self) -> DatabaseResult<u64> {
        if !self
            .connection
            .get_schema_builder()
            .has_table(TABLE)
            .await?
        {
            return Ok(0);
        }
        let mut migrated = 0;
        let mut after = None::<String>;
        loop {
            let mut query = self
                .connection
                .table(TABLE)
                .use_write_pdo()
                .select(["id", "snapshot", "revision"])
                .r#where("state", "=", "pending")
                .where_like("snapshot", "%base64-%", true)
                .order_by("id", "asc");
            if let Some(after) = &after {
                query = query.r#where("id", ">", after.as_str());
            }
            let Some(row) = query.first().await? else {
                return Ok(migrated);
            };
            let id = text(&row, "id")?;
            let legacy = text(&row, "snapshot")?;
            let mut snapshot: ApprovalCheckpoint =
                serde_json::from_str(legacy).map_err(|_| DatabaseError::InvalidScalar)?;
            after = Some(id.to_owned());
            if snapshot.attachments.is_some() {
                continue;
            }
            let Some(archive) = PromptAttachmentArchive::store(&self.disk, id, &snapshot.prompt)
                .await
                .map_err(|_| DatabaseError::InvalidQuery("AI 审批附件迁移写入失败"))?
            else {
                continue;
            };
            if archive.verify(id, &snapshot.prompt).await.is_err() {
                archive.discard().await;
                return Err(DatabaseError::InvalidQuery("AI 审批附件迁移校验失败"));
            }
            snapshot.attachments = Some(archive.clone());
            let encoded = match encode(&snapshot) {
                Ok(encoded) => encoded,
                Err(error) => {
                    archive.discard().await;
                    return Err(error);
                }
            };
            let revision = integer(&row, "revision")?;
            match self
                .connection
                .table(TABLE)
                .r#where("id", "=", id)
                .r#where("state", "=", "pending")
                .r#where("revision", "=", revision)
                .update(Record::from([
                    ("snapshot".into(), encoded.into()),
                    ("revision".into(), revision.saturating_add(1).into()),
                ]))
                .await
            {
                Ok(1) => migrated += 1,
                Ok(_) => archive.discard().await,
                // 数据库结果未知时保留可能被引用的文件。
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) async fn claim(
        &self,
        scope: &ApprovalScope<'_>,
        id: &str,
        revision: i64,
    ) -> DatabaseResult<Option<String>> {
        let stamp = now();
        let claim_id = Str::ulid().to_string();
        let written = self
            .query(scope, id)
            .r#where("state", "=", "pending")
            .r#where("revision", "=", revision)
            .r#where("expires_at", ">", stamp)
            .update(Record::from([
                ("state".into(), "running".into()),
                ("claim_id".into(), claim_id.as_str().into()),
                ("updated_at".into(), stamp.into()),
            ]))
            .await?;
        Ok((written == 1).then_some(claim_id))
    }

    pub(crate) async fn store_approval_results(
        &self,
        scope: &ApprovalScope<'_>,
        id: &str,
        claim: Option<&str>,
        results: &[ToolResult],
        edits: Option<&BTreeMap<String, Map<String, Json>>>,
    ) -> AiResult<i64> {
        let row = self
            .query(scope, id)
            .select(["state", "claim_id", "snapshot", "revision"])
            .first()
            .await
            .map_err(storage_error)?
            .ok_or(AiError::ApprovalNotFound)?;
        if text(&row, "state").map_err(storage_error)? != "running" {
            return Err(AiError::ApprovalConsumed);
        }
        let current_claim = text(&row, "claim_id").map_err(storage_error)?;
        if claim.is_some_and(|claim| claim != current_claim) {
            return Err(AiError::ApprovalConsumed);
        }
        let revision = integer(&row, "revision").map_err(storage_error)?;
        // 在原始快照上追加结果，保留旧记录中的附件直到正常归档或结算。
        let mut snapshot = snapshot_json(&row)?;
        let results_json = serde_json::json!(results);
        let edits_json = edits.map(|edits| serde_json::json!(edits));
        if let Some(recorded) = snapshot.get("resolved_results") {
            if recorded != &results_json {
                return Err(AiError::InvalidApprovalDecisions);
            }
            match (edits_json.as_ref(), snapshot.get("edited_arguments")) {
                (None, _) => return Ok(revision),
                (Some(edits), Some(recorded)) if edits == recorded => return Ok(revision),
                (Some(_), Some(_)) => return Err(AiError::InvalidApprovalDecisions),
                (Some(_), None) => {}
            }
        }
        let calls = snapshot
            .pointer("/response/tool_calls")
            .and_then(Json::as_array)
            .ok_or(AiError::ConversationStorageUnavailable)?;
        if calls.len() != results.len()
            || calls.iter().zip(results).any(|(call, result)| {
                call.get("id").and_then(Json::as_str) != Some(result.id.as_str())
                    || call.get("name").and_then(Json::as_str) != Some(result.name.as_str())
            })
            || edits.is_some_and(|edits| {
                edits
                    .keys()
                    .any(|id| !results.iter().any(|result| &result.id == id))
            })
        {
            return Err(AiError::InvalidApprovalDecisions);
        }
        let next = revision
            .checked_add(1)
            .ok_or(AiError::ConversationStorageUnavailable)?;
        snapshot["resolved_results"] = results_json;
        if let Some(edits) = edits_json {
            snapshot["edited_arguments"] = edits;
        }
        let fields = Record::from([
            ("snapshot".into(), snapshot.to_string().into()),
            ("revision".into(), next.into()),
            ("updated_at".into(), now().into()),
        ]);
        if !replace_claim(
            self.query(scope, id).r#where("revision", "=", revision),
            current_claim,
            fields,
        )
        .await
        .map_err(storage_error)?
        {
            return Err(AiError::ApprovalConsumed);
        }
        Ok(next)
    }

    pub(crate) async fn approval_results(
        &self,
        scope: &ApprovalScope<'_>,
        id: &str,
    ) -> AiResult<Option<Vec<ToolResult>>> {
        let row = self
            .query(scope, id)
            .select(["state", "snapshot"])
            .first()
            .await
            .map_err(storage_error)?
            .ok_or(AiError::ApprovalNotFound)?;
        if text(&row, "state").map_err(storage_error)? != "running" {
            return Ok(None);
        }
        snapshot_json(&row)?
            .as_object_mut()
            .and_then(|snapshot| snapshot.remove("resolved_results"))
            .map(|results| {
                serde_json::from_value(results).map_err(|_| AiError::ConversationStorageUnavailable)
            })
            .transpose()
    }

    pub(crate) async fn settle<T: Serialize>(
        &self,
        scope: &ApprovalScope<'_>,
        id: &str,
        claim: &str,
        next: Option<(i64, &T)>,
    ) -> DatabaseResult<bool> {
        let mut fields = settled_fields();
        if let Some((revision, snapshot)) = next {
            fields.insert("state".into(), "pending".into());
            fields.insert("revision".into(), revision.saturating_add(1).into());
            fields.insert("snapshot".into(), encode(snapshot)?.into());
        }
        replace_claim(self.query(scope, id), claim, fields).await
    }

    pub(crate) async fn complete(
        &self,
        transaction: &mut Transaction,
        scope: &ApprovalScope<'_>,
        id: &str,
        claim: &str,
    ) -> DatabaseResult<bool> {
        if !transaction.belongs_to(&self.connection) {
            return Err(DatabaseError::InvalidQuery(
                "AI 工具审批与会话须使用同一个数据库连接",
            ));
        }
        replace_claim(
            scoped(transaction.table(TABLE), scope, id),
            claim,
            settled_fields(),
        )
        .await
    }
}

fn scoped<'a>(query: Builder<'a>, scope: &ApprovalScope<'_>, id: &str) -> Builder<'a> {
    query
        .r#where("id", "=", id)
        .r#where("conversation_id", "=", scope.conversation_id)
        .r#where("participant_key", "=", scope.participant.key(scope.agent))
}

fn settled_fields() -> Record {
    Record::from([
        ("claim_id".into(), Value::Null),
        ("state".into(), "settled".into()),
        ("snapshot".into(), "".into()),
        ("updated_at".into(), now().into()),
    ])
}

async fn replace_claim(query: Builder<'_>, claim: &str, fields: Record) -> DatabaseResult<bool> {
    Ok(query
        .r#where("state", "=", "running")
        .r#where("claim_id", "=", claim)
        .update(fields)
        .await?
        == 1)
}

fn encode(snapshot: &impl Serialize) -> DatabaseResult<String> {
    serde_json::to_string(snapshot).map_err(|_| DatabaseError::InvalidScalar)
}

fn storage_error(_: DatabaseError) -> AiError {
    AiError::ConversationStorageUnavailable
}

fn snapshot_json(row: &Row) -> AiResult<Json> {
    serde_json::from_str(text(row, "snapshot").map_err(storage_error)?)
        .map_err(|_| AiError::ConversationStorageUnavailable)
}

fn text<'a>(row: &'a Row, field: &str) -> DatabaseResult<&'a str> {
    match row.get(field) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(DatabaseError::InvalidScalar),
    }
}

fn integer(row: &Row, field: &str) -> DatabaseResult<i64> {
    match row.get(field) {
        Some(Value::Integer(value)) => Ok(*value),
        _ => Err(DatabaseError::InvalidScalar),
    }
}
