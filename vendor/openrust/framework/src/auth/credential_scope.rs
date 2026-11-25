//! 可选凭据版本策略的作用域；未配置时保留原有应用级契约。
use crate::{
    auth::{AuthError, AuthResult},
    contracts::config::Repository,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    guards: Vec<String>,
    tables: Vec<Table>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    connection: String,
    table: String,
}
fn scope(config: &dyn Repository) -> AuthResult<Option<Scope>> {
    let Some(value) = config
        .get("auth.session_credential_version_scope")
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    let value: Scope = serde_json::from_value(value)
        .map_err(|_| AuthError::Configuration("auth.session_credential_version_scope"))?;
    if value.guards.iter().any(|name| name.trim().is_empty())
        || value
            .tables
            .iter()
            .any(|row| row.connection.trim().is_empty() || row.table.trim().is_empty())
    {
        return Err(AuthError::Configuration(
            "auth.session_credential_version_scope",
        ));
    }
    Ok(Some(value))
}
pub(super) fn guard_applies(config: &dyn Repository, guard: &str) -> AuthResult<bool> {
    Ok(scope(config)?.is_none_or(|scope| scope.guards.iter().any(|name| name == guard)))
}
pub(super) fn table_applies(
    config: &dyn Repository,
    connection: &str,
    table: &str,
) -> AuthResult<bool> {
    Ok(scope(config)?.is_none_or(|scope| {
        scope
            .tables
            .iter()
            .any(|row| row.connection == connection && row.table == table)
    }))
}
