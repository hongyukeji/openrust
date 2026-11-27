use std::time::{SystemTime, UNIX_EPOCH};

use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddExpiryToAiGatewayGeminiBatchFiles;
pub(crate) static MIGRATION: AddExpiryToAiGatewayGeminiBatchFiles =
    AddExpiryToAiGatewayGeminiBatchFiles;

impl Migration for AddExpiryToAiGatewayGeminiBatchFiles {
    fn get_connection(&self, config: &dyn Repository) -> DatabaseResult<Option<String>> {
        match config.get("ai_gateway.usage.connection") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(name)) if !name.trim().is_empty() => Ok(Some(name)),
            _ => Err(DatabaseError::InvalidConfiguration(
                "ai_gateway.usage.connection",
            )),
        }
    }

    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            // 升级前已经暂存的文件从部署迁移起保留完整的窗口。
            let deployed_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| DatabaseError::InvalidScalar)?;
            let expires_at = i64::try_from(deployed_at.as_secs())
                .map_err(|_| DatabaseError::InvalidScalar)?
                + 48 * 60 * 60;
            context
                .get_schema_builder()
                .table("ai_gateway_gemini_batch_files", |table| {
                    table.big_integer("expires_at").default(expires_at);
                    table.index(
                        &["expires_at", "id"],
                        Some("ai_gateway_gemini_batch_files_expiry_idx"),
                    );
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("ai_gateway_gemini_batch_files", |table| {
                    table.drop_index("ai_gateway_gemini_batch_files_expiry_idx");
                    table.drop_column(&["expires_at"]);
                })
                .await
        })
    }
}
