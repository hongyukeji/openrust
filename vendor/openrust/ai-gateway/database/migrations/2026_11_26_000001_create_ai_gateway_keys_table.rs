use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayKeysTable;
pub(crate) static MIGRATION: CreateAiGatewayKeysTable = CreateAiGatewayKeysTable;

impl Migration for CreateAiGatewayKeysTable {
    fn get_connection(&self, config: &dyn Repository) -> DatabaseResult<Option<String>> {
        match config.get("ai_gateway.managed_keys.connection") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(name)) if !name.trim().is_empty() => Ok(Some(name)),
            _ => Err(DatabaseError::InvalidConfiguration(
                "ai_gateway.managed_keys.connection",
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
                .create("ai_gateway_keys", |table| {
                    table.string("id", 26).primary();
                    table.string("name", 128);
                    table.string("group_name", 128).index();
                    table.string("digest", 64).nullable().unique();
                    table.big_integer("requests_per_minute").nullable();
                    table.big_integer("created_at");
                    table.big_integer("updated_at");
                    table.big_integer("expires_at").nullable();
                    table.big_integer("revoked_at").nullable();
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
                .drop_if_exists("ai_gateway_keys")
                .await
        })
    }
}
