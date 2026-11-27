use openrust::database::{DatabaseError, DatabaseResult, Value, query::Record};

use super::UsageStore;
use crate::managed_keys::{integer, now, text};

const TABLE: &str = "ai_gateway_gemini_batch_files";

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
            query.where_null("deleted_at")
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
        Ok(self
            .connection
            .table(TABLE)
            .r#where("owner", "=", file.owner.as_str())
            .r#where("id", "=", file.id.as_str())
            .where_null("deleted_at")
            .where_null("object_path")
            .update(Record::from([
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
}
