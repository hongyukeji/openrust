use crate::database::{Connection, DB, DatabaseError, DatabaseResult, Transaction, Value};

use super::{
    Model, ModelState,
    concerns::{
        has_events::{ModelEvent::*, fire_with_transaction},
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

fn transaction_connection<M: Model>(
    model: &M,
    transaction: &Transaction,
) -> DatabaseResult<Connection> {
    if transaction.transactionLevel() == 0 {
        return Err(DatabaseError::TransactionClosed);
    }
    let connection = transaction.query_connection();
    let mismatch = match &model.state().connection {
        Some(source) => !transaction.belongs_to(source),
        None => M::metadata()
            .connection
            .is_some_and(|name| name != connection.getName()),
    };
    if mismatch {
        return Err(DatabaseError::InvalidQuery("model transaction connection"));
    }
    Ok(connection.clone())
}

fn operation_connection<M: Model>(
    model: &M,
    transaction: Option<&Transaction>,
) -> DatabaseResult<Connection> {
    match transaction {
        Some(transaction) => transaction_connection(model, transaction),
        None => connection(model),
    }
}

fn table<'a>(
    connection: &Connection,
    transaction: Option<&'a mut Transaction>,
    name: &str,
) -> crate::database::query::Builder<'a> {
    match transaction {
        Some(transaction) => transaction.table(name),
        None => connection.table(name),
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

pub(super) async fn save<M: Model>(
    model: &mut M,
    mut transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    let metadata = M::metadata();
    let connection = operation_connection(model, transaction.as_deref())?;
    if !fire_with_transaction(model, &connection, Saving, true, transaction.as_deref_mut()).await? {
        return Ok(false);
    }
    let exists = model.state().exists;
    if !exists && M::usesUniqueIds() {
        model.setUniqueIds().await?;
    }
    if exists && model.getDirty()?.is_empty() {
        return finish_save(model, &connection, transaction).await;
    }
    if !fire_with_transaction(
        model,
        &connection,
        if exists { Updating } else { Creating },
        true,
        transaction.as_deref_mut(),
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
        return finish_save(model, &connection, transaction).await;
    }
    let query = table(&connection, transaction.as_deref_mut(), metadata.table);
    if exists {
        query
            .r#where(metadata.primary_key, "=", persisted_key(model)?)
            .update(attributes.clone())
            .await?;
    } else if M::getIncrementing() {
        let id = query.insertGetId(attributes.clone()).await?;
        // MySQL reports zero for an explicitly supplied non-generated key.
        if !matches!(id, Value::Integer(0) | Value::Unsigned(0))
            || !attributes.contains_key(metadata.primary_key)
        {
            attributes.insert(metadata.primary_key.into(), id);
        }
    } else if attributes.is_empty() {
        return finish_save(model, &connection, transaction).await;
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
        fire_with_transaction(
            model,
            &connection,
            if exists { Updated } else { Created },
            false,
            transaction.as_deref_mut(),
        )
        .await?;
        fire_with_transaction(model, &connection, Saved, false, transaction).await?;
        Ok(true)
    }
    .await;
    // Record the write even if a post-write listener fails. Preserve listener mutations as dirty.
    model.state_mut().original = persisted;
    result
}

async fn finish_save<M: Model>(
    model: &mut M,
    connection: &Connection,
    transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    let persisted = model.state().attributes.clone();
    fire_with_transaction(model, connection, Saved, false, transaction).await?;
    model.state_mut().original = persisted;
    Ok(true)
}

pub(super) async fn update<M: Model>(
    model: &mut M,
    attributes: super::Attributes,
    transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    if !model.state().exists {
        return Ok(false);
    }
    if let Some(transaction) = transaction.as_deref() {
        transaction_connection(model, transaction)?;
    }
    model.fillAsync(attributes).await?;
    save(model, transaction).await
}

pub(super) async fn delete<M: Model>(
    model: &mut M,
    force: bool,
    mut transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    if !model.state().exists {
        return Ok(false);
    }
    let metadata = M::metadata();
    let connection = operation_connection(model, transaction.as_deref())?;
    let force = force && metadata.deleted_at.is_some();
    if force
        && !fire_with_transaction(
            model,
            &connection,
            ForceDeleting,
            true,
            transaction.as_deref_mut(),
        )
        .await?
    {
        return Ok(false);
    }
    if !fire_with_transaction(
        model,
        &connection,
        Deleting,
        true,
        transaction.as_deref_mut(),
    )
    .await?
    {
        return Ok(false);
    }
    let query = table(&connection, transaction.as_deref_mut(), metadata.table).r#where(
        metadata.primary_key,
        "=",
        persisted_key(model)?,
    );
    if let Some(column) = metadata.deleted_at.filter(|_| !force) {
        let attributes = soft_deletes::deletion_attributes(metadata, column, &connection)?;
        query.update(attributes.clone()).await?;
        sync_columns(model.state_mut(), attributes);
        fire_with_transaction(
            model,
            &connection,
            Trashed,
            false,
            transaction.as_deref_mut(),
        )
        .await?;
    } else {
        query.delete().await?;
        model.state_mut().exists = false;
    }
    fire_with_transaction(
        model,
        &connection,
        Deleted,
        false,
        transaction.as_deref_mut(),
    )
    .await?;
    if force {
        fire_with_transaction(model, &connection, ForceDeleted, false, transaction).await?;
    }
    Ok(true)
}

pub(super) async fn restore<M: Model>(
    model: &mut M,
    mut transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    let column = M::metadata()
        .deleted_at
        .ok_or_else(soft_deletes::unsupported)?;
    if !model.state().exists {
        return Ok(false);
    }
    let connection = operation_connection(model, transaction.as_deref())?;
    if !fire_with_transaction(
        model,
        &connection,
        Restoring,
        true,
        transaction.as_deref_mut(),
    )
    .await?
    {
        return Ok(false);
    }
    let previous = model.getAttribute(column).cloned();
    model.state_mut().setAttribute(column, Value::Null);
    let result = save(model, transaction.as_deref_mut()).await;
    if matches!(result, Ok(true)) {
        fire_with_transaction(model, &connection, Restored, false, transaction).await?;
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
