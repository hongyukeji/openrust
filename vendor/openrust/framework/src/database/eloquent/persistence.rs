use crate::database::{Connection, DB, DatabaseError, DatabaseResult, Value};

use super::{Model, ModelState, concerns::has_timestamps::timestamp, soft_deletes};

fn connection<M: Model>(model: &M) -> DatabaseResult<Connection> {
    match &model.state().connection {
        Some(connection) => Ok(connection.clone()),
        None => DB::connection(M::metadata().connection),
    }
}

fn persisted_key<M: Model>(model: &M) -> DatabaseResult<Value> {
    let state = model.state();
    state
        .original
        .get(M::metadata().primary_key)
        .or_else(|| model.getKey())
        .filter(|value| !matches!(value, Value::Null))
        .cloned()
        .ok_or(DatabaseError::InvalidQuery("model primary key"))
}

pub(super) async fn save<M: Model>(model: &mut M) -> DatabaseResult<bool> {
    let metadata = M::metadata();
    let connection = connection(model)?;
    let state = model.state();
    let exists = state.exists;
    let mut attributes = if exists {
        model.getDirty()?
    } else {
        state.attributes.clone()
    };
    if exists && attributes.is_empty() {
        let state = model.state_mut();
        state.original.clone_from(&state.attributes);
        return Ok(true);
    }

    let timestamp_columns = [
        metadata.updated_at,
        if exists { None } else { metadata.created_at },
    ];
    let missing =
        timestamp_columns.map(|column| column.filter(|column| !attributes.contains_key(*column)));
    if missing.iter().any(Option::is_some) {
        let time = timestamp(&connection, metadata.date_format)?;
        attributes.extend(
            missing
                .into_iter()
                .flatten()
                .map(|column| (column.into(), time.clone())),
        );
    }

    let query = connection.table(metadata.table);
    if exists {
        query
            .r#where(metadata.primary_key, "=", persisted_key(model)?)
            .update(attributes.clone())
            .await?;
    } else if metadata.incrementing {
        let id = query.insertGetId(attributes.clone()).await?;
        // MySQL reports zero for an explicitly supplied non-generated key.
        if !matches!(id, Value::Integer(0) | Value::Unsigned(0))
            || !attributes.contains_key(metadata.primary_key)
        {
            attributes.insert(metadata.primary_key.into(), id);
        }
    } else if attributes.is_empty() {
        return Ok(true);
    } else {
        query.insert(std::slice::from_ref(&attributes)).await?;
    }

    let state = model.state_mut();
    state.apply_attributes(attributes.clone());
    state.original.clone_from(&state.attributes);
    if exists {
        state.changes = attributes;
    } else {
        state.was_recently_created = true;
    }
    state.exists = true;
    state.connection = Some(connection);
    Ok(true)
}

pub(super) async fn update<M: Model>(
    model: &mut M,
    attributes: super::Attributes,
) -> DatabaseResult<bool> {
    if !model.state().exists {
        return Ok(false);
    }
    model.fillAsync(attributes).await?;
    save(model).await
}

pub(super) async fn delete<M: Model>(model: &mut M, force: bool) -> DatabaseResult<bool> {
    if !model.state().exists {
        return Ok(false);
    }
    let metadata = M::metadata();
    let connection = connection(model)?;
    let query =
        connection
            .table(metadata.table)
            .r#where(metadata.primary_key, "=", persisted_key(model)?);
    if let Some(column) = metadata.deleted_at.filter(|_| !force) {
        let attributes = soft_deletes::deletion_attributes(metadata, column, &connection)?;
        query.update(attributes.clone()).await?;
        sync_columns(model.state_mut(), attributes);
    } else {
        query.delete().await?;
        model.state_mut().exists = false;
    }
    Ok(true)
}

pub(super) async fn restore<M: Model>(model: &mut M) -> DatabaseResult<bool> {
    let column = M::metadata()
        .deleted_at
        .ok_or_else(soft_deletes::unsupported)?;
    if !model.state().exists {
        return Ok(false);
    }
    let previous = model
        .state_mut()
        .attributes
        .insert(column.into(), Value::Null);
    let result = save(model).await;
    if result.is_err() {
        let attributes = &mut model.state_mut().attributes;
        match previous {
            Some(value) => {
                attributes.insert(column.into(), value);
            }
            None => {
                attributes.remove(column);
            }
        }
    }
    result
}

fn sync_columns(state: &mut ModelState, attributes: super::Attributes) {
    state.apply_attributes(attributes.clone());
    state.original.extend(attributes);
}
