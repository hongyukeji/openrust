use crate::database::{Connection, DB, DatabaseError, DatabaseResult, Value};

use super::{
    Model, ModelState,
    concerns::{
        has_events::{ModelEvent::*, fire},
        has_timestamps::timestamp,
    },
    soft_deletes,
};

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
    if !fire(model, &connection, Saving, true).await? {
        return Ok(false);
    }
    let exists = model.state().exists;
    if exists && model.getDirty()?.is_empty() {
        return finish_save(model, &connection).await;
    }
    if !fire(
        model,
        &connection,
        if exists { Updating } else { Creating },
        true,
    )
    .await?
    {
        return Ok(false);
    }
    let mut attributes = if exists {
        model.getDirty()?
    } else {
        model.state().attributes.clone()
    };

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

    if exists && attributes.is_empty() {
        return finish_save(model, &connection).await;
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
        return finish_save(model, &connection).await;
    } else {
        query.insert(std::slice::from_ref(&attributes)).await?;
    }

    let state = model.state_mut();
    state.apply_attributes(attributes.clone());
    if exists {
        state.changes.clone_from(&attributes);
    } else {
        state.was_recently_created = true;
    }
    state.exists = true;
    state.connection = Some(connection.clone());
    let persisted = state.attributes.clone();
    let result = async {
        fire(
            model,
            &connection,
            if exists { Updated } else { Created },
            false,
        )
        .await?;
        fire(model, &connection, Saved, false).await?;
        Ok(true)
    }
    .await;
    // Record the write even if a post-write listener fails. Preserve listener mutations as dirty.
    model.state_mut().original = persisted;
    result
}

async fn finish_save<M: Model>(model: &mut M, connection: &Connection) -> DatabaseResult<bool> {
    let persisted = model.state().attributes.clone();
    fire(model, connection, Saved, false).await?;
    model.state_mut().original = persisted;
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
    let force = force && metadata.deleted_at.is_some();
    if force && !fire(model, &connection, ForceDeleting, true).await? {
        return Ok(false);
    }
    if !fire(model, &connection, Deleting, true).await? {
        return Ok(false);
    }
    let query =
        connection
            .table(metadata.table)
            .r#where(metadata.primary_key, "=", persisted_key(model)?);
    if let Some(column) = metadata.deleted_at.filter(|_| !force) {
        let attributes = soft_deletes::deletion_attributes(metadata, column, &connection)?;
        query.update(attributes.clone()).await?;
        sync_columns(model.state_mut(), attributes);
        fire(model, &connection, Trashed, false).await?;
    } else {
        query.delete().await?;
        model.state_mut().exists = false;
    }
    fire(model, &connection, Deleted, false).await?;
    if force {
        fire(model, &connection, ForceDeleted, false).await?;
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
    let connection = connection(model)?;
    if !fire(model, &connection, Restoring, true).await? {
        return Ok(false);
    }
    let previous = model.getAttribute(column).cloned();
    model.state_mut().setAttribute(column, Value::Null);
    let result = save(model).await;
    if matches!(result, Ok(true)) {
        fire(model, &connection, Restored, false).await?;
    } else if model.state().original.get(column) != Some(&Value::Null) {
        model.state_mut().invalidate_value_objects();
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
