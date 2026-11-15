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
                .create("personal_access_tokens", |table| {
                    table.id();
                    table.string("tokenable_type", 255);
                    table.unsignedBigInteger("tokenable_id");
                    table.index(&["tokenable_type", "tokenable_id"], None);
                    table.text("name");
                    table.string("token", 64).unique();
                    table.text("abilities").nullable();
                    table.timestamp("last_used_at", 0).nullable();
                    table.timestamp("expires_at", 0).nullable().index();
                    table.timestamps();
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
                .dropIfExists("personal_access_tokens")
                .await
        })
    }
}
