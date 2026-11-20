// Translated from Laravel Passkeys 0.3.0 (MIT).
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
                .create("passkeys", |table| {
                    table.id();
                    table
                        .foreignIdFor::<crate::app::models::User>(Some("user_id"))
                        .constrained()
                        .cascadeOnDelete();
                    table.string("name", None);
                    table.string("credential_id", None).unique();
                    table.json("credential");
                    table.timestamp("last_used_at", None).nullable();
                    table.timestamps();
                    table.index(&["user_id"], None);
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move { connection.getSchemaBuilder().dropIfExists("passkeys").await })
    }
}
