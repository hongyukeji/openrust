use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddKeyScopeToAiGatewayUsage;
pub(crate) static MIGRATION: AddKeyScopeToAiGatewayUsage = AddKeyScopeToAiGatewayUsage;

impl Migration for AddKeyScopeToAiGatewayUsage {
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
                    table.string("static_key_scope", 64).nullable();
                    table.index(
                        &["static_key_scope", "started_at"],
                        Some("ai_usage_scope_started_index"),
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
                    table.drop_index("ai_usage_scope_started_index");
                    table.drop_column(&["static_key_scope"]);
                })
                .await
        })
    }
}
