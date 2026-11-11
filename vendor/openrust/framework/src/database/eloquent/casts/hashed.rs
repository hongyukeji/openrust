use std::sync::Arc;

use zeroize::Zeroizing;

use crate::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        hashing::Hasher,
    },
    database::{Connection, DatabaseError, DatabaseResult, Value},
    hashing::{HashError, HashOptions},
};

enum Input {
    Ready(Value),
    Plain(Arc<dyn Hasher>, Zeroizing<String>),
}

fn prepare(value: Value, connection: Option<&Connection>) -> DatabaseResult<Input> {
    let text = Zeroizing::new(match value {
        Value::Null => return Ok(Input::Ready(Value::Null)),
        Value::Text(text) => text,
        value => super::primitive::scalar_string(&value)?,
    });
    let hasher = Connection::model_container(connection)
        .ok_or_else(|| BindingResolutionError::NotBound {
            service: "hash".into(),
        })
        .and_then(|container| container.make::<dyn Hasher>())
        .map_err(HashError::Unavailable)
        .map_err(DatabaseError::AttributeHashing)?;
    if hasher
        .info(&text)
        .map_err(DatabaseError::AttributeHashing)?
        .algo
        .is_none()
    {
        return Ok(Input::Plain(hasher, text));
    }
    if !hasher
        .verifyConfiguration(&text)
        .map_err(DatabaseError::AttributeHashing)?
    {
        return Err(DatabaseError::AttributeHashing(HashError::InvalidOptions));
    }
    Ok(Input::Ready(Value::Text((*text).clone())))
}

pub(super) fn write(value: Value, connection: Option<&Connection>) -> DatabaseResult<Value> {
    match prepare(value, connection)? {
        Input::Ready(value) => Ok(value),
        Input::Plain(..) => Err(DatabaseError::InvalidQuery(
            "hashed attribute requires async assignment",
        )),
    }
}

pub(super) async fn write_async(
    value: Value,
    connection: Option<&Connection>,
) -> DatabaseResult<Value> {
    match prepare(value, connection)? {
        Input::Ready(value) => Ok(value),
        Input::Plain(hasher, text) => hasher
            .make(text.as_bytes(), &HashOptions::new())
            .await
            .map(Value::Text)
            .map_err(DatabaseError::AttributeHashing),
    }
}
