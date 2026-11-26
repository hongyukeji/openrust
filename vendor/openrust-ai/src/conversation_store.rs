use std::{any::Any, time::SystemTime};

use futures_util::TryStreamExt;

use openrust::{
    contracts::config::Repository,
    database::{Connection, DatabaseError, DatabaseResult, Row, Transaction, Value, query::Record},
    http::HttpFuture,
    pagination::{Cursor, CursorOptions},
    support::Str,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use crate::{
    AgentResponse, AiError, AiResult, Message, MessageRole, PendingApproval, ToolCall, ToolResult,
    approval_store::{ApprovalScope, paused_history},
    approval_turn::ApprovalSession,
    attachment_storage,
    attachments::AttachmentBudget,
    messages::PromptAttachmentArchive,
    provider_replay::ProviderReplay,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageStatus {
    Completed,
    Failed,
    Paused,
}

impl ConversationMessageStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Paused => "paused",
        }
    }
}

pub(crate) struct TurnPersistence<'a> {
    pub(crate) response: &'a AgentResponse,
    pub(crate) approval: Option<&'a ApprovalSession>,
    pub(crate) title: Option<&'a str>,
    pub(crate) failure: Option<&'a AiError>,
    pub(crate) pause: Option<PausePersistence<'a>>,
}

pub(crate) struct PausePersistence<'a> {
    pub(crate) id: &'a str,
    pub(crate) checkpoint: &'a mut crate::tools::ApprovalCheckpoint,
}

#[derive(Clone, Debug, Serialize)]
pub struct StoredMessage {
    pub id: String,
    pub agent: String,
    pub message: Message,
    pub status: ConversationMessageStatus,
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

    /// 创建有明确参与者的会话。省略 ID 时由存储生成。
    fn store_conversation<'a>(
        &'a self,
        _participant: &'a ConversationParticipant,
        _title: &'a str,
        _id: Option<&'a str>,
    ) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async { Err(AiError::ConversationStorageUnavailable) })
    }

    /// 向已有会话追加用户消息及其附件，返回消息 ID。
    fn store_user_message<'a>(
        &'a self,
        _conversation: &'a str,
        _participant: &'a ConversationParticipant,
        _agent: &'a str,
        _message: &'a Message,
    ) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async { Err(AiError::ConversationStorageUnavailable) })
    }

    /// 向已有会话追加助手响应。可同时保存终止该次生成的错误。
    fn store_assistant_message<'a>(
        &'a self,
        _conversation: &'a str,
        _participant: &'a ConversationParticipant,
        _agent: &'a str,
        _response: &'a AgentResponse,
        _failure: Option<&'a AiError>,
    ) -> HttpFuture<'a, AiResult<Option<String>>> {
        Box::pin(async { Err(AiError::ConversationStorageUnavailable) })
    }

    /// 持久化已领取审批的步骤结果，供可信应用端的工具执行流程调用。
    fn store_approval_results<'a>(
        &'a self,
        _conversation: &'a str,
        _participant: &'a ConversationParticipant,
        _agent: &'a str,
        _approval: &'a str,
        results: &'a [ToolResult],
    ) -> HttpFuture<'a, AiResult<()>> {
        Box::pin(async move {
            if results.is_empty() {
                Ok(())
            } else {
                Err(AiError::ConversationStorageUnavailable)
            }
        })
    }

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

    fn supports_attachments(&self) -> bool {
        false
    }

    /// 保存用户附件以及本轮其余消息。仅处理文本的后端可沿用 `store_turn`。
    fn store_turn_with_message<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        agent: &'a str,
        existing: bool,
        message: &'a Message,
        response: &'a AgentResponse,
    ) -> HttpFuture<'a, AiResult<bool>> {
        if !message.attachments.is_empty() || message.role != MessageRole::User {
            return Box::pin(async { Err(AiError::ConversationStorageUnavailable) });
        }
        self.store_turn(
            conversation,
            participant,
            agent,
            existing,
            &message.content,
            response,
        )
    }
}

impl dyn ConversationStore {
    pub(crate) fn as_database(&self) -> Option<&DatabaseConversationStore> {
        (self as &dyn Any).downcast_ref()
    }
}

#[derive(Clone)]
pub struct DatabaseConversationStore {
    connection: Connection,
    conversations: String,
    messages: String,
    attachment_disk: String,
    generate_title: bool,
}

impl DatabaseConversationStore {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            conversations: CONVERSATIONS.to_owned(),
            messages: MESSAGES.to_owned(),
            attachment_disk: attachment_storage::DEFAULT_DISK.to_owned(),
            generate_title: true,
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
        const DISK_KEY: &str = "ai.conversations.attachments.disk";
        let attachment_disk = match config.get(DISK_KEY) {
            None | Some(Json::Null) => attachment_storage::DEFAULT_DISK.to_owned(),
            Some(Json::String(disk)) if attachment_storage::valid_disk(&disk) => disk,
            _ => return Err(AiError::InvalidConfiguration(DISK_KEY)),
        };
        const TITLE_KEY: &str = "ai.conversations.generate_title";
        let generate_title = match config.get(TITLE_KEY) {
            None | Some(Json::Null) => true,
            Some(Json::Bool(value)) => value,
            _ => return Err(AiError::InvalidConfiguration(TITLE_KEY)),
        };
        Ok(Self {
            connection,
            conversations,
            messages,
            attachment_disk,
            generate_title,
        })
    }

    pub(crate) fn generates_titles(&self) -> bool {
        self.generate_title
    }

    pub(crate) fn approval_store(&self) -> crate::approval_store::ApprovalStore {
        crate::approval_store::ApprovalStore::new(
            self.connection.clone(),
            self.attachment_disk.clone(),
            self.messages.clone(),
        )
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
        let mut rows = self
            .connection
            .table(&self.messages)
            .use_write_pdo()
            .select(["id", "role", "content", "attachments", "steps", "status"])
            .r#where("conversation_id", "=", conversation)
            .order_by_desc("id")
            .limit(limit)
            .cursor();
        let mut turns = Vec::new();
        let mut budget = AttachmentBudget::default();
        while let Some(row) = rows.try_next().await? {
            let message = message(&row).await?;
            budget
                .include(&message.attachments)
                .map_err(|_| DatabaseError::InvalidQuery("AI 会话历史附件超限"))?;
            turns.push(if message.role == MessageRole::Assistant {
                replay_steps(
                    message,
                    text(&row, "steps")?,
                    text(&row, "status")? == "paused",
                )?
            } else {
                vec![message]
            });
        }
        Ok(Some(turns.into_iter().rev().flatten().collect()))
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
                    "attachments",
                    "steps",
                    "usage",
                    "meta",
                    "status",
                    "created_at",
                ],
                cursor,
                ..CursorOptions::default()
            })
            .await?;
        let mut messages = Vec::with_capacity(page.items().len());
        for row in page.items() {
            messages.push(stored_message(row).await?);
        }
        Ok(Some(ConversationPage {
            messages,
            next_cursor: page.next_cursor().map(Cursor::encode).transpose()?,
            previous_cursor: page.previous_cursor().map(Cursor::encode).transpose()?,
        }))
    }

    pub(crate) async fn migrate_attachments(&self) -> DatabaseResult<u64> {
        let mut migrated = 0;
        let mut after = None::<String>;
        loop {
            let mut query = self
                .connection
                .table(&self.messages)
                .use_write_pdo()
                .select(["id", "attachments"])
                .r#where("role", "=", "user")
                .where_like("attachments", "%base64-%", true)
                .order_by("id", "asc");
            if let Some(after) = &after {
                query = query.r#where("id", ">", after.as_str());
            }
            let Some(row) = query.first().await? else {
                return Ok(migrated);
            };
            let id = text(&row, "id")?;
            let legacy = text(&row, "attachments")?;
            after = Some(id.to_owned());
            if !attachment_storage::is_inline(legacy).map_err(attachment_error)? {
                continue;
            }
            let attachments = attachment_storage::read(id, legacy)
                .await
                .map_err(attachment_error)?;
            let files = attachment_storage::write(&self.attachment_disk, id, &attachments)
                .await
                .map_err(attachment_error)?;
            let references = match files.json() {
                Ok(references) => references,
                Err(error) => {
                    files.discard().await;
                    return Err(attachment_error(error));
                }
            };
            if !matches!(attachment_storage::read(id, &references).await, Ok(verified) if verified == attachments)
            {
                files.discard().await;
                return Err(DatabaseError::InvalidQuery("AI 会话附件迁移校验失败"));
            }
            // 更新条件保证两个迁移进程不会互相覆盖引用。
            match self
                .connection
                .table(&self.messages)
                .r#where("id", "=", id)
                .r#where("attachments", "=", legacy)
                .update(Record::from([("attachments".into(), references.into())]))
                .await
            {
                Ok(1) => migrated += 1,
                Ok(_) => files.discard().await,
                // 驱动未确认结果时保留文件，避免删除已更新的引用。
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) async fn store_turn_with_approval(
        &self,
        scope: ApprovalScope<'_>,
        existing: bool,
        user: &Message,
        outcome: TurnPersistence<'_>,
    ) -> DatabaseResult<bool> {
        let TurnPersistence {
            response,
            approval,
            title,
            failure,
            mut pause,
        } = outcome;
        let ApprovalScope {
            conversation_id: id,
            participant,
            agent,
        } = scope;
        validate_message_scope(&scope)?;
        if user.role != MessageRole::User {
            return Err(DatabaseError::InvalidQuery("AI 会话首条消息角色无效"));
        }
        let user_id = uuid();
        let previous_message = approval.and_then(|session| session.history_message_id.as_deref());
        let assistant_id = previous_message.map(str::to_owned).unwrap_or_else(uuid);
        let mut transaction = self.connection.begin_transaction().await?;
        let files = if previous_message.is_none() {
            Some(
                attachment_storage::write(&self.attachment_disk, &user_id, &user.attachments)
                    .await
                    .map_err(|_| DatabaseError::InvalidQuery("AI 会话附件写入失败"))?,
            )
        } else {
            None
        };
        let mut created_archive = false;
        let write = async {
            let timestamp = now();
            if existing || previous_message.is_some() {
                if !self
                    .touch_owned_conversation(&mut transaction, &scope, timestamp)
                    .await?
                {
                    return Ok(false);
                }
                if let Some(title) = title {
                    transaction
                        .table(&self.conversations)
                        .r#where("id", "=", id)
                        .update(Record::from([("title".into(), title.into())]))
                        .await?;
                }
            } else {
                let default_title;
                let title = match title {
                    Some(value) => value,
                    None => {
                        default_title = Str::limit(&user.content, 50, "...", true);
                        &default_title
                    }
                };
                if !transaction
                    .table(&self.conversations)
                    .insert(&[conversation_record(id, participant, title, timestamp)])
                    .await?
                {
                    return Err(DatabaseError::InvalidQuery("AI 会话未保存"));
                }
            }
            let mut assistant = assistant_record(
                &assistant_id,
                &scope,
                response,
                failure,
                timestamp,
                pause
                    .as_ref()
                    .map(|pause| pause.checkpoint.pending.as_slice()),
            );
            if let Some(files) = &files {
                let attachments = files.json().map_err(|_| DatabaseError::InvalidScalar)?;
                let user = message_record(
                    &user_id,
                    id,
                    participant,
                    agent,
                    user,
                    attachments,
                    timestamp,
                );
                if !transaction
                    .table(&self.messages)
                    .insert(&[user, assistant])
                    .await?
                {
                    return Err(DatabaseError::InvalidQuery("AI 会话消息未保存"));
                }
            } else {
                for column in [
                    "id",
                    "conversation_id",
                    "participant_type",
                    "participant_id",
                    "participant_key",
                    "agent",
                    "role",
                    "created_at",
                    "attachments",
                ] {
                    assistant.remove(column);
                }
                if paused_history(transaction.table(&self.messages), &scope, &assistant_id)
                    .update(assistant)
                    .await?
                    != 1
                {
                    return Ok(false);
                }
            }
            if let Some(approval) = approval {
                let transitioned = match &mut pause {
                    Some(pause) => {
                        pause.checkpoint.history_message_id = Some(assistant_id);
                        if pause.checkpoint.attachments.is_none() {
                            pause.checkpoint.attachments = PromptAttachmentArchive::store(
                                &self.attachment_disk,
                                pause.id,
                                &pause.checkpoint.prompt,
                            )
                            .await
                            .map_err(|_| DatabaseError::InvalidQuery("AI 工具审批附件写入失败"))?;
                            created_archive = pause.checkpoint.attachments.is_some();
                        }
                        approval
                            .persist_pause(&mut transaction, pause.id, pause.checkpoint)
                            .await?
                    }
                    None => approval.complete(&mut transaction).await?,
                };
                if !transitioned {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        .await;
        match write {
            Ok(true) => {
                // 提交结果不明时保留文件，避免已提交的引用失效。
                transaction.commit().await?;
                if pause.is_none()
                    && let Some(approval) = approval
                {
                    approval.discard_archived_attachments().await;
                }
                Ok(true)
            }
            result => {
                let rollback = transaction.roll_back(None).await;
                if let Some(files) = files {
                    files.discard().await;
                }
                if created_archive
                    && let Some(archive) = pause
                        .as_ref()
                        .and_then(|pause| pause.checkpoint.attachments.as_ref())
                {
                    archive.discard().await;
                }
                rollback?;
                result
            }
        }
    }

    async fn touch_owned_conversation(
        &self,
        transaction: &mut Transaction,
        scope: &ApprovalScope<'_>,
        timestamp: i64,
    ) -> DatabaseResult<bool> {
        let Some(row) = transaction
            .table(&self.conversations)
            .select(["participant_type", "participant_id"])
            .r#where("id", "=", scope.conversation_id)
            .first()
            .await?
        else {
            return Ok(false);
        };
        if !owns(&row, scope.participant)? {
            return Ok(false);
        }
        transaction
            .table(&self.conversations)
            .r#where("id", "=", scope.conversation_id)
            .update(Record::from([("updated_at".into(), timestamp.into())]))
            .await?;
        Ok(true)
    }

    async fn begin_owned_message(
        &self,
        scope: &ApprovalScope<'_>,
        timestamp: i64,
    ) -> AiResult<Transaction> {
        validate_message_scope(scope).map_err(storage_error)?;
        let mut transaction = self
            .connection
            .begin_transaction()
            .await
            .map_err(storage_error)?;
        if !self
            .touch_owned_conversation(&mut transaction, scope, timestamp)
            .await
            .map_err(storage_error)?
        {
            return Err(AiError::ConversationNotFound);
        }
        Ok(transaction)
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

    fn store_conversation<'a>(
        &'a self,
        participant: &'a ConversationParticipant,
        title: &'a str,
        id: Option<&'a str>,
    ) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            participant.validate().map_err(storage_error)?;
            let id = id.map(str::to_owned).unwrap_or_else(uuid);
            if id.trim().is_empty() || id.chars().count() > 36 || title.chars().count() > 255 {
                return Err(AiError::ConversationStorageUnavailable);
            }
            let record = conversation_record(&id, participant, title, now());
            if !self
                .connection
                .table(&self.conversations)
                .insert(&[record])
                .await
                .map_err(storage_error)?
            {
                return Err(AiError::ConversationStorageUnavailable);
            }
            Ok(id)
        })
    }

    fn store_user_message<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        agent: &'a str,
        message: &'a Message,
    ) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            if message.role != MessageRole::User {
                return Err(AiError::ConversationStorageUnavailable);
            }
            let scope = ApprovalScope {
                conversation_id: conversation,
                participant,
                agent,
            };
            let id = uuid();
            let timestamp = now();
            let mut transaction = self.begin_owned_message(&scope, timestamp).await?;
            let files =
                attachment_storage::write(&self.attachment_disk, &id, &message.attachments).await?;
            let write = async {
                let record = message_record(
                    &id,
                    conversation,
                    participant,
                    agent,
                    message,
                    files.json()?,
                    timestamp,
                );
                if !transaction
                    .table(&self.messages)
                    .insert(&[record])
                    .await
                    .map_err(storage_error)?
                {
                    return Err(AiError::ConversationStorageUnavailable);
                }
                Ok(())
            }
            .await;
            if let Err(error) = write {
                let rollback = transaction.roll_back(None).await;
                files.discard().await;
                rollback.map_err(storage_error)?;
                return Err(error);
            }
            // 提交状态不明时保留文件，避免删除已提交的引用。
            transaction.commit().await.map_err(storage_error)?;
            Ok(id)
        })
    }

    fn store_assistant_message<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        agent: &'a str,
        response: &'a AgentResponse,
        failure: Option<&'a AiError>,
    ) -> HttpFuture<'a, AiResult<Option<String>>> {
        Box::pin(async move {
            let scope = ApprovalScope {
                conversation_id: conversation,
                participant,
                agent,
            };
            let timestamp = now();
            let mut transaction = self.begin_owned_message(&scope, timestamp).await?;
            let id = uuid();
            let record = assistant_record(&id, &scope, response, failure, timestamp, None);
            if !transaction
                .table(&self.messages)
                .insert(&[record])
                .await
                .map_err(storage_error)?
            {
                return Err(AiError::ConversationStorageUnavailable);
            }
            transaction.commit().await.map_err(storage_error)?;
            Ok(Some(id))
        })
    }

    fn store_approval_results<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        agent: &'a str,
        approval: &'a str,
        results: &'a [ToolResult],
    ) -> HttpFuture<'a, AiResult<()>> {
        Box::pin(async move {
            if results.is_empty() {
                return Ok(());
            }
            let scope = ApprovalScope {
                conversation_id: conversation,
                participant,
                agent,
            };
            validate_message_scope(&scope).map_err(storage_error)?;
            self.approval_store()
                .store_approval_results(&scope, approval, None, results, None)
                .await?;
            Ok(())
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
            self.store_turn_with_message(
                conversation,
                participant,
                agent,
                existing,
                &Message::user(prompt),
                response,
            )
            .await
        })
    }

    fn supports_attachments(&self) -> bool {
        true
    }

    fn store_turn_with_message<'a>(
        &'a self,
        conversation: &'a str,
        participant: &'a ConversationParticipant,
        agent: &'a str,
        existing: bool,
        message: &'a Message,
        response: &'a AgentResponse,
    ) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            self.store_turn_with_approval(
                ApprovalScope {
                    conversation_id: conversation,
                    participant,
                    agent,
                },
                existing,
                message,
                TurnPersistence {
                    response,
                    approval: None,
                    title: None,
                    failure: None,
                    pause: None,
                },
            )
            .await
            .map_err(storage_error)
        })
    }
}

fn attachment_error(_: AiError) -> DatabaseError {
    DatabaseError::InvalidQuery("AI 会话附件迁移失败")
}

fn storage_error(_: DatabaseError) -> AiError {
    AiError::ConversationStorageUnavailable
}

fn validate_message_scope(scope: &ApprovalScope<'_>) -> DatabaseResult<()> {
    scope.participant.validate()?;
    if !valid_field(scope.agent) {
        return Err(DatabaseError::InvalidQuery("AI 会话 Agent 名称无效"));
    }
    Ok(())
}

fn conversation_record(
    id: &str,
    participant: &ConversationParticipant,
    title: &str,
    timestamp: i64,
) -> Record {
    Record::from([
        ("id".into(), id.into()),
        ("participant_type".into(), participant.kind().into()),
        ("participant_id".into(), participant.id().into()),
        ("title".into(), title.into()),
        ("created_at".into(), timestamp.into()),
        ("updated_at".into(), timestamp.into()),
    ])
}

fn assistant_record(
    id: &str,
    scope: &ApprovalScope<'_>,
    response: &AgentResponse,
    failure: Option<&AiError>,
    timestamp: i64,
    pending: Option<&[PendingApproval]>,
) -> Record {
    let mut record = message_record(
        id,
        scope.conversation_id,
        scope.participant,
        scope.agent,
        &Message::assistant(&response.text),
        "[]".into(),
        timestamp,
    );
    let mut steps = if response.steps.is_empty() {
        json!([{"tool_calls": response.tool_calls, "tool_results": response.tool_results}])
    } else {
        Json::Array(
            response
                .steps
                .iter()
                .map(|step| {
                    let mut data = json!(step);
                    if let Some(replay) = &step.provider_replay {
                        data["provider_replay"] = json!(replay);
                    }
                    data
                })
                .collect(),
        )
    };
    if let Some(pending) = pending
        && let Some(calls) = steps
            .as_array_mut()
            .and_then(|steps| steps.last_mut())
            .and_then(|step| step["tool_calls"].as_array_mut())
    {
        for call in calls {
            if let Some(approval) = pending.iter().find(|approval| call["id"] == approval.id) {
                call["approval_reason"] = json!(approval.reason);
            }
        }
    }
    let mut meta = json!(response.meta());
    let status = match failure {
        Some(error) => {
            meta["error"] = json!(error.to_string());
            ConversationMessageStatus::Failed
        }
        None if pending.is_some() => ConversationMessageStatus::Paused,
        None => ConversationMessageStatus::Completed,
    };
    record.extend([
        ("status".into(), status.as_str().into()),
        ("usage".into(), json!(response.usage).to_string().into()),
        ("meta".into(), meta.to_string().into()),
        ("steps".into(), steps.to_string().into()),
    ]);
    record
}

fn message_record(
    id: &str,
    conversation: &str,
    participant: &ConversationParticipant,
    agent: &str,
    message: &Message,
    attachments: String,
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
        ("content".into(), message.content.as_str().into()),
        ("attachments".into(), attachments.into()),
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

async fn message(row: &Row) -> DatabaseResult<Message> {
    let role = match text(row, "role")? {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        _ => return Err(DatabaseError::InvalidScalar),
    };
    let content = text(row, "content")?;
    if role == MessageRole::User {
        let attachments = attachment_storage::read(text(row, "id")?, text(row, "attachments")?)
            .await
            .map_err(|_| DatabaseError::InvalidScalar)?;
        Ok(Message::user_with_attachments(content, attachments))
    } else {
        Ok(Message::assistant(content))
    }
}

fn replay_steps(fallback: Message, steps: &str, paused: bool) -> DatabaseResult<Vec<Message>> {
    #[derive(Deserialize)]
    struct StoredStep {
        #[serde(default)]
        text: String,
        #[serde(default)]
        tool_calls: Vec<ToolCall>,
        #[serde(default)]
        tool_results: Vec<ToolResult>,
        #[serde(default)]
        provider_replay: Option<ProviderReplay>,
    }

    let steps: Vec<StoredStep> =
        serde_json::from_str(steps).map_err(|_| DatabaseError::InvalidScalar)?;
    if steps.iter().all(|step| step.tool_calls.is_empty()) {
        return Ok(vec![fallback]);
    }
    let has_step_text = steps.iter().any(|step| !step.text.is_empty());
    let mut messages = Vec::new();
    for step in steps {
        let expected_calls = step.tool_calls.len();
        let mut calls = Vec::new();
        let mut results = Vec::new();
        for call in step.tool_calls {
            if let Some(result) = step
                .tool_results
                .iter()
                .find(|result| result.id == call.id)
                .cloned()
                .or_else(|| {
                    paused.then(|| ToolResult {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        result_id: call.result_id.clone(),
                        arguments: call.arguments.clone(),
                        result: "会话继续前，此工具调用尚未获得批准".into(),
                        denied: true,
                        failed: false,
                    })
                })
            {
                results.push(result);
                calls.push(call);
            }
        }
        if !step.text.is_empty() || !calls.is_empty() {
            let replay = if !calls.is_empty() && calls.len() == expected_calls {
                step.provider_replay
            } else {
                None
            };
            let mut assistant = Message::assistant_with_tool_calls(step.text, calls);
            assistant.provider_replay = replay;
            messages.push(assistant);
        }
        if !results.is_empty() {
            messages.push(Message::tool_results(results));
        }
    }
    if messages.is_empty() || (!has_step_text && !fallback.content.is_empty()) {
        messages.push(fallback);
    }
    Ok(messages)
}

async fn stored_message(row: &Row) -> DatabaseResult<StoredMessage> {
    let json =
        |key| serde_json::from_str(text(row, key)?).map_err(|_| DatabaseError::InvalidScalar);
    let Some(Value::Integer(created_at)) = row.get("created_at") else {
        return Err(DatabaseError::InvalidScalar);
    };
    let mut steps: Json = json("steps")?;
    if let Some(steps) = steps.as_array_mut() {
        for step in steps.iter_mut().filter_map(Json::as_object_mut) {
            step.remove("provider_replay");
        }
    }
    let status = match text(row, "status")? {
        "completed" => ConversationMessageStatus::Completed,
        "failed" => ConversationMessageStatus::Failed,
        "paused" => ConversationMessageStatus::Paused,
        _ => return Err(DatabaseError::InvalidScalar),
    };
    Ok(StoredMessage {
        id: text(row, "id")?.into(),
        status,
        agent: text(row, "agent")?.into(),
        message: message(row).await?,
        usage: json("usage")?,
        meta: json("meta")?,
        steps,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_tool_history_replays_only_answered_calls_before_final_text() {
        let steps = json!([{
            "tool_calls":[
                {"id":"answered","name":"lookup","arguments":{"key":"a"}},
                {"id":"missing","name":"lookup","arguments":{"key":"b"}}
            ],
            "tool_results":[{"id":"answered","name":"lookup","result":"lookup failed","failed":true}]
        }]);
        let result = replay_steps(
            Message::assistant("Use another key"),
            &steps.to_string(),
            false,
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].tool_calls[0].id, "answered");
        assert_eq!(result[0].tool_calls.len(), 1);
        assert_eq!(result[1].role, MessageRole::ToolResult);
        assert!(result[1].tool_results[0].failed);
        assert_eq!(result[2], Message::assistant("Use another key"));
    }
}
