use std::{fmt::Write as _, sync::Arc};

use openrust::{
    database::{Connection, DatabaseError, Row, Value},
    support::Str,
};
use sha2::{Digest as _, Sha256};

use crate::{
    ModerationError, ModerationResult, StoredMedia, VideoReview, VideoReviewRequest,
    credentials::valid_identifier,
};

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
        let id = Str::uuid()
            .map_err(|_| ModerationError::ReviewStorage)?
            .to_string();
        let check_id = Str::uuid()
            .map_err(|_| ModerationError::ReviewStorage)?
            .to_string();
        let record = Arc::new(request.clone());
        let scope_key = self.scope_key.clone();
        let saved_intent = intent_key.clone();
        let saved_id = id.clone();
        let clock = database_clock(&self.connection);
        let transaction = self
            .connection
            .transaction(
                move |transaction| {
                    let request = record.clone();
                    let scope_key = scope_key.clone();
                    let intent_key = saved_intent.clone();
                    let id = saved_id.clone();
                    let check_id = check_id.clone();
                    Box::pin(async move {
                        let created_at = transaction
                            .select_one(clock, &[])
                            .await?
                            .and_then(|row| integer(&row, "created_at"))
                            .ok_or(DatabaseError::InvalidQuery("无法读取审核存储时钟"))?;
                        let deadline_at = created_at + i64::from(request.timeout_seconds);
                        let profile = serde_json::to_string(&request.profile)
                            .map_err(|_| DatabaseError::InvalidQuery("无法序列化审核配置修订"))?;
                        let modalities =
                            serde_json::to_string(request.profile.required_modalities())
                                .map_err(|_| DatabaseError::InvalidQuery("无法序列化审核模态"))?;
                        transaction
                            .table(REVIEWS)
                            .insert(&[[
                                ("id".into(), id.as_str().into()),
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
                                ("id".into(), check_id.into()),
                                ("review_id".into(), id.into()),
                                ("kind".into(), "video".into()),
                                ("modalities".into(), modalities.into()),
                                ("submission".into(), "Ready".into()),
                                ("available_at".into(), created_at.into()),
                            ]
                            .into_iter()
                            .collect()])
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
            .connection
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
            ])
            .r#where("moderation_reviews.scope_key", "=", self.scope_key.as_str())
            .r#where(&format!("{REVIEWS}.{key}"), "=", value)
            .r#where("moderation_checks.kind", "=", "video")
            .first()
            .await
            .map_err(|_| ModerationError::ReviewStorage)?;
        row.map(decode).transpose()
    }
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
        "sqlite" => "select unixepoch() as created_at",
        "pgsql" => "select floor(extract(epoch from statement_timestamp()))::bigint as created_at",
        "sqlsrv" => "select datediff_big(second, '19700101', sysutcdatetime()) as created_at",
        _ => "select unix_timestamp() as created_at",
    }
}
