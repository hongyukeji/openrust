use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddAiGatewayAccountBudgets;
pub(crate) static MIGRATION: AddAiGatewayAccountBudgets = AddAiGatewayAccountBudgets;

impl Migration for AddAiGatewayAccountBudgets {
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
                .table("ai_gateway_budgets", |table| {
                    table.text("account_name").nullable();
                })
                .await?;
            schema
                .table("ai_gateway_usage", |table| {
                    table.string("account_budget_id", 64).nullable().index();
                    table.big_integer("reserved_cost_atoms").nullable();
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
                    table.drop_index("ai_gateway_usage_account_budget_id_index");
                    table.drop_column(&["account_budget_id", "reserved_cost_atoms"]);
                })
                .await?;
            schema
                .table("ai_gateway_budgets", |table| {
                    table.drop_column(&["account_name"]);
                })
                .await
        })
    }
}
