use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddObservationToAiGatewayUsage;
pub(crate) static MIGRATION: AddObservationToAiGatewayUsage = AddObservationToAiGatewayUsage;

impl Migration for AddObservationToAiGatewayUsage {
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
                    table.string("usage_state", 24).default("legacy");
                    table.big_integer("dispatch_started_at").nullable();
                    table.string("upstream_request_id", 255).nullable();
                    table.string("reported_model", 255).nullable();
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
                    table.drop_column(&[
                        "usage_state",
                        "dispatch_started_at",
                        "upstream_request_id",
                        "reported_model",
                    ]);
                })
                .await
        })
    }
}
