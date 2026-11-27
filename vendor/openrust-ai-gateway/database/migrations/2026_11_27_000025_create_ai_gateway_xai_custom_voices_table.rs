use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayXaiCustomVoicesTable;
pub(crate) static MIGRATION: CreateAiGatewayXaiCustomVoicesTable =
    CreateAiGatewayXaiCustomVoicesTable;

impl Migration for CreateAiGatewayXaiCustomVoicesTable {
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
                .create("ai_gateway_xai_custom_voices", |table| {
                    table.string("id", 8).primary();
                    table.string("position", 26).unique();
                    table.string("owner", 64);
                    table.string("scope", 64);
                    table.string("upstream_id", 8).nullable();
                    table.index(&["owner", "position"], Some("ai_voice_owner_page"));
                    table.unique(
                        &["scope", "upstream_id"],
                        Some("ai_voice_upstream_identity"),
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
                .drop_if_exists("ai_gateway_xai_custom_voices")
                .await
        })
    }
}
