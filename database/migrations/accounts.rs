//! 应用账户的可选登录标识；不改写已执行的框架基础迁移。

use openrust::database::{
    DatabaseTableNames, dialect,
    sea_orm::{ConnectionTrait, DbBackend, Statement, sea_query::Alias},
    sea_orm_migration::prelude::*,
};

/// 应用选择用户名、邮箱或手机号登录时使用的结构升级。
pub struct AccountIdentifiers;

impl MigrationName for AccountIdentifiers {
    fn name(&self) -> &'static str {
        "m20261105_000010_application_account_identifiers"
    }
}

fn users() -> Result<Alias, DbErr> {
    DatabaseTableNames::current()
        .ok_or_else(|| DbErr::Migration("账户升级需要标准迁移表名作用域".into()))?
        .resolve("users")
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

fn index_name(column: &str) -> Result<String, DbErr> {
    DatabaseTableNames::current()
        .ok_or_else(|| DbErr::Migration("账户升级需要标准迁移表名作用域".into()))?
        .resolve(&format!("application_users_{column}_unique"))
        .map_err(|error| DbErr::Migration(error.to_string()))
}

async fn validate_owned_email_definition(
    manager: &SchemaManager<'_>,
    table: &Alias,
) -> Result<(), DbErr> {
    if manager.get_database_backend() != DbBackend::MySql {
        return Ok(());
    }
    // MySQL MODIFY必须重述完整列定义。这里只升级本应用原有默认形状；
    // 自定义collation/default/comment等必须由应用显式迁移，不能静默重置。
    let row = manager.get_connection().query_one_raw(Statement::from_sql_and_values(
        DbBackend::MySql,
        "SELECT c.COLUMN_TYPE AS column_type,c.COLUMN_DEFAULT AS column_default,c.EXTRA AS extra,c.COLUMN_COMMENT AS comment,c.COLLATION_NAME AS column_collation,t.TABLE_COLLATION AS table_collation FROM information_schema.COLUMNS c JOIN information_schema.TABLES t ON t.TABLE_SCHEMA=c.TABLE_SCHEMA AND t.TABLE_NAME=c.TABLE_NAME WHERE c.TABLE_SCHEMA=DATABASE() AND c.TABLE_NAME=? AND c.COLUMN_NAME='email'",
        [table.to_string().into()],
    )).await?.ok_or_else(|| DbErr::Migration("应用账户缺少email列".into()))?;
    if row.try_get::<String>("", "column_type")? != "varchar(320)"
        || row
            .try_get::<Option<String>>("", "column_default")?
            .is_some()
        || !row.try_get::<String>("", "extra")?.is_empty()
        || !row.try_get::<String>("", "comment")?.is_empty()
        || row.try_get::<Option<String>>("", "column_collation")?
            != row.try_get::<Option<String>>("", "table_collation")?
    {
        return Err(DbErr::Migration(
            "email列已有自定义属性，请提供保全属性的显式应用升级".into(),
        ));
    }
    Ok(())
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for AccountIdentifiers {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = users()?;
        validate_owned_email_definition(manager, &table).await?;
        dialect::set_column_nullable(
            manager,
            table.clone(),
            ColumnDef::new(Alias::new("email")).string_len(320).take(),
            true,
        )
        .await?;
        for (column, length) in [("username", 255), ("phone", 32)] {
            manager
                .alter_table(
                    Table::alter()
                        .table(table.clone())
                        .add_column(ColumnDef::new(Alias::new(column)).string_len(length).null())
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .name(index_name(column)?)
                        .table(table.clone())
                        .col(Alias::new(column))
                        .unique()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = users()?;
        validate_owned_email_definition(manager, &table).await?;
        let backend = manager.get_database_backend();
        let query = Query::select()
            .expr(Expr::cust("1"))
            .from(table.clone())
            .cond_where(
                Cond::any()
                    .add(Expr::col(Alias::new("email")).is_null())
                    .add(Expr::col(Alias::new("username")).is_not_null())
                    .add(Expr::col(Alias::new("phone")).is_not_null()),
            )
            .limit(1)
            .to_owned();
        if manager
            .get_connection()
            .query_one_raw(backend.build(&query))
            .await?
            .is_some()
        {
            return Err(DbErr::Migration(
                "账户包含空邮箱或新增登录标识，不能无损回退；请先由应用显式迁出数据".into(),
            ));
        }
        // 先收紧邮箱，失败时不先删掉其他标识；不填假邮箱以通过回滚。
        dialect::set_column_nullable(
            manager,
            table.clone(),
            ColumnDef::new(Alias::new("email")).string_len(320).take(),
            false,
        )
        .await?;
        for column in ["phone", "username"] {
            manager
                .drop_index(
                    Index::drop()
                        .name(index_name(column)?)
                        .table(table.clone())
                        .to_owned(),
                )
                .await?;
            dialect::drop_column(manager, table.clone(), Alias::new(column)).await?;
        }
        Ok(())
    }
}
