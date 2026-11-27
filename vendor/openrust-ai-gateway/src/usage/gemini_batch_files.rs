use openrust::{
    database::{DatabaseError, DatabaseResult, Value, query::Record},
    filesystem::Storage,
    support::Str,
};

use super::UsageStore;
use crate::managed_keys::{integer, now, text};

pub(crate) const RETENTION_SECONDS: i64 = 48 * 60 * 60;
pub(crate) const DISK: &str = "local";
const CLEANUP_GRACE_SECONDS: i64 = 5 * 60;
const TABLE: &str = "ai_gateway_gemini_batch_files";

pub(crate) fn valid_id(id: &str) -> bool {
    id.strip_prefix("input-").is_some_and(|suffix| {
        !suffix.bytes().any(|byte| byte.is_ascii_uppercase())
            && Str::is_ulid(&suffix.to_ascii_uppercase())
    })
}

pub(crate) fn directory(id: &str) -> String {
    format!("ai/gateway/gemini/batch-files/{id}")
}

#[derive(Default)]
pub(crate) struct CleanupReport {
    pub(crate) candidates: usize,
    pub(crate) removed: usize,
    pub(crate) failed: usize,
}

pub(crate) struct BatchFile {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) display_name: String,
    pub(crate) mime: String,
    pub(crate) size: usize,
    pub(crate) object: Option<(String, String)>,
    pub(crate) deleted: bool,
}

impl UsageStore {
    pub(crate) async fn create_gemini_batch_file(&self, file: &BatchFile) -> DatabaseResult<()> {
        let inserted = self
            .connection
            .table(TABLE)
            .insert(&[Record::from([
                ("id".into(), file.id.as_str().into()),
                ("owner".into(), file.owner.as_str().into()),
                ("display_name".into(), file.display_name.as_str().into()),
                ("mime".into(), file.mime.as_str().into()),
                ("expires_at".into(), (now() + RETENTION_SECONDS).into()),
                (
                    "size_bytes".into(),
                    i64::try_from(file.size)
                        .map_err(|_| DatabaseError::InvalidScalar)?
                        .into(),
                ),
            ])])
            .await?;
        if !inserted {
            return Err(DatabaseError::InvalidQuery("批任务文件登记失败"));
        }
        Ok(())
    }

    pub(crate) async fn gemini_batch_file(
        &self,
        owner: &str,
        id: &str,
        include_deleted: bool,
    ) -> DatabaseResult<Option<BatchFile>> {
        let query = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("owner", "=", owner)
            .r#where("id", "=", id);
        let query = if include_deleted {
            query
        } else {
            query
                .where_null("deleted_at")
                .r#where("expires_at", ">", now())
        };
        let row = query.first().await?;
        let Some(row) = row.filter(|row| {
            text(row, "id").is_ok_and(|value| value == id)
                && text(row, "owner").is_ok_and(|value| value == owner)
        }) else {
            return Ok(None);
        };
        let object = match (row.get("object_path"), row.get("sha256")) {
            (Some(Value::Text(path)), Some(Value::Text(hash))) => {
                Some((path.clone(), hash.clone()))
            }
            (Some(Value::Null), Some(Value::Null)) => None,
            _ => return Err(DatabaseError::InvalidScalar),
        };
        Ok(Some(BatchFile {
            id: id.into(),
            owner: owner.into(),
            display_name: text(&row, "display_name")?,
            mime: text(&row, "mime")?,
            size: integer(&row, "size_bytes")?
                .and_then(|size| usize::try_from(size).ok())
                .ok_or(DatabaseError::InvalidScalar)?,
            object,
            deleted: integer(&row, "deleted_at")?.is_some(),
        }))
    }

    pub(crate) async fn finish_gemini_batch_file(
        &self,
        file: &BatchFile,
        path: &str,
        sha256: &str,
    ) -> DatabaseResult<bool> {
        let now = now();
        Ok(self
            .connection
            .table(TABLE)
            .r#where("owner", "=", file.owner.as_str())
            .r#where("id", "=", file.id.as_str())
            .where_null("deleted_at")
            .where_null("object_path")
            .r#where("expires_at", ">", now)
            .update(Record::from([
                ("expires_at".into(), (now + RETENTION_SECONDS).into()),
                ("object_path".into(), path.into()),
                ("sha256".into(), sha256.into()),
            ]))
            .await?
            == 1)
    }

    pub(crate) async fn delete_gemini_batch_file(&self, file: &BatchFile) -> DatabaseResult<bool> {
        let query = self
            .connection
            .table(TABLE)
            .r#where("owner", "=", file.owner.as_str())
            .r#where("id", "=", file.id.as_str())
            .where_null("deleted_at");
        let query = match &file.object {
            Some((path, _)) => query.r#where("object_path", "=", path.as_str()),
            None => query.where_null("object_path"),
        };
        Ok(query
            .update(Record::from([("deleted_at".into(), now().into())]))
            .await?
            == 1)
    }

    /// 清理已过期的上传及其暂存目录，保留删除失败的记录供下次重试。
    pub(crate) async fn prune_gemini_batch_files(
        &self,
        limit: u64,
        pretend: bool,
    ) -> DatabaseResult<CleanupReport> {
        let cutoff = now() - CLEANUP_GRACE_SECONDS;
        let rows = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("expires_at", "<=", cutoff)
            .select(["id"])
            .order_by("expires_at", "asc")
            .order_by("id", "asc")
            .limit(limit)
            .get()
            .await?;
        let mut report = CleanupReport {
            candidates: rows.len(),
            ..CleanupReport::default()
        };
        if pretend || rows.is_empty() {
            return Ok(report);
        }
        let disk = Storage::disk(Some(DISK))
            .map_err(|_| DatabaseError::InvalidQuery("无法打开批文件磁盘"))?;
        for row in rows {
            let id = text(&row, "id")?;
            if !valid_id(&id) {
                report.failed += 1;
                continue;
            }
            // 文件已经过期，上传入口无法再为该 ID 发布新对象。
            if !matches!(disk.delete_directory(&directory(&id)).await, Ok(true)) {
                report.failed += 1;
                continue;
            }
            let removed = self
                .connection
                .table(TABLE)
                .r#where("id", "=", id.as_str())
                .r#where("expires_at", "<=", cutoff)
                .delete()
                .await?;
            report.removed += usize::from(removed == 1);
        }
        Ok(report)
    }
}
