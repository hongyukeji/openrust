use super::{DatabaseWebhookStore, scope_key, text};
use crate::{Error, Result};
use openrust::{
    database::{
        Value,
        query::{Builder, Expression, UpdateValue},
    },
    support::Str,
};

/// 本次有界批次的候选、清理成功及待重试数量。
#[derive(Default)]
pub struct AttachmentCleanupReport {
    pub candidates: usize,
    pub removed: usize,
    pub failed: usize,
}

impl DatabaseWebhookStore {
    /// 收回早于一小时的孤立对象和已解除引用的过期对象。
    pub async fn prune_attachment_staging(
        &self,
        limit: u32,
        pretend: bool,
    ) -> Result<AttachmentCleanupReport> {
        if !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("prune.limit"));
        }
        let rows = self
            .cleanup_candidates(self.connection.table("webhook_attachment_staging"))
            .use_write_pdo()
            .select(["id", "disk"])
            .order_by("created_at", "asc")
            .order_by("id", "asc")
            .limit(u64::from(limit))
            .get()
            .await?;
        let mut report = AttachmentCleanupReport {
            candidates: rows.len(),
            ..Default::default()
        };
        if pretend {
            return Ok(report);
        }
        for row in rows {
            let id = text(&row, "id")?;
            let disk = text(&row, "disk")?;
            let token = Str::uuid().map_err(|_| Error::InvalidRecord)?.to_string();
            let claim = self
                .cleanup_candidates(
                    self.connection
                        .table("webhook_attachment_staging")
                        .r#where("id", "=", id),
                )
                .update_with(
                    [
                        (
                            "cleanup_token".into(),
                            UpdateValue::Operand(token.clone().into()),
                        ),
                        (
                            "cleanup_until".into(),
                            UpdateValue::Operand(
                                Expression::new(format!("{} + 60", self.clock())).into(),
                            ),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await?;
            if claim == 0 {
                continue;
            }
            if self.remove_attachment_object(id, disk).await.is_err() {
                // 保留所有权标记，等待租约过期后继续补偿。
                report.failed += 1;
                continue;
            }
            let removed = self
                .staging(self.connection.table("webhook_attachment_staging"), id)
                .r#where("cleanup_token", "=", token)
                .delete()
                .await?;
            report.removed += usize::from(removed == 1);
        }
        Ok(report)
    }

    fn cleanup_candidates<'a>(&self, builder: Builder<'a>) -> Builder<'a> {
        let clock = self.clock();
        builder
            .r#where("scope_key", "=", scope_key(&self.scope))
            .where_nested(|query| {
                query
                    .r#where("detached", "=", Value::from(true))
                    .or_where_raw(&format!("created_at <= {clock} - 3600"), vec![])
            })
            .where_nested(|query| {
                query
                    .where_null("cleanup_until")
                    .or_where_raw(&format!("cleanup_until <= {clock}"), vec![])
            })
    }
}
