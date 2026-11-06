//! 固定 Laravel 数据库 Session 表；增强的绝对截止由显式迁移扩展登记。

use crate::database::{
    DatabaseTableNames,
    sea_orm::{
        DbBackend,
        sea_query::{Alias, SqliteQueryBuilder},
    },
    sea_orm_migration::prelude::*,
};
use sha2::{Digest, Sha256};

/// 原生 Session 表，对应固定上游 session database.stub。
pub struct SessionMigration;

#[derive(DeriveIden)]
pub(super) enum Columns {
    Id,
    UserId,
    IpAddress,
    UserAgent,
    Payload,
    LastActivity,
}

pub(super) fn table_name(logical: &str) -> Result<String, DbErr> {
    DatabaseTableNames::current()
        .ok_or_else(|| DbErr::Migration("Session 迁移需要应用表名作用域".into()))?
        .resolve(logical)
        .map_err(|error| DbErr::Migration(error.to_string()))
}

pub(super) fn index_name(logical: &str, kind: &str) -> Result<String, DbErr> {
    let column = match kind {
        "user" => "user_id",
        "activity" => "last_activity",
        _ => return Err(DbErr::Migration("未知原生 Session 索引类型".into())),
    };
    Ok(blueprint_index_name(&table_name(logical)?, column))
}

// 固定 Blueprint.php:1826-1838：实际表名+列+index，ASCII 小写后替换 - 和 .。
fn blueprint_index_name(physical: &str, column: &str) -> String {
    format!("{physical}_{column}_index")
        .to_ascii_lowercase()
        .replace(['-', '.'], "_")
}

impl SessionMigration {
    /// 为应用显式配置的逻辑表生成相同原生结构。
    pub fn for_table(table: impl Into<String>) -> SessionTableMigration {
        let table = table.into();
        let digest = format!("{:x}", Sha256::digest(table.as_bytes()));
        SessionTableMigration {
            version: format!("m20261103_000001_session_{}", &digest[..24]),
            table,
        }
    }
}

/// 使用应用逻辑表名的原生 Session 迁移；不隐式改写旧摘要表的数据。
pub struct SessionTableMigration {
    table: String,
    version: String,
}

impl MigrationName for SessionMigration {
    fn name(&self) -> &'static str {
        "m20261103_000001_openrust_database_sessions"
    }
}

impl MigrationName for SessionTableMigration {
    fn name(&self) -> &str {
        &self.version
    }
}

async fn create(manager: &SchemaManager<'_>, logical: &str) -> Result<(), DbErr> {
    let table = table_name(logical)?;
    if manager.has_table(&table).await? && manager.has_column(&table, "id_hash").await? {
        return Err(DbErr::Migration(
            "旧 Session 摘要表须显式迁移；原始 ID 无法由摘要恢复，不会覆盖原数据".into(),
        ));
    }
    let mut payload = ColumnDef::new(Columns::Payload);
    if manager.get_database_backend() == crate::database::sea_orm::DbBackend::MySql {
        payload.custom("LONGTEXT");
    } else {
        payload.text();
    }
    manager
        .create_table(
            Table::create()
                .table(Alias::new(&table))
                .col(
                    ColumnDef::new(Columns::Id)
                        .string()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(Columns::UserId).big_unsigned().null())
                .col(ColumnDef::new(Columns::IpAddress).string_len(45).null())
                .col(ColumnDef::new(Columns::UserAgent).text().null())
                .col(payload.not_null())
                .col(ColumnDef::new(Columns::LastActivity).integer().not_null())
                .to_owned(),
        )
        .await?;
    for (kind, column) in [
        ("user", Columns::UserId),
        ("activity", Columns::LastActivity),
    ] {
        create_index(manager, logical, kind, column).await?;
    }
    Ok(())
}

async fn create_index(
    manager: &SchemaManager<'_>,
    logical: &str,
    kind: &str,
    column: Columns,
) -> Result<(), DbErr> {
    let statement = Index::create()
        .name(index_name(logical, kind)?)
        .table(Alias::new(table_name(logical)?))
        .col(column)
        .to_owned();
    if manager.get_database_backend() == DbBackend::Sqlite {
        return execute_sqlite_index(manager, statement.to_string(SqliteQueryBuilder)).await;
    }
    manager.create_index(statement).await
}

pub(super) async fn execute_sqlite_index(
    manager: &SchemaManager<'_>,
    ddl: String,
) -> Result<(), DbErr> {
    // 必须在同一物理连接先 step schema 读取，再 prepare DDL；另一连接 DROP 后的
    // 旧 schema 可在 CREATE 的 prepare 阶段误报 already exists，尚未进入 cookie 校验。
    // execute_unprepared 在固定 SQLx 链上顺序执行全串；不重试、不忽略冲突、不另包事务。
    manager
        .get_connection()
        .execute_unprepared(&format!("SELECT name FROM sqlite_master LIMIT 1; {ddl}"))
        .await
        .map(|_| ())
}

async fn drop_table(manager: &SchemaManager<'_>, logical: &str) -> Result<(), DbErr> {
    manager
        .drop_table(
            Table::drop()
                .table(Alias::new(table_name(logical)?))
                .if_exists()
                .to_owned(),
        )
        .await
}

#[async_trait::async_trait]
impl MigrationTrait for SessionMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create(manager, "sessions").await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, "sessions").await
    }
}

#[async_trait::async_trait]
impl MigrationTrait for SessionTableMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create(manager, &self.table).await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, &self.table).await
    }
}

/// 现有原生表的活动索引升级；基础迁移已创建时不会重复添加。
pub struct SessionGcMigration;
impl MigrationName for SessionGcMigration {
    fn name(&self) -> &'static str {
        "m20261103_000002_openrust_session_activity_index"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for SessionGcMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let name = index_name("sessions", "activity")?;
        if !manager.has_index(&table_name("sessions")?, &name).await? {
            create_index(manager, "sessions", "activity", Columns::LastActivity).await?;
        }
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let name = index_name("sessions", "activity")?;
        if manager.has_index(&table_name("sessions")?, &name).await? {
            let statement = Index::drop()
                .name(name)
                .table(Alias::new(table_name("sessions")?))
                .to_owned();
            if manager.get_database_backend() == DbBackend::Sqlite {
                execute_sqlite_index(manager, statement.to_string(SqliteQueryBuilder)).await?;
            } else {
                manager.drop_index(statement).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn native_index_names_follow_resolved_table_prefix_mapping_and_ascii_case()
    -> crate::Result<()> {
        for (prefix, mappings, user, activity) in [
            (
                "",
                BTreeMap::new(),
                "sessions_user_id_index",
                "sessions_last_activity_index",
            ),
            (
                "app_",
                BTreeMap::new(),
                "app_sessions_user_id_index",
                "app_sessions_last_activity_index",
            ),
            (
                "ignored_",
                BTreeMap::from([("sessions".into(), "MappedSessions".into())]),
                "mappedsessions_user_id_index",
                "mappedsessions_last_activity_index",
            ),
            (
                "UPPER_",
                BTreeMap::new(),
                "upper_sessions_user_id_index",
                "upper_sessions_last_activity_index",
            ),
        ] {
            let names = DatabaseTableNames::new(prefix, mappings)?;
            let actual = names
                .scope(async {
                    Ok::<_, DbErr>((
                        index_name("sessions", "user")?,
                        index_name("sessions", "activity")?,
                    ))
                })
                .await
                .map_err(crate::Error::internal)?;
            assert_eq!(actual, (user.to_owned(), activity.to_owned()));
        }
        Ok(())
    }

    #[test]
    fn blueprint_index_normalization_preserves_upstream_separator_rule() {
        assert_eq!(
            blueprint_index_name("Main.Mixed-Sessions", "user_id"),
            "main_mixed_sessions_user_id_index"
        );
        assert_eq!(
            blueprint_index_name("Sessions", "last_activity"),
            "sessions_last_activity_index"
        );
    }
}
