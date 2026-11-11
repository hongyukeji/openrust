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
            let mut schema = connection.getSchemaBuilder();
            schema
                .create("users", |table| {
                    table.id();
                    table.string("name", 255);
                    table.string("email", 255).unique();
                    table.timestamp("email_verified_at", 0).nullable();
                    table.string("password", 255);
                    table.string("remember_token", 100).nullable();
                    table.timestamps();
                })
                .await?;
            schema
                .create("password_reset_tokens", |table| {
                    table.string("email", 255).primary();
                    table.string("token", 255);
                    table.timestamp("created_at", 0).nullable();
                })
                .await?;
            schema
                .create("sessions", |table| {
                    table.string("id", 255).primary();
                    table.foreignId("user_id").nullable();
                    table.index(&["user_id"], None);
                    table.string("ip_address", 45).nullable();
                    table.text("user_agent").nullable();
                    table.longText("payload");
                    table.integer("last_activity").index();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = connection.getSchemaBuilder();
            schema.dropIfExists("sessions").await?;
            schema.dropIfExists("password_reset_tokens").await?;
            schema.dropIfExists("users").await
        })
    }
}
