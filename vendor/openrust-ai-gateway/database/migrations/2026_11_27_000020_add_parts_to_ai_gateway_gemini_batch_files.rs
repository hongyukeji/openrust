use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddPartsToAiGatewayGeminiBatchFiles;
pub(crate) static MIGRATION: AddPartsToAiGatewayGeminiBatchFiles =
    AddPartsToAiGatewayGeminiBatchFiles;

impl Migration for AddPartsToAiGatewayGeminiBatchFiles {
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
                .table("ai_gateway_gemini_batch_files", |table| {
                    table.big_integer("received_bytes").default(0);
                    table.text("upload_parts").nullable();
                    table.boolean("finalized").default(false);
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
                    table.drop_column(&["received_bytes", "upload_parts", "finalized"]);
                })
                .await
        })
    }
}
