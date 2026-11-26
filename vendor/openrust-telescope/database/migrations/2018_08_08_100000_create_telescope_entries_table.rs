use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateTelescopeEntriesTable;
pub(crate) static MIGRATION: CreateTelescopeEntriesTable = CreateTelescopeEntriesTable;
impl Migration for CreateTelescopeEntriesTable {
    fn get_connection(
        &self,
        config: &dyn openrust::contracts::config::Repository,
    ) -> DatabaseResult<Option<String>> {
        match config.get("telescope.storage.database.connection") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(name)) if !name.is_empty() => Ok(Some(name)),
            _ => Err(openrust::database::DatabaseError::InvalidConfiguration(
                "telescope.storage.database.connection",
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
                .create("telescope_entries", |table| {
                    table.big_increments("sequence");
                    table.uuid("uuid").unique();
                    table.uuid("batch_id").index();
                    table.string("family_hash", 255).nullable().index();
                    table.boolean("should_display_on_index").default(true);
                    table.string("type", 20);
                    table.long_text("content");
                    table.date_time("created_at", None).nullable().index();
                    table.index(&["type", "should_display_on_index"], None);
                })
                .await?;
            schema
                .create("telescope_entries_tags", |table| {
                    table.uuid("entry_uuid");
                    table.string("tag", 255);
                    table.primary(&["entry_uuid", "tag"], None);
                    table.index(&["tag"], None);
                    table
                        .foreign(&["entry_uuid"], None)
                        .references("uuid")
                        .on("telescope_entries")
                        .cascade_on_delete();
                })
                .await?;
            schema
                .create("telescope_monitoring", |table| {
                    table.string("tag", 255).primary();
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
            schema.drop_if_exists("telescope_entries_tags").await?;
            schema.drop_if_exists("telescope_entries").await?;
            schema.drop_if_exists("telescope_monitoring").await
        })
    }
}
