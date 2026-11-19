use std::sync::Arc;

use super::{RuleFailure, ValidationError, ValidationResult, Value};
use crate::{
    auth::Auth, contracts::auth::Authenticatable, hashing::HashOptions, http::Request,
    support::facades::Hash,
};

pub(super) type UserResolver =
    dyn Fn(Option<&str>) -> ValidationResult<Option<Arc<dyn Authenticatable>>> + Send + Sync;

pub(super) async fn failures(
    value: Option<&Value>,
    guard: Option<&str>,
    nullable: bool,
    request: Option<&mut Request>,
    resolver: Option<&UserResolver>,
) -> ValidationResult<Vec<RuleFailure>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if (nullable && value.is_null())
        || value.as_str().is_some_and(|value| {
            value
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .is_empty()
        })
    {
        return Ok(Vec::new());
    }
    // PHP AuthManager treats empty and "0" guard names as the default driver.
    let guard = guard.filter(|guard| !guard.is_empty() && *guard != "0");
    let user = if let Some(request) = request {
        Auth::manager()
            .map_err(execution)?
            .userForGuard(request, guard)
            .await
            .map_err(execution)?
    } else {
        resolver.ok_or_else(|| {
            execution(std::io::Error::other(
                "current_password requires an HTTP request or an explicit user resolver",
            ))
        })?(guard)?
    };
    let valid = if let Some(user) = user {
        if let Some(hash) = user.get_auth_password().filter(|hash| !hash.is_empty()) {
            let value = match value {
                Value::Null => String::new(),
                Value::String(value) => value.clone(),
                Value::Bool(false) => String::new(),
                Value::Bool(true) => "1".into(),
                Value::Number(value) if value.is_f64() => {
                    crate::support::php_float_string(value.as_f64().expect("float"))
                }
                Value::Number(value) => value.to_string(),
                _ => {
                    return Err(execution(std::io::Error::other(
                        "current_password must be convertible to a string",
                    )));
                }
            };
            Hash::check(value, hash, &HashOptions::new())
                .await
                .map_err(execution)?
        } else {
            false
        }
    } else {
        false
    };
    Ok((!valid)
        .then(|| RuleFailure::new("validation.current_password"))
        .into_iter()
        .collect())
}

fn execution(error: impl std::error::Error + Send + Sync + 'static) -> ValidationError {
    ValidationError::Execution {
        source: Box::new(error),
    }
}
