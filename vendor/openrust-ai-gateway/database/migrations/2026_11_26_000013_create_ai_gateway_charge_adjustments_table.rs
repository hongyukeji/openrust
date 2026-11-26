use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayChargeAdjustmentsTable;
pub(crate) static MIGRATION: CreateAiGatewayChargeAdjustmentsTable =
    CreateAiGatewayChargeAdjustmentsTable;

impl Migration for CreateAiGatewayChargeAdjustmentsTable {
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
                .create("ai_gateway_charge_adjustments", |table| {
                    table.string("id", 64).primary();
                    table.string("attempt_id", 26);
                    table.string("direction", 16);
                    table.string("currency", 3);
                    table.string("kind", 16);
                    table.string("amount", 48);
                    table.string("reference", 128);
                    table.long_text("reason");
                    table.string("report_id", 64).nullable();
                    table.string("actor", 128);
                    table.big_integer("created_at");
                    table.string("fingerprint", 64);
                    table.index(
                        &["attempt_id", "direction"],
                        Some("ai_gw_charge_adjustment_attempt"),
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
                .drop_if_exists("ai_gateway_charge_adjustments")
                .await
        })
    }
}
