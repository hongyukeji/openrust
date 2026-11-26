use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayCostPagesTables;
pub(crate) static MIGRATION: CreateAiGatewayCostPagesTables = CreateAiGatewayCostPagesTables;

impl Migration for CreateAiGatewayCostPagesTables {
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
                .create("ai_gateway_cost_uploads", |table| {
                    table.string("id", 64).primary();
                    table.big_integer("expected_pages");
                    table.boolean("complete");
                })
                .await?;
            schema
                .create("ai_gateway_cost_pages", |table| {
                    table.string("report_id", 64);
                    table.big_integer("page_number");
                    table.long_text("payload");
                    table.string("actor", 128);
                    table.big_integer("imported_at");
                    table.unique(
                        &["report_id", "page_number"],
                        Some("ai_gw_cost_report_page"),
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
            let mut schema = context.get_schema_builder();
            schema.drop_if_exists("ai_gateway_cost_pages").await?;
            schema.drop_if_exists("ai_gateway_cost_uploads").await
        })
    }
}
