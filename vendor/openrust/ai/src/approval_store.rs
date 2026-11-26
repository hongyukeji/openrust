use openrust::{
    database::{
        Connection, DatabaseError, DatabaseResult, Row, Transaction, Value,
        query::{Builder, Record},
    },
    support::Str,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::ConversationParticipant;
use crate::conversation_store::now;

const TABLE: &str = "agent_tool_approvals";
const TTL_SECONDS: i64 = 86_400;

pub(crate) struct ApprovalStore {
    connection: Connection,
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
    pub(crate) fn new(connection: Connection) -> Self {
        Self { connection }
    }

    fn query(&self, scope: &ApprovalScope<'_>, id: &str) -> Builder<'static> {
        scoped(self.connection.table(TABLE).use_write_pdo(), scope, id)
    }

    pub(crate) async fn insert(
        &self,
        scope: &ApprovalScope<'_>,
        snapshot: &impl Serialize,
    ) -> DatabaseResult<String> {
        let id = Str::ulid().to_string();
        let timestamp = now();
        let row = Record::from([
            ("id".into(), id.as_str().into()),
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
        Ok(id)
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
