//! 翻译来源迁移；表名使用框架同一迁移作用域，不嵌入默认连接或业务模块。
use openrust::database::{
    DatabaseTableNames,
    sea_orm::DbBackend,
    sea_orm_migration::{self, prelude::*},
};

/// 创建翻译行与唯一修订行；应用通过标准迁移Provider贡献执行。
#[derive(Debug, Default, Clone, Copy)]
pub struct TranslationSourceMigration;
impl MigrationName for TranslationSourceMigration {
    fn name(&self) -> &'static str {
        "m20261106_000001_openrust_translation_source"
    }
}
fn table(name: &str, backend: DbBackend) -> Result<Alias, DbErr> {
    let names = DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration("translation source requires a migration table-name scope".into())
    })?;
    names
        .quote(name, backend)
        .map_err(|_| DbErr::Migration("translation source table name invalid".into()))?;
    names
        .resolve(name)
        .map(Alias::new)
        .map_err(|_| DbErr::Migration("translation source table name invalid".into()))
}
#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for TranslationSourceMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let mut lines = Table::create();
        lines
            .table(table("translation_lines", backend)?)
            .if_not_exists()
            .col(
                ColumnDef::new(Alias::new("identity_hash"))
                    .string_len(64)
                    .not_null()
                    .primary_key(),
            );
        for (name, length) in [
            ("locale", 128),
            ("group_name", 255),
            ("namespace", 255),
            ("line_key", 512),
        ] {
            lines.col(
                ColumnDef::new(Alias::new(name))
                    .string_len(length)
                    .not_null(),
            );
        }
        lines
            .col(ColumnDef::new(Alias::new("value_json")).text().not_null())
            .col(ColumnDef::new(Alias::new("enabled")).boolean().not_null());
        manager.create_table(lines.clone()).await?;
        manager
            .create_table(
                Table::create()
                    .table(table("translation_revisions", backend)?)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("revision"))
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        // 重复up只补缺失的种子，不重置现有修订或修改已存在的消息。
        manager
            .exec_stmt(
                Query::insert()
                    .into_table(table("translation_revisions", backend)?)
                    .columns([Alias::new("id"), Alias::new("revision")])
                    .values([1_i32.into(), 0_i64.into()])
                    .map_err(|_| DbErr::Migration("translation revision seed invalid".into()))?
                    .on_conflict(
                        OnConflict::column(Alias::new("id"))
                            .do_nothing_on([Alias::new("id")])
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for name in ["translation_lines", "translation_revisions"] {
            manager
                .drop_table(
                    Table::drop()
                        .table(table(name, manager.get_database_backend())?)
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
