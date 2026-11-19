// Translated from Laravel Fortify 1.43.0 (MIT, Taylor Otwell).
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
                .getSchemaBuilder()
                .table("users", |table| {
                    table.text("two_factor_secret").after("password").nullable();
                    table
                        .text("two_factor_recovery_codes")
                        .after("two_factor_secret")
                        .nullable();
                    table
                        .timestamp("two_factor_confirmed_at", None)
                        .after("two_factor_recovery_codes")
                        .nullable();
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
                .getSchemaBuilder()
                .table("users", |table| {
                    table.dropColumn(&[
                        "two_factor_secret",
                        "two_factor_recovery_codes",
                        "two_factor_confirmed_at",
                    ]);
                })
                .await
        })
    }
}
