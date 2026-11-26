use std::time::SystemTime;

use openrust::{
    database::{Connection, DatabaseError, DatabaseResult, Row, Value, query::Record},
    pagination::{Cursor, CursorOptions},
    support::Str,
};
use serde::Serialize;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use crate::{AgentResponse, Message, MessageRole};

const CONVERSATIONS: &str = "agent_conversations";
const MESSAGES: &str = "agent_conversation_messages";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationParticipant {
    kind: String,
    id: String,
}

impl ConversationParticipant {
    pub fn new(kind: impl Into<String>, id: impl ToString) -> Self {
        Self {
            kind: kind.into(),
            id: id.to_string(),
        }
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn key(&self, agent: &str) -> String {
        let mut digest = Sha256::new();
        for part in [&self.kind, &self.id, agent] {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part.as_bytes());
        }
        format!("{:x}", digest.finalize())
    }

    pub(crate) fn validate(&self) -> DatabaseResult<()> {
        if valid_field(&self.kind) && valid_field(&self.id) {
            Ok(())
        } else {
            Err(DatabaseError::InvalidQuery("AI 会话参与者类型或 ID 无效"))
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StoredMessage {
    pub id: String,
    pub agent: String,
    pub message: Message,
    pub usage: Json,
    pub meta: Json,
    pub steps: Json,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConversationPage {
    pub messages: Vec<StoredMessage>,
    pub next_cursor: Option<String>,
    pub previous_cursor: Option<String>,
}

pub struct DatabaseConversationStore {
    connection: Connection,
}

impl DatabaseConversationStore {
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub async fn conversation_belongs_to(
        &self,
        conversation: &str,
        participant: &ConversationParticipant,
    ) -> DatabaseResult<bool> {
        participant.validate()?;
        let row = self
            .connection
            .table(CONVERSATIONS)
            .use_write_pdo()
            .select(["participant_type", "participant_id"])
            .r#where("id", "=", conversation)
            .first()
            .await?;
        row.map(|row| owns(&row, participant))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub async fn latest_conversation_id(
        &self,
        participant: &ConversationParticipant,
        agent: &str,
    ) -> DatabaseResult<Option<String>> {
        participant.validate()?;
        let key = participant.key(agent);
        self.connection
            .table(MESSAGES)
            .use_write_pdo()
            .select(["conversation_id"])
            .r#where("participant_key", "=", key)
            .order_by_desc("id")
            .first()
            .await?
            .map(|row| text(&row, "conversation_id").map(str::to_owned))
            .transpose()
    }

    pub async fn get_latest_conversation_messages(
        &self,
        conversation: &str,
        participant: &ConversationParticipant,
        limit: u64,
    ) -> DatabaseResult<Option<Vec<Message>>> {
        if !self
            .conversation_belongs_to(conversation, participant)
            .await?
        {
            return Ok(None);
        }
        if limit == 0 {
            return Ok(Some(Vec::new()));
        }
        let rows = self
            .connection
            .table(MESSAGES)
            .use_write_pdo()
            .select(["role", "content"])
            .r#where("conversation_id", "=", conversation)
            .order_by_desc("id")
            .limit(limit)
            .get()
            .await?;
        rows.iter()
            .rev()
            .map(message)
            .collect::<DatabaseResult<_>>()
            .map(Some)
    }

    pub async fn paginate_conversation_messages(
        &self,
        conversation: &str,
        participant: &ConversationParticipant,
        per_page: u64,
        cursor: Option<&Cursor>,
    ) -> DatabaseResult<Option<ConversationPage>> {
        if !self
            .conversation_belongs_to(conversation, participant)
            .await?
        {
            return Ok(None);
        }
        let page = self
            .connection
            .table(MESSAGES)
            .use_write_pdo()
            .r#where("conversation_id", "=", conversation)
            .order_by_desc("id")
            .cursor_paginate_with(CursorOptions {
                per_page: Some(per_page),
                columns: &[
                    "id",
                    "agent",
                    "role",
                    "content",
                    "steps",
                    "usage",
                    "meta",
                    "created_at",
                ],
                cursor,
                ..CursorOptions::default()
            })
            .await?;
        let messages = page
            .items()
            .iter()
            .map(stored_message)
            .collect::<DatabaseResult<_>>()?;
        Ok(Some(ConversationPage {
            messages,
            next_cursor: page.next_cursor().map(Cursor::encode).transpose()?,
            previous_cursor: page.previous_cursor().map(Cursor::encode).transpose()?,
        }))
    }

    pub(crate) async fn store_turn(
        &self,
        existing: Option<&str>,
        participant: &ConversationParticipant,
        agent: &str,
        prompt: &str,
        response: &AgentResponse,
    ) -> DatabaseResult<Option<String>> {
        participant.validate()?;
        if !valid_field(agent) {
            return Err(DatabaseError::InvalidQuery("AI 会话 Agent 名称无效"));
        }
        let id = existing.map_or_else(uuid, str::to_owned);
        let user_id = uuid();
        let assistant_id = uuid();
        let timestamp = now();
        let rows = [
            message_record(
                &user_id,
                &id,
                participant,
                agent,
                Message::user(prompt),
                timestamp,
            ),
            message_record(
                &assistant_id,
                &id,
                participant,
                agent,
                Message::assistant(&response.text),
                timestamp,
            ),
        ];
        let mut rows = rows;
        rows[1].extend([
            ("usage".into(), json!(response.usage).to_string().into()),
            (
                "meta".into(),
                json!({"provider": response.provider, "model": response.model})
                    .to_string()
                    .into(),
            ),
            (
                "steps".into(),
                json!([{"tool_calls": response.tool_calls, "tool_results": response.tool_results}])
                    .to_string()
                    .into(),
            ),
        ]);
        let mut transaction = self.connection.begin_transaction().await?;
        if existing.is_some() {
            let Some(row) = transaction
                .table(CONVERSATIONS)
                .select(["participant_type", "participant_id"])
                .r#where("id", "=", id.as_str())
                .first()
                .await?
            else {
                transaction.roll_back(None).await?;
                return Ok(None);
            };
            if !owns(&row, participant)? {
                transaction.roll_back(None).await?;
                return Ok(None);
            }
            transaction
                .table(CONVERSATIONS)
                .r#where("id", "=", id.as_str())
                .update(Record::from([("updated_at".into(), timestamp.into())]))
                .await?;
        } else {
            let title: String = prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(80)
                .collect();
            if !transaction
                .table(CONVERSATIONS)
                .insert(&[Record::from([
                    ("id".into(), id.as_str().into()),
                    ("participant_type".into(), participant.kind().into()),
                    ("participant_id".into(), participant.id().into()),
                    ("title".into(), title.into()),
                    ("created_at".into(), timestamp.into()),
                    ("updated_at".into(), timestamp.into()),
                ])])
                .await?
            {
                return Err(DatabaseError::InvalidQuery("AI 会话未保存"));
            }
        }
        if !transaction.table(MESSAGES).insert(&rows).await? {
            return Err(DatabaseError::InvalidQuery("AI 会话消息未保存"));
        }
        transaction.commit().await?;
        Ok(Some(id))
    }
}

fn message_record(
    id: &str,
    conversation: &str,
    participant: &ConversationParticipant,
    agent: &str,
    message: Message,
    timestamp: i64,
) -> Record {
    Record::from([
        ("id".into(), id.into()),
        ("conversation_id".into(), conversation.into()),
        ("participant_type".into(), participant.kind().into()),
        ("participant_id".into(), participant.id().into()),
        ("participant_key".into(), participant.key(agent).into()),
        ("agent".into(), agent.into()),
        ("role".into(), message.role.as_str().into()),
        ("content".into(), message.content.into()),
        ("attachments".into(), "[]".into()),
        ("steps".into(), "[]".into()),
        ("meta".into(), "{}".into()),
        ("usage".into(), "{}".into()),
        ("status".into(), "completed".into()),
        ("created_at".into(), timestamp.into()),
        ("updated_at".into(), timestamp.into()),
    ])
}

fn owns(row: &Row, participant: &ConversationParticipant) -> DatabaseResult<bool> {
    Ok(text(row, "participant_type")? == participant.kind()
        && text(row, "participant_id")? == participant.id())
}

fn message(row: &Row) -> DatabaseResult<Message> {
    let role = match text(row, "role")? {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        _ => return Err(DatabaseError::InvalidScalar),
    };
    Ok(Message::new(role, text(row, "content")?))
}

fn stored_message(row: &Row) -> DatabaseResult<StoredMessage> {
    let json =
        |key| serde_json::from_str(text(row, key)?).map_err(|_| DatabaseError::InvalidScalar);
    let Some(Value::Integer(created_at)) = row.get("created_at") else {
        return Err(DatabaseError::InvalidScalar);
    };
    Ok(StoredMessage {
        id: text(row, "id")?.into(),
        agent: text(row, "agent")?.into(),
        message: message(row)?,
        usage: json("usage")?,
        meta: json("meta")?,
        steps: json("steps")?,
        created_at: *created_at,
    })
}

fn text<'a>(row: &'a Row, field: &str) -> DatabaseResult<&'a str> {
    match row.get(field) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(DatabaseError::InvalidScalar),
    }
}

fn valid_field(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 255
}

fn uuid() -> String {
    Str::uuid7().to_string()
}

fn now() -> i64 {
    SystemTime::UNIX_EPOCH
        .elapsed()
        .map(|value| i64::try_from(value.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
