//! 在模型既有账户表追加官方三字段；不迁移或删除旧MFA保护数据。

use sha2::{Digest, Sha256};

use crate::database::{
    DatabaseTableNames, dialect,
    sea_orm::{DbBackend, sea_query::Alias},
    sea_orm_migration::prelude::*,
};

const VERSION: &str = "m20261105_000009_openrust_fortify_two_factor";

const SECRET: &str = "two_factor_secret";
const RECOVERY: &str = "two_factor_recovery_codes";
const CONFIRMED: &str = "two_factor_confirmed_at";

/// 为现有账户模型增加密文secret、密文恢复码及可空确认时间。
///
/// 默认逻辑表为users；自定义逻辑表按稳定摘要取得独立版本名，可与默认迁移并存。
/// 旧MFA的哈希恢复码不能逆向还原，本迁移只增三列，不搬数据或关闭旧保护。
#[derive(Clone, Debug)]
pub struct FortifyTwoFactorMigration {
    table: String,
    version: String,
}

impl FortifyTwoFactorMigration {
    /// 创建默认users账户表的迁移，保留固定迁移版本名。
    #[must_use]
    pub fn new() -> Self {
        Self {
            table: "users".into(),
            version: VERSION.into(),
        }
    }

    /// 选择模型现有的逻辑账户表；名称校验复用同一表名解析器。
    ///
    /// 非默认表的版本名追加逻辑表名SHA-256全摘要；相同逻辑表重复登记仍会冲突。
    pub fn with_table(logical_table: impl Into<String>) -> crate::Result<Self> {
        let table = logical_table.into();
        DatabaseTableNames::default().resolve(&table)?;
        let version = if table == "users" {
            VERSION.to_owned()
        } else {
            format!("{VERSION}_{:x}", Sha256::digest(table.as_bytes()))
        };
        Ok(Self { table, version })
    }

    fn table(&self, backend: DbBackend) -> Result<Alias, DbErr> {
        let names = DatabaseTableNames::current()
            .ok_or_else(|| DbErr::Migration("Fortify账户迁移需要标准迁移表名作用域".into()))?;
        names
            .quote(&self.table, backend)
            .map_err(|error| DbErr::Migration(error.to_string()))?;
        names
            .resolve(&self.table)
            .map(Alias::new)
            .map_err(|error| DbErr::Migration(error.to_string()))
    }
}

impl Default for FortifyTwoFactorMigration {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationName for FortifyTwoFactorMigration {
    fn name(&self) -> &str {
        &self.version
    }
}

#[crate::database::sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for FortifyTwoFactorMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let table = self.table(backend)?;
        // SQLite只允许每条ALTER追加一列；沿同一迁移执行所有者顺序提交字段操作。
        let columns = [
            ColumnDef::new(Alias::new(SECRET)).text().null().take(),
            ColumnDef::new(Alias::new(RECOVERY)).text().null().take(),
            dialect::timestamp_utc(Alias::new(CONFIRMED), backend)?
                .null()
                .take(),
        ];
        for column in columns {
            manager
                .alter_table(
                    Table::alter()
                        .table(table.clone())
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let table = self.table(manager.get_database_backend())?;
        for column in [CONFIRMED, RECOVERY, SECRET] {
            dialect::drop_column(manager, table.clone(), Alias::new(column)).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_custom_tables_have_stable_distinct_versions() -> crate::Result<()> {
        let default = FortifyTwoFactorMigration::new();
        let users = FortifyTwoFactorMigration::with_table("users")?;
        let customers = FortifyTwoFactorMigration::with_table("customers")?;
        let repeated = FortifyTwoFactorMigration::with_table("customers")?;
        let operators = FortifyTwoFactorMigration::with_table("operators")?;
        assert_eq!(default.name(), VERSION);
        assert_eq!(users.name(), VERSION);
        assert_eq!(customers.name(), repeated.name());
        assert_ne!(customers.name(), operators.name());
        assert_ne!(customers.name(), default.name());
        assert_eq!(
            customers.name(),
            format!("{VERSION}_{:x}", Sha256::digest(b"customers"))
        );
        Ok(())
    }

    #[test]
    fn custom_table_rejects_untrusted_identifier_syntax() {
        for table in [
            "",
            "accounts; DROP TABLE users",
            "accounts.user",
            "accounts\"",
            "../users",
        ] {
            assert!(FortifyTwoFactorMigration::with_table(table).is_err());
        }
    }
}
