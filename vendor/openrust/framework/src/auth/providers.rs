use crate::{
    contracts::{auth::Authenticatable, hashing::Hasher},
    database::{Connection, Value},
    hashing::HashOptions,
};

use super::{AuthError, AuthIdentifier, AuthResult, Credentials};

pub(super) fn matches_token(user: &dyn Authenticatable, token: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    !token.is_empty()
        && user
            .get_remember_token()
            .is_some_and(|stored| bool::from(stored.as_bytes().ct_eq(token.as_bytes())))
}

pub(super) fn matches_api_token(value: Option<&Value>, token: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    matches!(value, Some(Value::Text(stored)) if bool::from(stored.as_bytes().ct_eq(token.as_bytes())))
}

pub(super) async fn validate(
    hasher: &dyn Hasher,
    user: &dyn Authenticatable,
    credentials: &Credentials,
) -> AuthResult<bool> {
    let Some(hash) = user.get_auth_password() else {
        return Ok(false);
    };
    hasher
        .check(
            credentials.password().as_bytes(),
            hash,
            &HashOptions::default(),
        )
        .await
        .map_err(AuthError::backend)
}

pub(super) async fn update_token(
    connection: &Connection,
    table: &str,
    user: &mut dyn Authenticatable,
    token: &str,
) -> AuthResult<()> {
    let field = user.get_remember_token_name();
    if field.is_empty() {
        return Err(AuthError::Configuration("remember token column"));
    }
    let affected = guard_credentials(
        connection.table(table).r#where(
            user.get_auth_identifier_name(),
            "=",
            Value::from(user.get_auth_identifier()),
        ),
        connection,
        user,
        table,
    )?
    .update(std::collections::BTreeMap::from([(
        field.to_owned(),
        Value::from(token),
    )]))
    .await?;
    if affected == 0 {
        return Err(if requires_primary(connection, table)? {
            crate::http::HttpError::Unauthenticated.into()
        } else {
            AuthError::InvalidIdentity
        });
    }
    user.set_remember_token(Some(token.to_owned()));
    Ok(())
}

pub(super) async fn rehash(
    hasher: &dyn Hasher,
    connection: &Connection,
    table: &str,
    user: &dyn Authenticatable,
    credentials: &Credentials,
    force: bool,
) -> AuthResult<()> {
    let Some(old) = user.get_auth_password() else {
        return Ok(());
    };
    let options = HashOptions::default();
    if !force && !hasher.needs_rehash(old, &options)? {
        return Ok(());
    }
    let new = hasher
        .make(credentials.password().as_bytes(), &options)
        .await?;
    let field = user.get_auth_password_name();
    let affected = guard_credentials(
        connection
            .table(table)
            .r#where(
                user.get_auth_identifier_name(),
                "=",
                Value::from(user.get_auth_identifier()),
            )
            .r#where(field, "=", old),
        connection,
        user,
        table,
    )?
    .update(std::collections::BTreeMap::from([(
        field.to_owned(),
        Value::Text(new),
    )]))
    .await?;
    if affected == 0 {
        return Err(if requires_primary(connection, table)? {
            crate::http::HttpError::Unauthenticated.into()
        } else {
            AuthError::InvalidIdentity
        });
    }
    Ok(())
}

/// MySQL compares textual parameters against numeric columns with coercion. Recheck
/// a retrieved identity before authenticating, including the full value of string keys.
// A wire identifier may be decimal text while the selected model hydrates an integer.
// Compare canonical forms so SQL numeric coercion cannot turn "077" or "77junk" into 77.
pub(super) fn same_identity(requested: &AuthIdentifier, actual: &AuthIdentifier) -> bool {
    match (requested, actual) {
        (AuthIdentifier::Integer(left), AuthIdentifier::UnsignedInteger(right))
        | (AuthIdentifier::UnsignedInteger(right), AuthIdentifier::Integer(left)) => {
            u64::try_from(*left).ok() == Some(*right)
        }
        (AuthIdentifier::String(text), AuthIdentifier::Integer(number))
        | (AuthIdentifier::Integer(number), AuthIdentifier::String(text)) => {
            text == &number.to_string()
        }
        (AuthIdentifier::String(text), AuthIdentifier::UnsignedInteger(number))
        | (AuthIdentifier::UnsignedInteger(number), AuthIdentifier::String(text)) => {
            text == &number.to_string()
        }
        _ => requested == actual,
    }
}

fn version_attribute(connection: &Connection, table: &str) -> AuthResult<Option<String>> {
    use crate::contracts::container::Container as _;
    let Some(container) = Connection::model_container(Some(connection)) else {
        return Ok(None);
    };
    let config = container.get::<dyn crate::contracts::config::Repository>("config")?;
    if !super::credential_scope::table_applies(config.as_ref(), connection.get_name(), table)? {
        return Ok(None);
    }
    match config.get("auth.session_credential_version_attribute") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(name)) if !name.is_empty() => Ok(Some(name)),
        _ => Err(AuthError::Configuration(
            "auth.session_credential_version_attribute",
        )),
    }
}

/// 持久化版本撤销要求读取主库，不能因副本延迟再次接受已撤销凭据。
pub(super) fn requires_primary(connection: &Connection, table: &str) -> AuthResult<bool> {
    Ok(version_attribute(connection, table)?.is_some())
}

fn guard_credentials(
    query: crate::database::query::Builder<'static>,
    connection: &Connection,
    user: &dyn Authenticatable,
    table: &str,
) -> AuthResult<crate::database::query::Builder<'static>> {
    let Some(column) = version_attribute(connection, table)? else {
        return Ok(query);
    };
    let version = match user.get_auth_attribute(&column)? {
        Some(Value::Integer(value)) if value >= 0 => Value::Integer(value),
        Some(Value::Unsigned(value)) => Value::Unsigned(value),
        _ => return Err(AuthError::Configuration("用户持久化凭据版本必须是非负整数")),
    };
    let query = query.r#where(&column, "=", version);
    Ok(match user.get_auth_password() {
        Some(password) => query.r#where(user.get_auth_password_name(), "=", password),
        None => query.where_null(user.get_auth_password_name()),
    })
}
