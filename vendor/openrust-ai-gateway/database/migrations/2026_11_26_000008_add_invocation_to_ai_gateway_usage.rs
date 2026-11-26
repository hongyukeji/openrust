use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddInvocationToAiGatewayUsage;
pub(crate) static MIGRATION: AddInvocationToAiGatewayUsage = AddInvocationToAiGatewayUsage;

impl Migration for AddInvocationToAiGatewayUsage {
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
                    table.string("consumer_invocation", 26).nullable();
                    table.string("invocation_scope", 64).nullable();
                    table.big_integer("model_turn").nullable();
                    table.index(
                        &["consumer_invocation", "started_at"],
                        Some("ai_gw_usage_invocation"),
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
                .table("ai_gateway_usage", |table| {
                    table.drop_index("ai_gw_usage_invocation");
                    table.drop_column(&["consumer_invocation", "invocation_scope", "model_turn"]);
                })
                .await
        })
    }
}
