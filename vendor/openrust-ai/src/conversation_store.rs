use std::{any::Any, time::SystemTime};

use openrust::{
    contracts::config::Repository,
    database::{Connection, DatabaseError, DatabaseResult, Row, Value, query::Record},
    http::HttpFuture,
    pagination::{Cursor, CursorOptions},
    support::Str,
};
use serde::Serialize;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use crate::{
    AgentResponse, AiError, AiResult, Message, MessageRole, approval_store::ApprovalScope,
    approval_turn::ApprovalSession,
};

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

    pub(crate) fn key(&self, agent: &str) -> String {
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

/// 按参与者保存和读取 Agent 会话。历史读取在归属不符时返回 `None`。
pub trait ConversationStore: Any + Send + Sync {
    fn conversation_belongs_to<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
    ) -> HttpFuture<'a, AiResult<bool>>;

    fn latest_conversation_id<'a>(
        &'a self,
        participant: &'a ConversationParticipant,
        agent: &'a str,
    ) -> HttpFuture<'a, AiResult<Option<String>>>;

    fn get_latest_conversation_messages<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        limit: u64,
    ) -> HttpFuture<'a, AiResult<Option<Vec<Message>>>>;

    fn paginate_conversation_messages<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        per_page: u64,
        cursor: Option<&'a Cursor>,
    ) -> HttpFuture<'a, AiResult<Option<ConversationPage>>>;

    /// 原子保存用户和助手消息。已有会话归属不符时返回 `false`。
    fn store_turn<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        agent: &'a str,
        existing: bool,
        prompt: &'a str,
        response: &'a AgentResponse,
    ) -> HttpFuture<'a, AiResult<bool>>;
}

impl dyn ConversationStore {
    pub(crate) fn as_database(&self) -> Option<&DatabaseConversationStore> {
        (self as &dyn Any).downcast_ref()
    }
}

pub struct DatabaseConversationStore {
    connection: Connection,
    conversations: String,
    messages: String,
}

impl DatabaseConversationStore {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            conversations: CONVERSATIONS.to_owned(),
            messages: MESSAGES.to_owned(),
        }
    }

    pub(crate) fn from_config(connection: Connection, config: &dyn Repository) -> AiResult<Self> {
        let conversations = configured_table(
            config,
            "ai.conversations.tables.conversations",
            CONVERSATIONS,
        )?;
        let messages = configured_table(config, "ai.conversations.tables.messages", MESSAGES)?;
        if conversations.eq_ignore_ascii_case(&messages) {
            return Err(AiError::InvalidConfiguration(
                "ai.conversations.tables.messages",
            ));
        }
        Ok(Self {
            connection,
            conversations,
            messages,
        })
    }

    pub(crate) fn approval_store(&self) -> crate::approval_store::ApprovalStore {
        crate::approval_store::ApprovalStore::new(self.connection.clone())
    }

    pub async fn conversation_belongs_to(
        &self,
        conversation: &str,
        participant: &ConversationParticipant,
    ) -> DatabaseResult<bool> {
        participant.validate()?;
        let row = self
            .connection
            .table(&self.conversations)
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
            .table(&self.messages)
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
            .table(&self.messages)
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
            .table(&self.messages)
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
        scope: ApprovalScope<'_>,
        existing: bool,
        prompt: &str,
        response: &AgentResponse,
        approval: Option<&ApprovalSession>,
    ) -> DatabaseResult<bool> {
        let ApprovalScope {
            conversation_id: id,
            participant,
            agent,
        } = scope;
        participant.validate()?;
        if !valid_field(agent) {
            return Err(DatabaseError::InvalidQuery("AI 会话 Agent 名称无效"));
        }
        let user_id = uuid();
        let assistant_id = uuid();
        let timestamp = now();
        let rows = [
            message_record(
                &user_id,
                id,
                participant,
                agent,
                Message::user(prompt),
                timestamp,
            ),
            message_record(
                &assistant_id,
                id,
                participant,
                agent,
                Message::assistant(&response.text),
                timestamp,
            ),
        ];
        let mut rows = rows;
        let steps = if response.steps.is_empty() {
            json!([{"tool_calls": response.tool_calls, "tool_results": response.tool_results}])
        } else {
            json!(response.steps)
        };
        rows[1].extend([
            ("usage".into(), json!(response.usage).to_string().into()),
            ("meta".into(), json!(response.meta()).to_string().into()),
            ("steps".into(), steps.to_string().into()),
        ]);
        let mut transaction = self.connection.begin_transaction().await?;
        if existing {
            let Some(row) = transaction
                .table(&self.conversations)
                .select(["participant_type", "participant_id"])
                .r#where("id", "=", id)
                .first()
                .await?
            else {
                transaction.roll_back(None).await?;
                return Ok(false);
            };
            if !owns(&row, participant)? {
                transaction.roll_back(None).await?;
                return Ok(false);
            }
            transaction
                .table(&self.conversations)
                .r#where("id", "=", id)
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
                .table(&self.conversations)
                .insert(&[Record::from([
                    ("id".into(), id.into()),
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
        if !transaction.table(&self.messages).insert(&rows).await? {
            return Err(DatabaseError::InvalidQuery("AI 会话消息未保存"));
        }
        if let Some(approval) = approval
            && !approval.complete(&mut transaction).await?
        {
            transaction.roll_back(None).await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }
}

impl ConversationStore for DatabaseConversationStore {
    fn conversation_belongs_to<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
    ) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            self.conversation_belongs_to(conversation, participant)
                .await
                .map_err(storage_error)
        })
    }

    fn latest_conversation_id<'a>(
        &'a self,
        participant: &'a ConversationParticipant,
        agent: &'a str,
    ) -> HttpFuture<'a, AiResult<Option<String>>> {
        Box::pin(async move {
            self.latest_conversation_id(participant, agent)
                .await
                .map_err(storage_error)
        })
    }

    fn get_latest_conversation_messages<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        limit: u64,
    ) -> HttpFuture<'a, AiResult<Option<Vec<Message>>>> {
        Box::pin(async move {
            self.get_latest_conversation_messages(conversation, participant, limit)
                .await
                .map_err(storage_error)
        })
    }

    fn paginate_conversation_messages<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        per_page: u64,
        cursor: Option<&'a Cursor>,
    ) -> HttpFuture<'a, AiResult<Option<ConversationPage>>> {
        Box::pin(async move {
            self.paginate_conversation_messages(conversation, participant, per_page, cursor)
                .await
                .map_err(storage_error)
        })
    }

    fn store_turn<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        agent: &'a str,
        existing: bool,
        prompt: &'a str,
        response: &'a AgentResponse,
    ) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            self.store_turn(
                ApprovalScope {
                    conversation_id: conversation,
                    participant,
                    agent,
                },
                existing,
                prompt,
                response,
                None,
            )
            .await
            .map_err(storage_error)
        })
    }
}

fn storage_error(_: DatabaseError) -> AiError {
    AiError::ConversationStorageUnavailable
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

pub(crate) fn now() -> i64 {
    SystemTime::UNIX_EPOCH
        .elapsed()
        .map(|value| i64::try_from(value.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn configured_table(config: &dyn Repository, key: &'static str, default: &str) -> AiResult<String> {
    match config.get(key) {
        None | Some(Json::Null) => Ok(default.to_owned()),
        Some(Json::String(name)) if valid_table_name(&name) => Ok(name),
        _ => Err(AiError::InvalidConfiguration(key)),
    }
}

fn valid_table_name(name: &str) -> bool {
    let (schema, table) = name.rsplit_once('.').unwrap_or(("", name));
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    };
    valid_segment(table) && (!name.contains('.') || valid_segment(schema))
}
