use openrust::localization::__;
// Translated from the framework Fortify 1.43.0 (MIT, Taylor Otwell).
use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct Definition;
impl Migration for Definition {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection
                .get_schema_builder()
                .table("users", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.users", &Default::default())?);
                    table
                        .text("two_factor_secret")
                        .after("password")
                        .nullable()
                        .comment(__("schema.columns.two_factor_secret", &Default::default())?);
                    table
                        .text("two_factor_recovery_codes")
                        .after("two_factor_secret")
                        .nullable()
                        .comment(__(
                            "schema.columns.two_factor_recovery_codes",
                            &Default::default(),
                        )?);
                    table
                        .timestamp("two_factor_confirmed_at", None)
                        .after("two_factor_recovery_codes")
                        .nullable()
                        .comment(__(
                            "schema.columns.two_factor_confirmed_at",
                            &Default::default(),
                        )?);
                    Ok(())
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection
                .get_schema_builder()
                .table("users", |table| {
                    table.drop_column(&[
                        "two_factor_secret",
                        "two_factor_recovery_codes",
                        "two_factor_confirmed_at",
                    ]);
                })
                .await
        })
    }
}
