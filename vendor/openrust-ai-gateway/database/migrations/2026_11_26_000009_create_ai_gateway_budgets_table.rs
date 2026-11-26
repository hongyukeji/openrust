use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayBudgetsTable;
pub(crate) static MIGRATION: CreateAiGatewayBudgetsTable = CreateAiGatewayBudgetsTable;

impl Migration for CreateAiGatewayBudgetsTable {
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
            let mut schema = context.get_schema_builder();
            schema
                .create("ai_gateway_budgets", |table| {
                    table.string("id", 64).primary();
                    table.string("group_name", 128);
                    table.big_integer("window_start").index();
                    table.string("currency", 3);
                    table.big_integer("occupied_atoms");
                    table.big_integer("held_atoms");
                    table.boolean("overrun");
                })
                .await?;
            schema
                .table("ai_gateway_usage", |table| {
                    table.string("budget_id", 64).nullable().index();
                    table.big_integer("reserved_receivable_atoms").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = context.get_schema_builder();
            schema
                .table("ai_gateway_usage", |table| {
                    table.drop_index("ai_gateway_usage_budget_id_index");
                    table.drop_column(&["budget_id", "reserved_receivable_atoms"]);
                })
                .await?;
            schema.drop_if_exists("ai_gateway_budgets").await
        })
    }
}
