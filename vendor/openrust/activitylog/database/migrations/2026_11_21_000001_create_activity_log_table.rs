use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct CreateActivityLogTable;
pub(crate) static MIGRATION: CreateActivityLogTable = CreateActivityLogTable;
impl Migration for CreateActivityLogTable {
    fn up<'a>(&'a self, c: &'a mut MigrationContext) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            c.get_schema_builder()
                .create("activity_log", |t| {
                    t.id();
                    t.string("log_name", 255).nullable().index();
                    t.text("description");
                    t.string("event", 255).nullable();
                    t.nullable_morphs("subject", Some("subject"), None);
                    t.nullable_morphs("causer", Some("causer"), None);
                    t.string("request_id", 255).nullable().index();
                    t.json("properties").nullable();
                    t.json("attribute_changes").nullable();
                    t.timestamps();
                })
                .await
        })
    }
    fn down<'a>(&'a self, c: &'a mut MigrationContext) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move { c.get_schema_builder().drop_if_exists("activity_log").await })
    }
}
