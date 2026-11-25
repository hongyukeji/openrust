use std::{fmt::Write as _, sync::Arc};

use openrust::{
    database::{Connection, DatabaseError, Row, Transaction, Value, query::Builder},
    support::Str,
};
use sha2::{Digest as _, Sha256};

use crate::{
    ModerationError, ModerationResult, ReviewState, StoredMedia, TaskId, VideoReview,
    VideoReviewRequest, VideoSubmission, credentials::valid_identifier,
};

mod assessment_record;
mod cancellation;
pub(crate) use cancellation::Cancellation;
#[cfg(feature = "webhooks")]
mod callback;
mod decisions;
mod groups;
#[cfg(feature = "webhooks")]
pub(crate) use callback::ProviderTask;
mod query;
mod recovery;
mod submission;

const REVIEWS: &str = "moderation_reviews";
const CHECKS: &str = "moderation_checks";

/// 内容审核的数据库适配器。共享数据库中的应用与租户分别使用稳定标识。
#[derive(Clone)]
pub struct ModerationStore {
    connection: Connection,
    scope_key: String,
}

impl ModerationStore {
    pub fn new(connection: Connection, application: &str, tenant: &str) -> ModerationResult<Self> {
        if !valid_identifier(application) || !valid_identifier(tenant) {
            return Err(ModerationError::InvalidConfiguration("review scope"));
        }
        Ok(Self {
            connection,
            scope_key: digest(&format!("{application}\n{tenant}")),
        })
    }

    /// 在同一短事务写入父审核和子检查，不联系供应商或派发队列。
    pub async fn stage_video(&self, request: &VideoReviewRequest) -> ModerationResult<VideoReview> {
        let intent_key = digest(&request.intent);
        if let Some(review) = self.read("intent_key", &intent_key).await? {
            return existing(review, request);
        }
        let ids = ReviewIds::generate()?;
        let id = ids.review.clone();
        let record = Arc::new(request.clone());
        let scope_key = self.scope_key.clone();
        let saved_intent = intent_key.clone();
        let clock = database_clock_query(&self.connection);
        let transaction = self
            .connection
            .transaction(
                move |transaction| {
                    let request = record.clone();
                    let scope_key = scope_key.clone();
                    let intent_key = saved_intent.clone();
                    let ids = ids.clone();
                    let clock = clock.clone();
                    Box::pin(async move {
                        let created_at = read_clock(transaction, &clock).await?;
                        write_check(
                            transaction,
                            &ids,
                            &scope_key,
                            &intent_key,
                            &request,
                            created_at,
                        )
                        .await
                    })
                },
                3,
            )
            .await;
        if transaction.is_ok() {
            return self.find(&id).await?.ok_or(ModerationError::ReviewStorage);
        }
        // 有竞争请求先提交时，重新读取并核对身份及所有模式字段。
        self.read("intent_key", &intent_key)
            .await?
            .ok_or(ModerationError::ReviewStorage)
            .and_then(|review| existing(review, request))
    }

    pub async fn find(&self, id: &str) -> ModerationResult<Option<VideoReview>> {
        if id.len() != 36 || !valid_identifier(id) {
            return Ok(None);
        }
        self.read("id", id).await
    }

    async fn read(&self, key: &str, value: &str) -> ModerationResult<Option<VideoReview>> {
        let row = self
            .review_rows()
            .r#where(&format!("{REVIEWS}.{key}"), "=", value)
            .first()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        row.map(decode).transpose()
    }

    fn review_rows(&self) -> Builder<'_> {
        self.connection
            .table(REVIEWS)
            .use_write_pdo()
            .join(
                CHECKS,
                "moderation_reviews.id",
                "=",
                "moderation_checks.review_id",
            )
            .select([
                "moderation_reviews.id as review_id",
                "moderation_checks.id as check_id",
                "content_id",
                "content_revision",
                "profile_revision",
                "disk",
                "object_key",
                "object_version",
                "created_at",
                "deadline_at",
                "version",
                "state",
                "submission",
                "submit_lease_until",
                "remote_task_id",
                "submit_failure",
                "available_at",
                "query_attempts",
                "query_lease_until",
                "query_failure",
                "assessment",
                "confirmation_callback_id",
                "lookup_hint_id",
                "provider_cancellation",
            ])
            .r#where("moderation_reviews.scope_key", "=", self.scope_key.as_str())
            .r#where("moderation_checks.kind", "=", "video")
    }
}

#[derive(Clone)]
struct ReviewIds {
    review: String,
    check: String,
}

impl ReviewIds {
    fn generate() -> ModerationResult<Self> {
        Ok(Self {
            review: new_id()?,
            check: new_id()?,
        })
    }
}

fn new_id() -> ModerationResult<String> {
    Str::uuid()
        .map(|uuid| uuid.to_string())
        .map_err(|_| ModerationError::ReviewStorage)
}

async fn read_clock(transaction: &mut Transaction, sql: &str) -> Result<i64, DatabaseError> {
    transaction
        .select_one(sql, &[])
        .await?
        .and_then(|row| integer(&row, "created_at"))
        .ok_or(DatabaseError::InvalidQuery("无法读取审核存储时钟"))
}

async fn write_check(
    transaction: &mut Transaction,
    ids: &ReviewIds,
    scope_key: &str,
    intent_key: &str,
    request: &VideoReviewRequest,
    created_at: i64,
) -> Result<(), DatabaseError> {
    let deadline_at = created_at + i64::from(request.timeout_seconds);
    let profile = serde_json::to_string(&request.profile)
        .map_err(|_| DatabaseError::InvalidQuery("无法序列化审核配置修订"))?;
    let modalities = serde_json::to_string(request.profile.required_modalities())
        .map_err(|_| DatabaseError::InvalidQuery("无法序列化审核模态"))?;
    transaction
        .table(REVIEWS)
        .insert(&[[
            ("id".into(), ids.review.as_str().into()),
            ("scope_key".into(), scope_key.into()),
            ("intent_key".into(), intent_key.into()),
            ("content_id".into(), request.content_id.as_str().into()),
            (
                "content_revision".into(),
                request.content_revision.as_str().into(),
            ),
            ("profile_revision".into(), profile.into()),
            ("disk".into(), request.source.disk().into()),
            ("object_key".into(), request.source.key().into()),
            ("object_version".into(), request.source.version().into()),
            ("state".into(), "Pending".into()),
            ("created_at".into(), created_at.into()),
            ("deadline_at".into(), deadline_at.into()),
            ("version".into(), 1_i64.into()),
        ]
        .into_iter()
        .collect()])
        .await?;
    transaction
        .table(CHECKS)
        .insert(&[[
            ("id".into(), ids.check.as_str().into()),
            ("review_id".into(), ids.review.as_str().into()),
            ("kind".into(), "video".into()),
            ("modalities".into(), modalities.into()),
            ("submission".into(), "Ready".into()),
            ("available_at".into(), created_at.into()),
        ]
        .into_iter()
        .collect()])
        .await
        .map(|_| ())
}

fn existing(review: VideoReview, request: &VideoReviewRequest) -> ModerationResult<VideoReview> {
    if review.matches(request) {
        Ok(review)
    } else {
        Err(ModerationError::ReviewConflict)
    }
}

fn decode(row: Row) -> ModerationResult<VideoReview> {
    let text = |column| match row.get(column) {
        Some(Value::Text(value)) => Ok(value.as_str()),
        _ => Err(ModerationError::ReviewStorage),
    };
    let number = |column| integer(&row, column).ok_or(ModerationError::ReviewStorage);
    let optional_text = |column| match row.get(column) {
        Some(Value::Text(value)) => Ok(Some(value.as_str())),
        Some(Value::Null) => Ok(None),
        _ => Err(ModerationError::ReviewStorage),
    };
    Ok(VideoReview {
        id: text("review_id")?.into(),
        check_id: text("check_id")?.into(),
        content_id: text("content_id")?.into(),
        content_revision: text("content_revision")?.into(),
        source: StoredMedia::new(text("disk")?, text("object_key")?, text("object_version")?)
            .map_err(|_| ModerationError::ReviewStorage)?,
        profile: serde_json::from_str(text("profile_revision")?)
            .map_err(|_| ModerationError::ReviewStorage)?,
        created_at: number("created_at")?,
        deadline_at: number("deadline_at")?,
        version: number("version")?,
        state: ReviewState::parse(text("state")?)?,
        submission: match text("submission")? {
            "Ready" => VideoSubmission::Ready,
            "Submitting" => VideoSubmission::Submitting,
            "Submitted" => VideoSubmission::Submitted(
                TaskId::new(text("remote_task_id")?).map_err(|_| ModerationError::ReviewStorage)?,
            ),
            "Unknown" => VideoSubmission::Unknown,
            "Failed" => VideoSubmission::Failed(
                serde_json::from_value(serde_json::Value::String(text("submit_failure")?.into()))
                    .map_err(|_| ModerationError::ReviewStorage)?,
            ),
            "Expired" => VideoSubmission::Expired,
            _ => return Err(ModerationError::ReviewStorage),
        },
        submit_lease_until: integer(&row, "submit_lease_until"),
        available_at: number("available_at")?,
        query_attempts: u16::try_from(number("query_attempts")?)
            .map_err(|_| ModerationError::ReviewStorage)?,
        query_lease_until: integer(&row, "query_lease_until"),
        query_failure: optional_text("query_failure")?
            .map(|value| serde_json::from_value(serde_json::Value::String(value.into())))
            .transpose()
            .map_err(|_| ModerationError::ReviewStorage)?,
        assessment: optional_text("assessment")?
            .map(assessment_record::decode)
            .transpose()?,
        confirmation_callback_id: optional_text("confirmation_callback_id")?.map(str::to_owned),
        lookup_hint_id: optional_text("lookup_hint_id")?.map(str::to_owned),
        provider_cancellation: optional_text("provider_cancellation")?
            .map(|value| serde_json::from_value(serde_json::Value::String(value.into())))
            .transpose()
            .map_err(|_| ModerationError::ReviewStorage)?,
    })
}

fn integer(row: &Row, column: &str) -> Option<i64> {
    match row.get(column)? {
        Value::Integer(value) => Some(*value),
        Value::Unsigned(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

fn digest(value: &str) -> String {
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(value.as_bytes()) {
        write!(result, "{byte:02x}").expect("向字符串写入摘要");
    }
    result
}

fn database_clock(connection: &Connection) -> &'static str {
    match connection.get_driver_name() {
        "sqlite" => "unixepoch()",
        "pgsql" => "floor(extract(epoch from statement_timestamp()))::bigint",
        "sqlsrv" => "datediff_big(second, '19700101', sysutcdatetime())",
        _ => "unix_timestamp()",
    }
}

fn database_clock_query(connection: &Connection) -> String {
    format!("select {} as created_at", database_clock(connection))
}
