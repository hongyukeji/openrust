use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayGeminiBatchFilesTable;
pub(crate) static MIGRATION: CreateAiGatewayGeminiBatchFilesTable =
    CreateAiGatewayGeminiBatchFilesTable;

impl Migration for CreateAiGatewayGeminiBatchFilesTable {
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
            context
                .get_schema_builder()
                .create("ai_gateway_gemini_batch_files", |table| {
                    table.string("id", 32).primary();
                    table.string("owner", 64);
                    table.string("display_name", 255);
                    table.string("mime", 64);
                    table.big_integer("size_bytes");
                    table.string("object_path", 255).nullable();
                    table.string("sha256", 64).nullable();
                    table.big_integer("deleted_at").nullable();
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
                .drop_if_exists("ai_gateway_gemini_batch_files")
                .await
        })
    }
}
