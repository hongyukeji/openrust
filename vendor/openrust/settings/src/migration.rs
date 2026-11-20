use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

/// Register with the existing migrator; this never runs during settings reads.
pub struct CreateSettingsTable {
    table: &'static str,
}
impl CreateSettingsTable {
    pub const fn new(table: &'static str) -> Self {
        Self { table }
    }
}
impl Default for CreateSettingsTable {
    fn default() -> Self {
        Self::new("settings")
    }
}
impl Migration for CreateSettingsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .getSchemaBuilder()
                .create(self.table, |table| {
                    table.id();
                    table.string("group", None);
                    table.string("name", None);
                    table.boolean("locked").default(false);
                    table.json("payload");
                    table.timestamps();
                    table.unique(&["group", "name"], None);
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move { context.getSchemaBuilder().dropIfExists(self.table).await })
    }
}
