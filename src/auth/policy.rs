//! Database-backed roles and exact permission checks.

use std::fmt;

use crate::core::{Error, Result};
use crate::db::sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement};
use crate::db::{DatabaseConnection, Db};

use crate::auth::CurrentUser;

/// Shared database-backed role and permission operations.
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
    /// Creates a policy service using the application's shared database.
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Ensures a globally named role exists and returns its database ID.
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

    /// Assigns an existing role to an existing user; repeated assignment is idempotent.
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

    /// Removes a role assignment from a user.
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

    /// Ensures a permission exists, then grants it to an existing role.
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

    /// Removes one permission grant from a role without deleting the permission itself.
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

    /// Returns whether the user has this exact permission through any assigned role.
    ///
    /// PAT access additionally requires the same exact value in its token abilities. Every call
    /// reads the shared database, so committed grants and revocations are immediately visible.
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

    /// Returns `Forbidden` unless the identity is permitted by its role grants and token scope.
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
