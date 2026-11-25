use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationQueryBudget;
pub(crate) static MIGRATION: AddModerationQueryBudget = AddModerationQueryBudget;

impl Migration for AddModerationQueryBudget {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.integer("query_attempts").default(0);
                    table.big_integer("query_lease_until").nullable();
                    table.string("query_failure", 32).nullable();
                    table.text("assessment").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("moderation_checks")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核子检查"));
            }
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.drop_column(&[
                        "query_attempts",
                        "query_lease_until",
                        "query_failure",
                        "assessment",
                    ]);
                })
                .await
        })
    }
}
