//! 经过校验的数据库逻辑表名到物理表名映射。

use std::{collections::BTreeMap, future::Future};

use crate::config::ConfigError;
use crate::core::{Error, Result};
use sea_orm::DbBackend;

tokio::task_local! {
    static CURRENT_TABLE_NAMES: DatabaseTableNames;
}

/// 由查询和迁移共用的不可变表前缀及全名映射。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatabaseTableNames {
    prefix: String,
    mappings: BTreeMap<String, String>,
}

impl DatabaseTableNames {
    /// 创建并校验表名解析器。
    pub fn new(prefix: impl Into<String>, mappings: BTreeMap<String, String>) -> Result<Self> {
        let names = Self {
            prefix: prefix.into(),
            mappings,
        };
        names.validate()?;
        Ok(names)
    }

    /// 返回已配置的表前缀。
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// 将逻辑表名解析为完整的物理表名。
    pub fn resolve(&self, logical_name: &str) -> Result<String> {
        validate_identifier(logical_name, "logical table name")?;
        let physical = self
            .mappings
            .get(logical_name)
            .cloned()
            .unwrap_or_else(|| format!("{}{logical_name}", self.prefix));
        validate_identifier(&physical, "physical table name")?;
        if physical.len() > 64 {
            return Err(invalid_name("physical table name exceeds 64 bytes"));
        }
        Ok(physical)
    }

    /// 为 PostgreSQL、MySQL 或 SQLite 解析并引用物理表名。
    pub fn quote(&self, logical_name: &str, backend: DbBackend) -> Result<String> {
        let physical = self.resolve(logical_name)?;
        match backend {
            DbBackend::Postgres if physical.len() <= 63 => Ok(format!("\"{physical}\"")),
            DbBackend::MySql if physical.len() <= 64 => Ok(format!("`{physical}`")),
            DbBackend::Sqlite => Ok(format!("\"{physical}\"")),
            DbBackend::Postgres => Err(invalid_name(
                "PostgreSQL table names cannot exceed 63 bytes",
            )),
            DbBackend::MySql => Err(invalid_name("MySQL table names cannot exceed 64 bytes")),
            _ => Err(Error::custom(
                "unsupported_database_backend",
                "table-name resolution does not support this SeaORM backend",
            )),
        }
    }

    /// 若当前存在迁移操作，返回该操作作用域内的解析器。
    pub fn current() -> Option<Self> {
        CURRENT_TABLE_NAMES.try_with(Clone::clone).ok()
    }

    pub(crate) fn validate_backend(&self, backend: DbBackend) -> Result<()> {
        self.validate()?;
        let maximum = match backend {
            DbBackend::Postgres => 63,
            DbBackend::MySql | DbBackend::Sqlite => 64,
            _ => {
                return Err(Error::custom(
                    "unsupported_database_backend",
                    "table-name resolution does not support this SeaORM backend",
                ));
            }
        };
        if self.prefix.len() > maximum {
            return Err(invalid_name(format!(
                "database.table_prefix cannot exceed {maximum} bytes for this backend"
            )));
        }
        for logical in self.mappings.keys() {
            self.quote(logical, backend)?;
        }
        for logical in [
            "users",
            "sessions",
            "personal_access_tokens",
            "auth_refresh_tokens",
            "rate_limits",
            "verification_codes",
        ] {
            self.quote(logical, backend)?;
        }
        Ok(())
    }

    /// 运行 future，并使 SeaORM migration 可以访问此解析器。
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT_TABLE_NAMES.scope(self.clone(), future).await
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !self.prefix.is_empty() {
            validate_prefix(&self.prefix)?;
        }
        for (logical, physical) in &self.mappings {
            validate_identifier(logical, "logical table mapping key")?;
            validate_identifier(physical, "physical table mapping value")?;
            if physical.len() > 64 {
                return Err(invalid_name("physical table name exceeds 64 bytes"));
            }
        }
        Ok(())
    }
}

fn validate_prefix(prefix: &str) -> Result<()> {
    let mut bytes = prefix.bytes();
    let first_valid = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
    if !first_valid || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(invalid_name(
            "database.table_prefix must contain only ASCII letters, digits, or underscores and start with a letter or underscore",
        ));
    }
    Ok(())
}

fn validate_identifier(value: &str, kind: &str) -> Result<()> {
    let mut bytes = value.bytes();
    let first_valid = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
    if !first_valid || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(invalid_name(format!(
            "{kind} must contain only ASCII letters, digits, or underscores and start with a letter or underscore"
        )));
    }
    Ok(())
}

fn invalid_name(message: impl Into<String>) -> Error {
    Error::Config(ConfigError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_defaults_empty_prefix_and_full_mapping() -> Result<()> {
        assert_eq!(DatabaseTableNames::default().resolve("users")?, "users");
        assert_eq!(
            DatabaseTableNames::new("", BTreeMap::new())?.resolve("users")?,
            "users"
        );
        let names = DatabaseTableNames::new(
            "shop_",
            BTreeMap::from([("users".to_owned(), "accounts".to_owned())]),
        )?;
        assert_eq!(names.resolve("sessions")?, "shop_sessions");
        assert_eq!(names.resolve("users")?, "accounts");
        Ok(())
    }

    #[test]
    fn rejects_unsafe_names_and_backend_length_overflow() {
        assert!(DatabaseTableNames::new("shop.", BTreeMap::new()).is_err());
        assert!(
            DatabaseTableNames::default()
                .resolve("users; DROP TABLE users")
                .is_err()
        );
        let long = "a".repeat(64);
        let names = DatabaseTableNames::new("", BTreeMap::from([("users".into(), long)])).ok();
        assert!(names.is_some());
        assert!(
            names
                .and_then(|names| names.quote("users", DbBackend::Postgres).ok())
                .is_none()
        );
        let long_prefix = DatabaseTableNames::new("x".repeat(60), BTreeMap::new()).ok();
        assert!(
            long_prefix.is_some_and(|names| names.validate_backend(DbBackend::Postgres).is_err())
        );
    }

    #[test]
    fn quotes_identifiers_for_each_supported_backend() -> Result<()> {
        let names = DatabaseTableNames::new("", BTreeMap::new())?;
        assert_eq!(names.quote("users", DbBackend::Postgres)?, "\"users\"");
        assert_eq!(names.quote("users", DbBackend::MySql)?, "`users`");
        Ok(())
    }

    #[test]
    fn validates_and_quotes_sqlite_names_for_custom_plugins() -> Result<()> {
        let names = DatabaseTableNames::new(
            "custom_",
            BTreeMap::from([("users".to_owned(), "accounts".to_owned())]),
        )?;
        names.validate_backend(DbBackend::Sqlite)?;
        assert_eq!(names.quote("users", DbBackend::Sqlite)?, "\"accounts\"");
        assert_eq!(
            names.quote("sessions", DbBackend::Sqlite)?,
            "\"custom_sessions\""
        );
        Ok(())
    }

    #[tokio::test]
    async fn migration_scope_is_explicit_and_task_local() -> Result<()> {
        let names = DatabaseTableNames::new("test_", BTreeMap::new())?;
        assert!(DatabaseTableNames::current().is_none());
        names
            .scope(async {
                assert_eq!(
                    DatabaseTableNames::current().and_then(|current| current.resolve("users").ok()),
                    Some("test_users".to_owned())
                );
            })
            .await;
        assert!(DatabaseTableNames::current().is_none());
        Ok(())
    }
}
