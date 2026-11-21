#![allow(non_snake_case, reason = "认证与队列公开契约沿用 Laravel 原名")]

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
            .getRememberToken()
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
    let Some(hash) = user.getAuthPassword() else {
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
    let field = user.getRememberTokenName();
    if field.is_empty() {
        return Err(AuthError::Configuration("remember token column"));
    }
    let affected = connection
        .table(table)
        .r#where(
            user.getAuthIdentifierName(),
            "=",
            Value::from(user.getAuthIdentifier()),
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
    user.setRememberToken(Some(token.to_owned()));
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
    let Some(old) = user.getAuthPassword() else {
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
    let field = user.getAuthPasswordName();
    let affected = connection
        .table(table)
        .r#where(
            user.getAuthIdentifierName(),
            "=",
            Value::from(user.getAuthIdentifier()),
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
