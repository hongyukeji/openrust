use openrust::translation::__;
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
                .create("passkeys", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.passkeys", &Default::default())?);
                    table
                        .id()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .foreignIdFor::<crate::app::models::User>(Some("user_id"))
                        .comment(__("schema.columns.user_id", &Default::default())?)
                        .constrained()
                        .cascadeOnDelete();
                    table
                        .string("name", None)
                        .comment(__("schema.columns.name", &Default::default())?);
                    table
                        .string("credential_id", None)
                        .unique()
                        .comment(__("schema.columns.credential_id", &Default::default())?);
                    table
                        .json("credential")
                        .comment(__("schema.columns.credential", &Default::default())?);
                    table
                        .timestamp("last_used_at", None)
                        .nullable()
                        .comment(__("schema.columns.last_used_at", &Default::default())?);
                    table
                        .timestamp("created_at", None)
                        .nullable()
                        .comment(__("schema.columns.created_at", &Default::default())?);
                    table
                        .timestamp("updated_at", None)
                        .nullable()
                        .comment(__("schema.columns.updated_at", &Default::default())?);
                    table.index(&["user_id"], None);
                    Ok(())
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
