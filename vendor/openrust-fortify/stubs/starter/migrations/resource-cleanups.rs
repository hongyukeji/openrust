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
                .create("user_resource_cleanups", |table| {
                    table.string("id", 64).primary();
                    table.string("scope", 64);
                    table.string("owner_hash", 64);
                    table.string("receipt_hash", 64);
                    table.string("kind", 32);
                    table.text("resources");
                    table.string("status", 16);
                    table.big_integer("created_at");
                    table.big_integer("updated_at");
                    table.big_integer("expires_at");
                    // 显式短名称保留表前缀隔离，避免MySQL自动索引名超过64字符。
                    let prefix = table.get_prefix().to_owned();
                    table.index(
                        &["owner_hash"],
                        Some(&format!("{prefix}usr_cleanup_owner_idx")),
                    );
                    table.index(
                        &["status"],
                        Some(&format!("{prefix}usr_cleanup_status_idx")),
                    );
                    table.index(
                        &["expires_at"],
                        Some(&format!("{prefix}usr_cleanup_expiry_idx")),
                    );
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
                .drop_if_exists("user_resource_cleanups")
                .await
        })
    }
}
