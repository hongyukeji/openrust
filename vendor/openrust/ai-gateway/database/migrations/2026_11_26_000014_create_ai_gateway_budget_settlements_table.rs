use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayBudgetSettlementsTable;
pub(crate) static MIGRATION: CreateAiGatewayBudgetSettlementsTable =
    CreateAiGatewayBudgetSettlementsTable;

impl Migration for CreateAiGatewayBudgetSettlementsTable {
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
                .create("ai_gateway_budget_settlements", |table| {
                    table.string("id", 64).primary();
                    table.string("attempt_id", 26);
                    table.string("direction", 16);
                    table.string("budget_id", 64);
                    table.big_integer("sequence");
                    table.string("currency", 3);
                    table.string("effective_amount", 48);
                    table.big_integer("booked_atoms").nullable();
                    table.big_integer("released_atoms");
                    table.string("reference", 128);
                    table.string("actor", 128);
                    table.big_integer("settled_at");
                    table.unique(
                        &["attempt_id", "direction", "sequence"],
                        Some("ai_gw_budget_attempt_sequence"),
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
                .drop_if_exists("ai_gateway_budget_settlements")
                .await
        })
    }
}
