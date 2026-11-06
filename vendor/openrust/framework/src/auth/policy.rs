//! 基于数据库的角色与精确权限检查。

use std::fmt;

use crate::core::{Error, Result};
use crate::database::sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement};
use crate::database::{DatabaseConnection, Db};

use crate::auth::CurrentUser;

/// 通过共享数据库执行的角色与权限操作。
#[derive(Clone)]
pub struct Policy {
    db: Db,
    guard_scope: Option<(String, Option<String>)>,
}

impl fmt::Debug for Policy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Policy")
    }
}

impl Policy {
    /// 使用应用共享数据库创建 policy 服务。
    pub fn new(db: Db) -> Self {
        Self {
            db,
            guard_scope: None,
        }
    }

    /// 显式将本角色数据库绑定到具名Guard/Provider，避免同值标识跨账户来源授权。
    #[must_use]
    pub fn with_guard_scope(mut self, guard: impl Into<String>, provider: Option<String>) -> Self {
        self.guard_scope = Some((guard.into(), provider));
        self
    }

    /// 确保指定全局名称的角色存在，并返回其数据库 ID。
    pub async fn ensure_role(&self, name: &str) -> Result<i64> {
        validate_name(name, "role")?;
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        if backend == DbBackend::Sqlite {
            let db = self.db.clone();
            let name = name.trim().to_owned();
            return self
                .db
                .transaction_for_update(move |transaction| {
                    Box::pin(async move {
                        let insert = crate::auth::resolve_auth_sql(
                            &db,
                            "INSERT INTO tb_roles (name) VALUES (?) ON CONFLICT (name) DO NOTHING",
                        )?;
                        transaction
                            .execute_raw(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                insert,
                                [name.clone().into()],
                            ))
                            .await
                            .map_err(Error::internal)?;
                        let select = crate::auth::resolve_auth_sql(
                            &db,
                            "SELECT id FROM tb_roles WHERE name=?",
                        )?;
                        let row = transaction
                            .query_one_raw(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                select,
                                [name.into()],
                            ))
                            .await
                            .map_err(Error::internal)?
                            .ok_or_else(|| {
                                Error::internal(std::io::Error::other(
                                    "role insert was not visible",
                                ))
                            })?;
                        row.try_get::<i64>("", "id").map_err(Error::internal)
                    })
                })
                .await;
        }
        let insert = match backend {
            DbBackend::Postgres => {
                "INSERT INTO tb_roles (name) VALUES ($1) ON CONFLICT (name) DO NOTHING"
            }
            DbBackend::MySql => {
                "INSERT INTO tb_roles (name) VALUES (?) ON DUPLICATE KEY UPDATE name = VALUES(name)"
            }
            _ => return Err(unsupported_backend()),
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, insert)?,
                [name.trim().into()],
            ))
            .await
            .map_err(Error::internal)?;
        let select = match backend {
            DbBackend::Postgres => "SELECT id FROM tb_roles WHERE name = $1",
            DbBackend::MySql | DbBackend::Sqlite => "SELECT id FROM tb_roles WHERE name = ?",
            _ => return Err(unsupported_backend()),
        };
        let row = connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, select)?,
                [name.trim().into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::internal(std::io::Error::other("role insert was not visible")))?;
        row.try_get::<i64>("", "id").map_err(Error::internal)
    }

    /// 为现有用户分配现有角色；重复分配具有幂等性。
    pub async fn assign_role(&self, user_id: i64, role_id: i64) -> Result<()> {
        validate_ids(user_id, role_id)?;
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "INSERT INTO tb_user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT (user_id, role_id) DO NOTHING"
            }
            DbBackend::MySql => {
                "INSERT INTO tb_user_roles (user_id, role_id) VALUES (?, ?) ON DUPLICATE KEY UPDATE role_id = VALUES(role_id)"
            }
            DbBackend::Sqlite => {
                "INSERT INTO tb_user_roles (user_id, role_id) VALUES (?, ?) ON CONFLICT (user_id, role_id) DO NOTHING"
            }
            _ => return Err(unsupported_backend()),
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [user_id.into(), role_id.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(())
    }

    /// 移除用户的角色分配。
    pub async fn revoke_role(&self, user_id: i64, role_id: i64) -> Result<()> {
        validate_ids(user_id, role_id)?;
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => "DELETE FROM tb_user_roles WHERE user_id = $1 AND role_id = $2",
            DbBackend::MySql | DbBackend::Sqlite => {
                "DELETE FROM tb_user_roles WHERE user_id = ? AND role_id = ?"
            }
            _ => return Err(unsupported_backend()),
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [user_id.into(), role_id.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(())
    }

    /// 确保权限存在，然后授予现有角色。
    pub async fn grant_permission(&self, role_id: i64, permission: &str) -> Result<()> {
        if role_id <= 0 {
            return Err(Error::BadRequest("role ID must be positive".into()));
        }
        validate_name(permission, "permission")?;

        let db = self.db.clone();
        let permission = permission.trim().to_owned();
        self.db
            .transaction_for_update(move |transaction| {
                let db = db.clone();
                let permission = permission.clone();
                Box::pin(async move {
                    grant_permission_in_transaction(transaction, &db, role_id, &permission).await
                })
            })
            .await
    }

    /// 在调用方拥有的事务中授予权限，使相关审计记录可以与授权变更一同原子提交。
    pub async fn grant_permission_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        role_id: i64,
        permission: &str,
    ) -> Result<()> {
        if role_id <= 0 {
            return Err(Error::BadRequest("role ID must be positive".into()));
        }
        validate_name(permission, "permission")?;
        grant_permission_in_transaction(transaction, &self.db, role_id, permission.trim()).await
    }

    /// 从角色移除一项权限授予，但不删除权限本身。
    pub async fn revoke_permission(&self, role_id: i64, permission: &str) -> Result<()> {
        if role_id <= 0 {
            return Err(Error::BadRequest("role ID must be positive".into()));
        }
        validate_name(permission, "permission")?;
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "DELETE FROM tb_role_permissions WHERE role_id = $1 AND permission_id = (SELECT id FROM tb_permissions WHERE name = $2)"
            }
            DbBackend::MySql | DbBackend::Sqlite => {
                "DELETE FROM tb_role_permissions WHERE role_id = ? AND permission_id = (SELECT id FROM tb_permissions WHERE name = ?)"
            }
            _ => return Err(unsupported_backend()),
        };
        connection
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [role_id.into(), permission.trim().into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(())
    }

    /// 返回用户是否通过任意已分配角色拥有此精确权限。
    ///
    /// PAT 访问还要求 token abilities 中包含完全相同的值。每次调用都会读取共享数据库，因此已提交的授予与撤销会立即生效。
    pub async fn allows(&self, user: &CurrentUser, permission: &str) -> Result<bool> {
        validate_name(permission, "permission")?;
        let bound = match (&self.guard_scope, user.guard_name(), user.provider_name()) {
            (None, None, None) => true,
            (Some((guard, provider)), Some(actual_guard), actual_provider) => {
                guard == actual_guard && provider.as_deref() == actual_provider
            }
            _ => false,
        };
        if !bound {
            return Err(Error::custom(
                "auth_policy_provider_scope_mismatch",
                "角色数据库未绑定到当前Guard及账户Provider",
            ));
        }

        if user.integer_id().is_err()
            || user.source() == crate::auth::AuthSource::PersonalAccessToken
                && !user.has_ability(permission)
        {
            return Ok(false);
        }
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => {
                "SELECT 1 FROM tb_user_roles ur INNER JOIN tb_role_permissions rp ON rp.role_id = ur.role_id INNER JOIN tb_permissions p ON p.id = rp.permission_id WHERE ur.user_id = $1 AND p.name = $2 LIMIT 1"
            }
            DbBackend::MySql | DbBackend::Sqlite => {
                "SELECT 1 FROM tb_user_roles ur INNER JOIN tb_role_permissions rp ON rp.role_id = ur.role_id INNER JOIN tb_permissions p ON p.id = rp.permission_id WHERE ur.user_id = ? AND p.name = ? LIMIT 1"
            }
            _ => return Err(unsupported_backend()),
        };
        Ok(connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [user.integer_id()?.into(), permission.trim().into()],
            ))
            .await
            .map_err(Error::internal)?
            .is_some())
    }

    /// 仅当角色授予和 token scope 均允许此身份时返回成功，否则返回 `Forbidden`。
    pub async fn authorize(&self, user: &CurrentUser, permission: &str) -> Result<()> {
        if self.allows(user, permission).await? {
            Ok(())
        } else {
            Err(Error::Forbidden(format!(
                "permission `{permission}` is required"
            )))
        }
    }

    fn connection(&self) -> Result<DatabaseConnection> {
        self.db.connection()
    }
}

fn validate_name(name: &str, kind: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.len() > 160 || name.chars().any(char::is_control) {
        return Err(Error::BadRequest(format!(
            "{kind} name must be 1..=160 non-control characters"
        )));
    }
    Ok(())
}

fn validate_ids(user_id: i64, role_id: i64) -> Result<()> {
    if user_id <= 0 || role_id <= 0 {
        return Err(Error::BadRequest(
            "user and role IDs must be positive".into(),
        ));
    }
    Ok(())
}

async fn grant_permission_in_transaction(
    transaction: &DatabaseTransaction,
    db: &Db,
    role_id: i64,
    permission: &str,
) -> Result<()> {
    let backend = transaction.get_database_backend();
    let insert = match backend {
        DbBackend::Postgres => {
            "INSERT INTO tb_permissions (name) VALUES ($1) ON CONFLICT (name) DO NOTHING"
        }
        DbBackend::MySql => {
            "INSERT INTO tb_permissions (name) VALUES (?) ON DUPLICATE KEY UPDATE name = VALUES(name)"
        }
        DbBackend::Sqlite => {
            "INSERT INTO tb_permissions (name) VALUES (?) ON CONFLICT (name) DO NOTHING"
        }
        _ => return Err(unsupported_backend()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &crate::auth::resolve_auth_sql(db, insert)?,
            [permission.into()],
        ))
        .await
        .map_err(Error::internal)?;
    let select = match backend {
        DbBackend::Postgres => "SELECT id FROM tb_permissions WHERE name = $1",
        DbBackend::MySql | DbBackend::Sqlite => "SELECT id FROM tb_permissions WHERE name = ?",
        _ => return Err(unsupported_backend()),
    };
    let row = transaction
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            &crate::auth::resolve_auth_sql(db, select)?,
            [permission.into()],
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| {
            Error::internal(std::io::Error::other("permission insert was not visible"))
        })?;
    let permission_id = row.try_get::<i64>("", "id").map_err(Error::internal)?;
    let link = match backend {
        DbBackend::Postgres => {
            "INSERT INTO tb_role_permissions (role_id, permission_id) VALUES ($1, $2) ON CONFLICT (role_id, permission_id) DO NOTHING"
        }
        DbBackend::MySql => {
            "INSERT INTO tb_role_permissions (role_id, permission_id) VALUES (?, ?) ON DUPLICATE KEY UPDATE permission_id = VALUES(permission_id)"
        }
        DbBackend::Sqlite => {
            "INSERT INTO tb_role_permissions (role_id, permission_id) VALUES (?, ?) ON CONFLICT (role_id, permission_id) DO NOTHING"
        }
        _ => return Err(unsupported_backend()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &crate::auth::resolve_auth_sql(db, link)?,
            [role_id.into(), permission_id.into()],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

fn unsupported_backend() -> Error {
    Error::custom(
        "unsupported_auth_database",
        "authorization requires PostgreSQL, MySQL or SQLite",
    )
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use crate::auth::AuthSource;
    #[cfg(feature = "db-sqlite")]
    use crate::auth::{AuthMigration, AuthPolicyMigration};
    #[cfg(feature = "db-sqlite")]
    use crate::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
    #[cfg(feature = "db-sqlite")]
    use crate::testing::TestDatabase;

    #[cfg(feature = "db-sqlite")]
    struct PolicyMigrator;
    #[cfg(feature = "db-sqlite")]
    impl MigratorTrait for PolicyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(AuthMigration), Box::new(AuthPolicyMigration)]
        }
    }

    fn user(guard: Option<&str>, provider: Option<&str>) -> CurrentUser {
        CurrentUser {
            id: 42.into(),
            source: AuthSource::Session,
            token_abilities: None,
            guard: guard.map(str::to_owned),
            provider: provider.map(str::to_owned),
        }
    }

    fn ensure(condition: bool, message: &'static str) -> Result<()> {
        if condition {
            Ok(())
        } else {
            Err(Error::custom("policy_scope_contract", message))
        }
    }

    #[tokio::test]
    async fn named_policy_rejects_other_sources_before_database_access() -> Result<()> {
        let cold = Policy::new(Db::default()).with_guard_scope("web", Some("users".into()));
        // 未初始化的Db：错误必须来自来源检查，不能先尝试角色查询或默认为拒绝false。
        for other in [
            user(Some("admin"), Some("users")),
            user(Some("web"), Some("admins")),
            user(Some("web"), None),
            user(None, None),
        ] {
            let error = cold
                .allows(&other, "articles.read")
                .await
                .err()
                .ok_or_else(|| Error::custom("policy_scope_contract", "跨来源身份未明确拒绝"))?;
            ensure(
                error.code() == "auth_policy_provider_scope_mismatch",
                "来源不匹配前访问了数据库",
            )?;
        }
        let modern = user(Some("web"), Some("users"));
        let unbound = Policy::new(Db::default());
        let error = unbound
            .allows(&modern, "articles.read")
            .await
            .err()
            .ok_or_else(|| {
                Error::custom("policy_scope_contract", "未绑定角色来源仍授予具名身份")
            })?;
        ensure(
            error.code() == "auth_policy_provider_scope_mismatch",
            "未绑定角色来源先访问了数据库",
        )?;
        let mut string_id = modern.clone();
        string_id.id = serde_json::json!("42");
        ensure(
            !cold.allows(&string_id, "articles.read").await?,
            "字符串主键被截断为整数角色ID",
        )?;
        Ok(())
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn named_policy_honors_real_grants_only_for_matching_source() -> Result<()> {
        let modern = user(Some("web"), Some("users"));
        let database = TestDatabase::sqlite_file::<PolicyMigrator>().await?;
        let outcome: Result<()> = async {
            let db = Db::default();
            db.initialize(database.connection()?)?;
            let now: crate::database::sea_orm::Value = "2026-11-04 00:00:00.000000".into();
            db.table("users")?
                .insert(vec![
                    ("id", 42.into()),
                    ("email", "policy-source@example.test".into()),
                    ("password_hash", "synthetic-unused-hash".into()),
                    ("email_verified_at", now.clone()),
                    ("created_at", now.clone()),
                    ("updated_at", now),
                ])
                .await?;
            let policy = Policy::new(db).with_guard_scope("web", Some("users".into()));
            let role = policy.ensure_role("editor").await?;
            policy.assign_role(42, role).await?;
            policy.grant_permission(role, "articles.read").await?;
            ensure(
                policy.allows(&modern, "articles.read").await?,
                "同来源真实角色授权未生效",
            )?;
            ensure(
                !policy.allows(&modern, "articles.write").await?,
                "未授予的权限被放行",
            )?;
            for other in [
                user(Some("admin"), Some("users")),
                user(Some("web"), Some("admins")),
            ] {
                let error = policy
                    .allows(&other, "articles.read")
                    .await
                    .err()
                    .ok_or_else(|| {
                        Error::custom("policy_scope_contract", "相同整数ID跨来源借用了真实角色")
                    })?;
                ensure(
                    error.code() == "auth_policy_provider_scope_mismatch",
                    "跨来源拒绝错误类型变化",
                )?;
            }
            policy.revoke_permission(role, "articles.read").await?;
            ensure(
                !policy.allows(&modern, "articles.read").await?,
                "真实权限撤销后仍授权",
            )
        }
        .await;
        let closed = database.close().await;
        outcome?;
        closed
    }
}
