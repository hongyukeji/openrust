use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct CreateWebhookVerificationAuditsTable;
pub(crate) static MIGRATION: CreateWebhookVerificationAuditsTable =
    CreateWebhookVerificationAuditsTable;
impl Migration for CreateWebhookVerificationAuditsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_verification_audits", |table| {
                    table.string("audit_key", 64).primary();
                    table.string("scope_key", 64);
                    table.string("scope", 128);
                    table.string("profile", 128);
                    table.string("revision", 128);
                    table.string("validator", 128);
                    table.string("policy", 128);
                    table.string("classification", 16);
                    table.string("evidence", 24);
                    table.big_integer("observed_minute");
                    table.index(
                        &["scope_key", "observed_minute"],
                        Some("webhook_audit_scope_minute"),
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
                .drop_if_exists("webhook_verification_audits")
                .await
        })
    }
}
