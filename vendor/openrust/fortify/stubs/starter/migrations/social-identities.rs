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
            // 显式短名称保留连接前缀，兼容MySQL长度限制及PostgreSQL同Schema并行表。
            let index = format!(
                "{}usi_owner_provider",
                User::from_state(Default::default())
                    .get_connection()?
                    .get_table_prefix()
            );
            context
                .get_schema_builder()
                .create("user_social_identities", |table| {
                    table.string("id", 64).primary();
                    table.string("provider", 64);
                    table.string("owner_hash", 64);
                    table.text("payload");
                    table.big_integer("created_at");
                    table.unique(&["owner_hash", "provider"], Some(&index));
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
                .drop_if_exists("user_social_identities")
                .await
        })
    }
}
