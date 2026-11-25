use super::{DatabaseWebhookStore, scope_key, text};
use crate::{Error, Result, factories::validate_reference, models::WebhookCall};
use openrust::{
    database::{Row, Transaction, Value, query::Builder},
    filesystem::{Storage, Visibility, WriteOptions},
};
use std::time::Duration;
use tokio::io::AsyncReadExt as _;

const MAX_BYTES: usize = 16 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttachmentReference {
    disk: String,
    length: usize,
    digest: String,
}

impl DatabaseWebhookStore {
    pub(super) async fn stage_attachment(
        &self,
        call: &WebhookCall,
        disk: &str,
    ) -> Result<AttachmentReference> {
        let path = object_path(&self.scope, call.id())?;
        let filesystem = Storage::disk(Some(disk)).map_err(|_| Error::AttachmentStorage)?;
        let reference = AttachmentReference {
            disk: disk.to_owned(),
            length: call.raw_body().len(),
            digest: digest(call.raw_body()),
        };
        self.connection
            .table("webhook_attachment_staging")
            .insert(&[[
                ("id".into(), call.id().into()),
                ("scope_key".into(), scope_key(&self.scope).into()),
                ("disk".into(), disk.into()),
                ("created_at".into(), call.created_at().into()),
            ]
            .into_iter()
            .collect()])
            .await?;
        let written = tokio::time::timeout(
            IO_TIMEOUT,
            filesystem.put_with_options(
                &path,
                call.raw_body(),
                WriteOptions {
                    visibility: Some(Visibility::Private),
                },
            ),
        )
        .await
        .map_err(|_| Error::AttachmentStorage)?
        .map_err(|_| Error::AttachmentStorage)?;
        if !written {
            return Err(Error::AttachmentStorage);
        }
        Ok(reference)
    }

    pub(super) async fn finish_attachment(
        &self,
        transaction: &mut Transaction,
        id: &str,
    ) -> Result<()> {
        let changed = self
            .staging(transaction.table("webhook_attachment_staging"), id)
            .where_null("cleanup_token")
            .delete()
            .await?;
        if changed != 1 {
            return Err(Error::AttachmentStorage);
        }
        Ok(())
    }

    pub(super) async fn discard_attachment(&self, id: &str, reference: &AttachmentReference) {
        let result = async {
            if self
                .remove_attachment_object(id, &reference.disk)
                .await
                .is_ok()
            {
                self.staging(self.connection.table("webhook_attachment_staging"), id)
                    .delete()
                    .await?;
            }
            Ok::<(), Error>(())
        }
        .await;
        // 清理失败时保留登记，以便后续补偿；接收方保留原始错误。
        let _ = result;
    }

    pub(super) async fn remove_attachment_object(&self, id: &str, disk: &str) -> Result<()> {
        validate_reference("attachment_disk", disk)?;
        let path = object_path(&self.scope, id)?;
        let disk = Storage::disk(Some(disk)).map_err(|_| Error::AttachmentStorage)?;
        let deleted = tokio::time::timeout(IO_TIMEOUT, disk.delete(&path))
            .await
            .map_err(|_| Error::AttachmentStorage)?
            .map_err(|_| Error::AttachmentStorage)?;
        if !deleted {
            return Err(Error::AttachmentStorage);
        }
        Ok(())
    }

    pub(super) fn staging<'a>(&self, builder: Builder<'a>, id: &str) -> Builder<'a> {
        builder
            .r#where("id", "=", id)
            .r#where("scope_key", "=", scope_key(&self.scope))
    }

    pub(super) async fn disk_body(&self, row: &Row) -> Result<Option<Vec<u8>>> {
        let Some(reference) = AttachmentReference::from_row(row)? else {
            return Ok(None);
        };
        let path = object_path(&self.scope, text(row, "id")?)?;
        let disk = Storage::disk(Some(&reference.disk)).map_err(|_| Error::AttachmentStorage)?;
        let bytes = tokio::time::timeout(IO_TIMEOUT, async {
            let reader = disk
                .read_stream(&path)
                .await
                .map_err(|_| Error::AttachmentStorage)?
                .ok_or(Error::AttachmentStorage)?;
            let mut bytes = Vec::with_capacity(reference.length);
            reader
                .take(reference.length as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| Error::AttachmentStorage)?;
            Ok::<_, Error>(bytes)
        })
        .await
        .map_err(|_| Error::AttachmentStorage)??;
        if bytes.len() != reference.length || digest(&bytes) != reference.digest {
            return Err(Error::InvalidRecord);
        }
        Ok(Some(bytes))
    }
}

impl AttachmentReference {
    pub(super) fn from_row(row: &Row) -> Result<Option<Self>> {
        let reference = match row.get("attachment_reference") {
            Some(Value::Null) => return Ok(None),
            Some(Value::Text(value)) if value.len() <= 512 => value,
            _ => return Err(Error::InvalidRecord),
        };
        let reference: Self = serde_json::from_str(reference).map_err(|_| Error::InvalidRecord)?;
        validate_reference("attachment_disk", &reference.disk)?;
        if !(1..=MAX_BYTES).contains(&reference.length) || reference.digest.len() != 64 {
            return Err(Error::InvalidRecord);
        }
        Ok(Some(reference))
    }

    pub(super) fn disk(&self) -> &str {
        &self.disk
    }
}

fn object_path(scope: &str, id: &str) -> Result<String> {
    if id.len() != 36
        || !id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
    {
        return Err(Error::InvalidRecord);
    }
    Ok(format!(
        "webhook-client/{}/{}.multipart",
        scope_key(scope),
        id
    ))
}

fn digest(bytes: &[u8]) -> String {
    aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
