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
    let affected = connection
        .table(table)
        .r#where(
            user.get_auth_identifier_name(),
            "=",
            Value::from(user.get_auth_identifier()),
        )
        .update(std::collections::BTreeMap::from([(
            field.to_owned(),
            Value::from(token),
        )]))
        .await
        .map_err(AuthError::backend)?;
    if affected == 0 {
        return Err(AuthError::InvalidIdentity);
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
    if !force
        && !hasher
            .needsRehash(old, &options)
            .map_err(AuthError::backend)?
    {
        return Ok(());
    }
    let new = hasher
        .make(credentials.password().as_bytes(), &options)
        .await
        .map_err(AuthError::backend)?;
    let field = user.get_auth_password_name();
    let affected = connection
        .table(table)
        .r#where(
            user.get_auth_identifier_name(),
            "=",
            Value::from(user.get_auth_identifier()),
        )
        .r#where(field, "=", old)
        .update(std::collections::BTreeMap::from([(
            field.to_owned(),
            Value::Text(new),
        )]))
        .await
        .map_err(AuthError::backend)?;
    if affected == 0 {
        return Err(AuthError::InvalidIdentity);
    }
    Ok(())
}

/// MySQL compares textual parameters against numeric columns with coercion. Recheck
/// a retrieved identity before authenticating, including the full value of string keys.
pub(super) fn same_identity(requested: &AuthIdentifier, actual: &AuthIdentifier) -> bool {
    match (requested, actual) {
        (AuthIdentifier::Integer(left), AuthIdentifier::UnsignedInteger(right))
        | (AuthIdentifier::UnsignedInteger(right), AuthIdentifier::Integer(left)) => {
            u64::try_from(*left).ok() == Some(*right)
        }
        _ => requested == actual,
    }
}
