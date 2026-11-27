use openrust::{
    database::{DatabaseError, DatabaseResult, Value, query::Record},
    filesystem::Storage,
    support::Str,
};

use serde::{Deserialize, Serialize};

use super::UsageStore;
use crate::managed_keys::{integer, now, text};

pub(crate) const RETENTION_SECONDS: i64 = 48 * 60 * 60;
pub(crate) const DISK: &str = "local";
pub(crate) const MAX_PARTS: usize = 256;
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

#[derive(Deserialize, Serialize)]
pub(crate) struct UploadPart {
    pub(crate) path: String,
    pub(crate) size: usize,
    pub(crate) sha256: String,
}

pub(crate) struct BatchFile {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) display_name: String,
    pub(crate) mime: String,
    pub(crate) size: usize,
    pub(crate) object: Option<(String, String)>,
    pub(crate) deleted: bool,
    pub(crate) received: usize,
    pub(crate) parts: Vec<UploadPart>,
    pub(crate) finalized: bool,
}

impl BatchFile {
    pub(crate) fn confirmed_bytes(&self) -> usize {
        if self.object.is_some() {
            self.size
        } else {
            self.received
        }
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.finalized || self.object.is_some()
    }

    pub(crate) fn matches_part(&self, offset: usize, size: usize, sha256: &str) -> bool {
        let mut start = 0;
        for part in &self.parts {
            if start == offset {
                return part.size == size && part.sha256 == sha256;
            }
            start += part.size;
        }
        false
    }
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
        let size = read_size(&row, "size_bytes")?;
        let received = read_size(&row, "received_bytes")?;
        let finalized = match row.get("finalized") {
            Some(Value::Bool(value)) => *value,
            Some(Value::Integer(value @ (0 | 1))) => *value == 1,
            _ => return Err(DatabaseError::InvalidScalar),
        };
        let parts: Vec<UploadPart> = match row.get("upload_parts") {
            Some(Value::Text(parts)) => {
                serde_json::from_str(parts).map_err(|_| DatabaseError::InvalidScalar)?
            }
            Some(Value::Null) => Vec::new(),
            _ => return Err(DatabaseError::InvalidScalar),
        };
        let prefix = format!("{}/", directory(id));
        let valid_path = |path: &str| path.strip_prefix(&prefix).is_some_and(Str::is_ulid);
        if parts.len() > MAX_PARTS
            || parts.iter().try_fold(0_usize, |total, part| {
                (valid_path(&part.path) && part.size > 0).then_some(())?;
                total.checked_add(part.size)
            }) != Some(received)
            || received > size
            || (finalized && received != size)
            || object
                .as_ref()
                .is_some_and(|(path, _)| finalized || received != 0 || !valid_path(path))
        {
            return Err(DatabaseError::InvalidScalar);
        }
        Ok(Some(BatchFile {
            id: id.into(),
            owner: owner.into(),
            display_name: text(&row, "display_name")?,
            mime: text(&row, "mime")?,
            size,
            received,
            parts,
            finalized,
            object,
            deleted: integer(&row, "deleted_at")?.is_some(),
        }))
    }

    pub(crate) async fn record_gemini_batch_file_part(
        &self,
        file: &BatchFile,
        part: Option<&UploadPart>,
        finalize: bool,
    ) -> DatabaseResult<bool> {
        let now = now();
        let received = file.received + part.map_or(0, |part| part.size);
        let mut update = Record::from([
            ("received_bytes".into(), (received as i64).into()),
            ("finalized".into(), finalize.into()),
        ]);
        if let Some(part) = part {
            let parts = file
                .parts
                .iter()
                .chain(std::iter::once(part))
                .collect::<Vec<_>>();
            let parts = serde_json::to_string(&parts).map_err(|_| DatabaseError::InvalidScalar)?;
            update.insert("upload_parts".into(), parts.into());
        }
        if finalize {
            update.insert("expires_at".into(), (now + RETENTION_SECONDS).into());
        }
        Ok(self
            .connection
            .table(TABLE)
            .r#where("owner", "=", file.owner.as_str())
            .r#where("id", "=", file.id.as_str())
            .where_null("deleted_at")
            .where_null("object_path")
            .r#where("finalized", "=", false)
            .r#where("received_bytes", "=", file.received as i64)
            .r#where("expires_at", ">", now)
            .update(update)
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

fn read_size(row: &openrust::database::Row, column: &str) -> DatabaseResult<usize> {
    integer(row, column)?
        .and_then(|size| usize::try_from(size).ok())
        .ok_or(DatabaseError::InvalidScalar)
}
