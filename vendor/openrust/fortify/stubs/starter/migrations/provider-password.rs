use crate::app::models::User;
use openrust::{
    contracts::auth::Authenticatable,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        eloquent::Model,
        migrations::{Migration, MigrationContext},
    },
};
pub struct Definition;
impl Definition {
    async fn change(context: &mut MigrationContext, nullable: bool) -> DatabaseResult<()> {
        if !matches!(
            context.get_driver_name(),
            "sqlite" | "mysql" | "mariadb" | "pgsql"
        ) {
            return Err(DatabaseError::InvalidMigration(
                "此方言的密码列请手动合并可空迁移，避免更改自定义长度或约束；未修改原列",
            ));
        }
        let user = User::from_state(Default::default());
        let password = user.get_auth_password_name();
        if !nullable
            && let Some(query) = context.query_table(user.get_table())
            && query.where_null(password).exists().await?
        {
            return Err(DatabaseError::InvalidMigration(
                "仍有无本地密码账户，请先建立本地密码后再回滚；身份表与迁移账本保留",
            ));
        }
        let column = context
            .get_schema_builder()
            .get_columns(user.get_table())
            .await?
            .into_iter()
            .find(|column| column.name == password)
            .ok_or(DatabaseError::InvalidMigration(
                "用户密码列不存在，请手动合并第三方登录迁移",
            ))?;
        if column.nullable == nullable
            || column.auto_increment
            || column.primary
            || column.generation.is_some()
            || column.default.is_some()
            || !matches!(
                column.type_name.to_ascii_lowercase().as_str(),
                "varchar" | "character varying" | "nvarchar" | "text"
            )
        {
            return Err(DatabaseError::InvalidMigration(
                "密码列已自定义，请手动合并此迁移以保留原列契约；未修改已有列",
            ));
        }
        context
            .get_schema_builder()
            .table(user.get_table(), |table| {
                let changed = table
                    .raw_column(password, column.full_type)
                    .nullable_with(nullable)
                    .change();
                if let Some(comment) = &column.comment {
                    changed.comment(comment);
                }
                if let Some(collation) = &column.collation {
                    changed.collation(collation);
                }
            })
            .await
    }
}
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
        Box::pin(Self::change(context, true))
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(Self::change(context, false))
    }
}
