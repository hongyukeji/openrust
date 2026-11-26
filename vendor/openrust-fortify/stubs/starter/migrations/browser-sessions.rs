use crate::app::models::User;
use openrust::database::{
    DatabaseFuture, DatabaseResult,
    eloquent::Model,
    migrations::{Migration, MigrationContext},
};

pub struct Definition;
impl Migration for Definition {
    fn get_connection(
        &self,
        _: &dyn openrust::contracts::config::Repository,
    ) -> DatabaseResult<Option<String>> {
        Ok(User::from_state(Default::default())
            .get_connection_name()
            .map(str::to_owned))
    }
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("user_browser_sessions", |table| {
                    table.string("id", 64).primary();
                    table.string("owner_hash", 64).index();
                    table.text("session_id");
                    table.string("ip_address", 45).nullable();
                    table.text("user_agent").nullable();
                    table.big_integer("last_seen");
                    table.big_integer("expires_at").index();
                    table.big_integer("revoked_at").nullable();
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .drop_if_exists("user_browser_sessions")
                .await
        })
    }
}
