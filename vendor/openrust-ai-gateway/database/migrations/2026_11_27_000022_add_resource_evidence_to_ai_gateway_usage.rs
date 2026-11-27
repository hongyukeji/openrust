use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddResourceEvidenceToAiGatewayUsage;
pub(crate) static MIGRATION: AddResourceEvidenceToAiGatewayUsage =
    AddResourceEvidenceToAiGatewayUsage;

impl Migration for AddResourceEvidenceToAiGatewayUsage {
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
                .table("ai_gateway_usage", |table| {
                    table.string("resource_id", 160).nullable();
                    table.text("resource_usage").nullable();
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
                .table("ai_gateway_usage", |table| {
                    table.drop_column(&["resource_id", "resource_usage"]);
                })
                .await
        })
    }
}
