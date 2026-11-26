use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayCostEvidenceTables;
pub(crate) static MIGRATION: CreateAiGatewayCostEvidenceTables = CreateAiGatewayCostEvidenceTables;

impl Migration for CreateAiGatewayCostEvidenceTables {
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
                .create("ai_gateway_cost_reports", |table| {
                    table.string("id", 64).primary();
                    table.string("statement_scope", 64);
                    table.string("source", 128);
                    table.string("statement_id", 128);
                    table.big_integer("revision");
                    table.boolean("complete");
                    table.big_integer("starts_at");
                    table.big_integer("ends_at");
                    table.string("content_hash", 64);
                    table.string("actor", 128);
                    table.big_integer("imported_at");
                    table.index(
                        &["statement_scope", "revision"],
                        Some("ai_gw_cost_report_revisions"),
                    );
                })
                .await?;
            schema
                .create("ai_gateway_cost_evidence", |table| {
                    table.string("report_id", 64);
                    table.string("entry_id", 128);
                    table.string("entry_scope", 64);
                    table.string("attempt_id", 26).nullable();
                    table.string("provider", 255);
                    table.string("upstream_request_id", 255);
                    table.string("currency", 3);
                    table.string("amount", 48);
                    table.string("state", 24);
                    table.unique(
                        &["report_id", "entry_id"],
                        Some("ai_gw_cost_evidence_report_entry"),
                    );
                    table.index(&["attempt_id"], Some("ai_gw_cost_evidence_attempt"));
                    table.index(&["entry_scope"], Some("ai_gw_cost_evidence_scope"));
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
            schema.drop_if_exists("ai_gateway_cost_evidence").await?;
            schema.drop_if_exists("ai_gateway_cost_reports").await
        })
    }
}
