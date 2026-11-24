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
            let user = User::from_state(Default::default());
            context
                .get_schema_builder()
                .table(user.get_table(), |table| {
                    table.big_integer("credential_version").default(0);
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let user = User::from_state(Default::default());
            context
                .get_schema_builder()
                .table(user.get_table(), |table| {
                    table.drop_column(&["credential_version"]);
                })
                .await
        })
    }
}
