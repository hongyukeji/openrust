use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayChargesTable;
pub(crate) static MIGRATION: CreateAiGatewayChargesTable = CreateAiGatewayChargesTable;

impl Migration for CreateAiGatewayChargesTable {
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
                .create("ai_gateway_charges", |table| {
                    table.string("attempt_id", 26);
                    table.string("direction", 16);
                    table.string("version", 128);
                    table.string("currency", 3).nullable();
                    table.long_text("tariff").nullable();
                    table.string("amount", 48).nullable();
                    table.string("state", 24);
                    table.unique(
                        &["attempt_id", "direction"],
                        Some("ai_gw_charges_attempt_direction"),
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
                .drop_if_exists("ai_gateway_charges")
                .await
        })
    }
}
