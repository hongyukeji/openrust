use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayOutputQuotaTable;
pub(crate) static MIGRATION: CreateAiGatewayOutputQuotaTable = CreateAiGatewayOutputQuotaTable;

impl Migration for CreateAiGatewayOutputQuotaTable {
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
                .create("ai_gateway_output_quota", |table| {
                    table.string("id", 64).primary();
                    table.string("group_name", 128);
                    table.big_integer("window_start").index();
                    table.big_integer("charged_tokens");
                    table.boolean("overrun");
                })
                .await?;
            schema
                .table("ai_gateway_usage", |table| {
                    table.string("output_quota_id", 64).nullable().index();
                    table.big_integer("reserved_output_tokens").nullable();
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
                    table.drop_index("ai_gateway_usage_output_quota_id_index");
                    table.drop_column(&["output_quota_id", "reserved_output_tokens"]);
                })
                .await?;
            schema.drop_if_exists("ai_gateway_output_quota").await
        })
    }
}
