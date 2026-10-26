//! 基于数据库的角色与精确权限检查。

use std::fmt;

use crate::core::{Error, Result};
use crate::db::sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};

use crate::auth::CurrentUser;

/// 通过共享数据库执行的角色与权限操作。
#[derive(Clone)]
pub struct Policy {
    db: Db,
}

impl fmt::Debug for Policy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Policy")
    }
}

impl Policy {
    /// 使用应用共享数据库创建 policy 服务。
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// 确保指定全局名称的角色存在，并返回其数据库 ID。
    pub async fn ensure_role(&self, name: &str) -> Result<i64> {
        validate_name(name, "role")?;
        let connection = self.connection()?;
        let backend = connection.get_database_backend();
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
            DbBackend::MySql => "SELECT id FROM tb_roles WHERE name = ?",
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
            DbBackend::MySql => "DELETE FROM tb_user_roles WHERE user_id = ? AND role_id = ?",
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
            .transaction(move |transaction| {
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
            DbBackend::MySql => {
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
        if user.id() <= 0
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
            DbBackend::MySql => {
                "SELECT 1 FROM tb_user_roles ur INNER JOIN tb_role_permissions rp ON rp.role_id = ur.role_id INNER JOIN tb_permissions p ON p.id = rp.permission_id WHERE ur.user_id = ? AND p.name = ? LIMIT 1"
            }
            _ => return Err(unsupported_backend()),
        };
        Ok(connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &crate::auth::resolve_auth_sql(&self.db, sql)?,
                [user.id().into(), permission.trim().into()],
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
        DbBackend::MySql => "SELECT id FROM tb_permissions WHERE name = ?",
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
        "authorization requires PostgreSQL or MySQL",
    )
}
