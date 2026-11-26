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
            let mut schema = context.get_schema_builder();
            if schema
                .has_column(user.get_table(), "profile_photo_path")
                .await?
                || schema
                    .has_column(user.get_table(), "profile_photo_disk")
                    .await?
            {
                return Err(openrust::database::DatabaseError::InvalidMigration(
                    "头像列已存在，请核对并手动合并头像迁移，未修改已有列",
                ));
            }
            schema
                .table(user.get_table(), |table| {
                    table.string("profile_photo_path", 2048).nullable();
                    table.string("profile_photo_disk", 255).nullable();
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
                    table.drop_column(&["profile_photo_path", "profile_photo_disk"]);
                })
                .await
        })
    }
}
