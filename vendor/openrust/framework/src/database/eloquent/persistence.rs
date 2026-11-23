use crate::database::{Connection, DatabaseError, DatabaseResult, Transaction, Value};

use super::{
    Builder, Model, ModelState, NamedRelations,
    concerns::{
        has_events::{ModelEvent::*, fire_with_transaction},
        has_relationships::load_eager_with_transaction,
        has_timestamps::timestamp,
    },
    soft_deletes, touches,
};

fn connection<M: Model>(model: &M) -> DatabaseResult<Connection> {
    model.getConnection()
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
        None => model
            .resolveConnectionName()?
            .as_deref()
            .is_some_and(|name| name != connection.get_name()),
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

fn persistence_table<M: Model>(model: &M) -> &'static str {
    model
        .state()
        .pivot_identity
        .map_or(M::metadata().table, |identity| identity.table)
}

pub(super) fn scoped_model<'a, M: Model>(
    model: &M,
    query: crate::database::query::Builder<'a>,
) -> DatabaseResult<crate::database::query::Builder<'a>> {
    let state = model.state();
    if let Some(identity) = state.pivot_identity {
        let query = identity.constrain(query, &state.original)?;
        if let Some(key) = state.original.get(M::metadata().primary_key)
            && *key != Value::Null
            && !identity
                .columns()
                .any(|column| column == M::metadata().primary_key)
        {
            return Ok(query.r#where(M::metadata().primary_key, "=", key.clone()));
        }
        Ok(query)
    } else {
        Ok(query.r#where(M::metadata().primary_key, "=", persisted_key(model)?))
    }
}

pub(super) fn reload_query<M: Model>(model: &M) -> DatabaseResult<Builder<'static, M>> {
    reload_query_on(model, &connection(model)?)
}

fn reload_query_on<M: Model>(
    model: &M,
    connection: &Connection,
) -> DatabaseResult<Builder<'static, M>> {
    let mut query = M::queryOn(connection).withoutGlobalScopes().useWritePdo();
    query.casts.clone_from(&model.state().casts);
    query.for_persisted_model(model)
}

pub(super) async fn refresh<'a, M: Model>(
    model: &'a mut M,
    mut transaction: Option<&mut Transaction>,
) -> DatabaseResult<&'a mut M> {
    if !model.state().exists {
        return Ok(model);
    }

    let loaded = if let Some(tx) = transaction.as_deref_mut() {
        let connection = transaction_connection(model, tx)?;
        reload_query_on(model, &connection)?
            .lock_for_update()
            .limit(1)
            .get_with_transaction(Some(tx))
            .await?
            .into_iter()
            .next()
    } else {
        Box::pin(reload_query(model)?.first()).await?
    }
    .ok_or(DatabaseError::ModelNotFound(model.getTable()))?;
    let state = model.state_mut();
    if let Some(link) = state.detached_autoload.take() {
        state.relations = link.visible_relations(state, |_| true);
    }
    let names = state.relations.reloadable_names();
    let paths: Vec<_> = names.iter().map(String::as_str).collect();
    state.attributes = loaded.state().attributes.clone();
    state.invalidate_value_objects();
    let loads = NamedRelations::<M>::prepare(&paths)?;
    load_eager_with_transaction(loads, std::slice::from_mut(model), transaction).await?;
    let state = model.state_mut();
    state.original.clone_from(&state.attributes);
    Ok(model)
}

pub(super) async fn save<M: Model>(
    model: &mut M,
    transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    save_with_touch(model, transaction, true).await
}

#[derive(Clone, Copy)]
enum InsertMode<'a> {
    Normal,
    Ignore(Option<&'a [&'a str]>),
}

pub(super) async fn save_or_ignore<M: Model>(
    model: &mut M,
    unique_by: Option<&[&str]>,
) -> DatabaseResult<bool> {
    if model.state().exists {
        return Err(DatabaseError::InvalidQuery(
            "已存在的模型不能使用 saveOrIgnore",
        ));
    }
    save_model(model, None, true, InsertMode::Ignore(unique_by)).await
}

async fn finish_transaction(
    transaction: &mut Transaction,
    result: DatabaseResult<bool>,
) -> DatabaseResult<bool> {
    match result {
        Ok(saved) => {
            transaction.commit().await?;
            Ok(saved)
        }
        Err(error) => {
            transaction.rollBack(None).await?;
            Err(error)
        }
    }
}

pub(super) async fn save_or_fail<M: Model>(model: &mut M) -> DatabaseResult<bool> {
    let mut transaction = connection(model)?.beginTransaction().await?;
    let result = model.saveInTransaction(&mut transaction).await;
    finish_transaction(&mut transaction, result).await
}

pub(super) async fn delete_or_fail<M: Model>(model: &mut M) -> DatabaseResult<bool> {
    if !model.state().exists {
        return Ok(false);
    }
    let mut transaction = connection(model)?.beginTransaction().await?;
    let result = model.deleteInTransaction(&mut transaction).await;
    finish_transaction(&mut transaction, result).await
}

pub(in crate::database::eloquent) async fn save_with_touch<M: Model>(
    model: &mut M,
    transaction: Option<&mut Transaction>,
    touch: bool,
) -> DatabaseResult<bool> {
    save_model(model, transaction, touch, InsertMode::Normal).await
}

async fn save_model<M: Model>(
    model: &mut M,
    mut transaction: Option<&mut Transaction>,
    touch: bool,
    insert: InsertMode<'_>,
) -> DatabaseResult<bool> {
    let metadata = M::metadata();
    let connection = operation_connection(model, transaction.as_deref())?;
    if !fire_with_transaction(model, &connection, Saving, true, transaction.as_deref_mut()).await? {
        return Ok(false);
    }
    let exists = model.state().exists;
    if !exists && model.usesUniqueIdsForInstance() {
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

    let (created_at, updated_at) = model
        .state()
        .pivot_identity
        .map_or((metadata.created_at, metadata.updated_at), |identity| {
            identity.timestamps
        });
    let timestamp_columns = [updated_at, if exists { None } else { created_at }];
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
    if let Some(identity) = model.state().pivot_identity {
        for column in identity.columns() {
            if attributes
                .keys()
                .any(|name| name != column && name.eq_ignore_ascii_case(column))
            {
                return Err(DatabaseError::InvalidQuery(
                    "pivot model cannot replace relationship keys",
                ));
            }
            if attributes
                .get(column)
                .or_else(|| model.state().attributes.get(column))
                != model.state().original.get(column)
            {
                return Err(DatabaseError::InvalidQuery(
                    "pivot model cannot replace relationship keys",
                ));
            }
        }
    }
    let query = table(
        &connection,
        transaction.as_deref_mut(),
        persistence_table(model),
    );
    if let InsertMode::Ignore(unique_by) = insert {
        // 上游在插入前将时间戳合并到实例，忽略冲突时仍保留待保存属性。
        model.state_mut().apply_attributes(attributes.clone());
        if attributes.is_empty() {
            model.state_mut().connection = Some(connection.clone());
            return finish_save(model, &connection, transaction).await;
        }
        // 将驱动插入分支置于堆上，避免其 Future 扩大所有普通模型保存的调用栈。
        let rows = Box::pin(query.insertOrIgnoreReturning(
            std::slice::from_ref(&attributes),
            &["*"],
            unique_by,
        ))
        .await?;
        model.state_mut().connection = Some(connection.clone());
        let Some(row) = rows.first() else {
            return Ok(false);
        };
        if model.getIncrementingForInstance() {
            let id = row
                .get(metadata.primary_key)
                .cloned()
                .ok_or(DatabaseError::InvalidQuery("插入结果缺少主键"))?;
            attributes.insert(metadata.primary_key.into(), id);
        }
    } else if exists {
        scoped_model(model, query)?
            .update(attributes.clone())
            .await?;
    } else if model.getIncrementingForInstance() {
        let id = query
            .insertGetIdFor(attributes.clone(), metadata.primary_key)
            .await?;
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
    if !metadata.refreshes.is_empty() {
        let refreshed =
            refresh_saved_attributes(model, &connection, transaction.as_deref_mut()).await;
        match refreshed {
            Ok(refreshed) => {
                let state = model.state_mut();
                if exists {
                    for (column, value) in &refreshed {
                        if state.original.get(column) == Some(value) {
                            state.changes.remove(column);
                        } else {
                            state.changes.insert(column.clone(), value.clone());
                        }
                    }
                }
                state.apply_attributes(refreshed);
            }
            Err(error) => return Err(error),
        }
    }
    let persisted = model.state().attributes.clone();
    let result = async {
        fire_with_transaction(
            model,
            &connection,
            if exists { Updated } else { Created },
            false,
            transaction.as_deref_mut(),
        )
        .await?;
        fire_with_transaction(model, &connection, Saved, false, transaction.as_deref_mut()).await?;
        if touch {
            touches::touch_owners(model, &persisted, &connection, transaction).await?;
        }
        Ok(true)
    }
    .await;
    // 保存后事件及关联触碰全部成功后，才同步最终实例属性。
    if result.is_ok() {
        let state = model.state_mut();
        state.original.clone_from(&state.attributes);
    }
    result
}

pub(super) fn is_unique_violation(error: &DatabaseError) -> bool {
    matches!(error, DatabaseError::Driver { source, .. } if source.as_database_error().is_some_and(|error| error.is_unique_violation()))
}

fn numeric_counter(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Unsigned(_) => true,
        Value::Real(value) => value.is_finite(),
        _ => false,
    }
}

pub(super) fn validate_counter<M: Model>(
    column: &str,
    default: &Value,
    step: &Value,
    extra: &super::Attributes,
) -> DatabaseResult<()> {
    let key = M::metadata().primary_key;
    if column.is_empty()
        || column.contains(['.', '*'])
        || column.eq_ignore_ascii_case(key)
        || extra.keys().any(|name| name.eq_ignore_ascii_case(key))
        || !numeric_counter(default)
        || !numeric_counter(step)
    {
        return Err(DatabaseError::InvalidQuery("related model counter"));
    }
    Ok(())
}

pub(super) async fn increment_related<M: Model>(
    mut model: M,
    column: &str,
    step: Value,
    extra: super::Attributes,
) -> DatabaseResult<M> {
    change_counters(
        &mut model,
        super::Attributes::from([(column.into(), step)]),
        extra,
        false,
    )
    .await?;
    Ok(model)
}

pub(super) async fn change_counters<M: Model>(
    model: &mut M,
    columns: super::Attributes,
    mut extra: super::Attributes,
    decrement: bool,
) -> DatabaseResult<u64> {
    if columns.is_empty() {
        return Ok(0);
    }
    let connection = connection(model)?;
    for (column, step) in &columns {
        validate_counter::<M>(column, step, step, &extra)?;
        extra.remove(column);
    }

    if !model.state().exists {
        add_counter_timestamp::<M>(&connection, &mut extra)?;
        let query = M::queryOn(&connection).scoped()?.0;
        return write_counters(query, columns, extra, decrement).await;
    }

    let original = model.state().clone();
    let (next, affected) = connection
        .transaction(
            |transaction| {
                let model = M::from_state(original.clone());
                let connection = connection.clone();
                let columns = columns.clone();
                let extra = extra.clone();
                Box::pin(async move {
                    change_counters_in_transaction(
                        model,
                        &connection,
                        transaction,
                        columns,
                        extra,
                        decrement,
                    )
                    .await
                })
            },
            3,
        )
        .await?;
    *model.state_mut() = next;
    Ok(affected)
}

async fn write_counters(
    query: crate::database::query::Builder<'_>,
    columns: super::Attributes,
    extra: super::Attributes,
    decrement: bool,
) -> DatabaseResult<u64> {
    if decrement {
        query.decrementEach(columns, extra).await
    } else {
        query.incrementEach(columns, extra).await
    }
}

fn add_counter_timestamp<M: Model>(
    connection: &Connection,
    assignments: &mut super::Attributes,
) -> DatabaseResult<()> {
    if let Some(column) = M::metadata().updated_at
        && !assignments.contains_key(column)
    {
        assignments.insert(
            column.into(),
            timestamp(connection, M::metadata().date_format)?,
        );
    }
    Ok(())
}

async fn change_counters_in_transaction<M: Model>(
    mut model: M,
    connection: &Connection,
    transaction: &mut Transaction,
    mut columns: super::Attributes,
    extra: super::Attributes,
    decrement: bool,
) -> DatabaseResult<(ModelState, u64)> {
    let table = persistence_table(&model);
    let operator = if decrement { "-" } else { "+" };
    let mut projection = scoped_model(&model, transaction.table(table))?.lockForUpdate();
    let needs_cast = columns
        .keys()
        .any(|column| super::casts::has_increment_caster(&model, column));
    if needs_cast {
        projection = projection.select(["*"]);
    }
    for (index, (column, step)) in columns.iter().enumerate() {
        let quoted = connection.grammar.column(column)?;
        projection = projection.selectRaw(
            &format!("{quoted} {operator} ? as laravel_counter_{index}"),
            vec![step.clone()],
        );
    }
    let row = projection
        .first()
        .await?
        .ok_or(DatabaseError::ModelNotFound(table))?
        .into_columns();
    let (raw, computed) = row.split_at(row.len() - columns.len());
    let attributes = raw.iter().cloned().collect();
    let mut assignments = super::Attributes::new();
    for ((column, step), (_, next)) in columns.iter_mut().zip(computed) {
        let cast = super::casts::counter_cast(&model, column, step, &attributes, decrement)?;
        if let Some((write, encoded_step)) = cast {
            if !numeric_counter(&encoded_step) {
                return Err(DatabaseError::InvalidQuery("related model counter"));
            }
            let mut patch = super::Attributes::new();
            write.apply(&mut patch, column.clone());
            if !patch.contains_key(column) {
                return Err(DatabaseError::InvalidQuery(
                    "counter cast must store its column",
                ));
            }
            model.state_mut().apply_attributes(patch.clone());
            assignments.extend(patch);
            *step = encoded_step;
        } else {
            model.setAttribute(column, next.clone());
        }
    }

    let before_extra = model.state().attributes.clone();
    for (name, value) in extra {
        model.setAttributeValueAsync(&name, value).await?;
    }
    assignments.extend(
        model
            .state()
            .attributes
            .iter()
            .filter(|(name, value)| before_extra.get(*name) != Some(*value))
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    for column in columns.keys() {
        assignments.remove(column);
    }
    let identity_column = |name: &str| {
        name.eq_ignore_ascii_case(M::metadata().primary_key)
            || model.state().pivot_identity.is_some_and(|identity| {
                identity
                    .columns()
                    .any(|column| column.eq_ignore_ascii_case(name))
            })
    };
    if assignments
        .keys()
        .chain(columns.keys())
        .any(|name| identity_column(name))
    {
        return Err(DatabaseError::InvalidQuery(
            "counter cannot change model identity",
        ));
    }
    if !fire_with_transaction(&mut model, connection, Updating, true, Some(transaction)).await? {
        return Ok((model.state().clone(), 0));
    }

    add_counter_timestamp::<M>(connection, &mut assignments)?;
    let mut readback: Vec<_> = assignments
        .keys()
        .chain(columns.keys())
        .cloned()
        .chain(M::metadata().refreshes.iter().map(|name| (*name).into()))
        .collect();
    readback.sort_unstable();
    readback.dedup();
    let affected = write_counters(
        scoped_model(&model, transaction.table(table))?,
        columns,
        assignments,
        decrement,
    )
    .await?;
    let persisted: super::Attributes = scoped_model(&model, transaction.table(table))?
        .select(readback.iter().map(String::as_str))
        .first()
        .await?
        .ok_or(DatabaseError::ModelNotFound(table))?
        .into_columns()
        .into_iter()
        .collect();
    let state = model.state_mut();
    state.apply_attributes(persisted.clone());
    state.changes = state.getDirty();
    fire_with_transaction(&mut model, connection, Updated, false, Some(transaction)).await?;
    model.state_mut().original.extend(persisted);
    Ok((model.state().clone(), affected))
}

async fn refresh_saved_attributes<M: Model>(
    model: &M,
    connection: &Connection,
    transaction: Option<&mut Transaction>,
) -> DatabaseResult<super::Attributes> {
    let metadata = M::metadata();
    let query = table(connection, transaction, persistence_table(model))
        .useWritePdo()
        .select(metadata.refreshes.iter().copied());
    let state = model.state();
    let query = if let Some(identity) = state.pivot_identity {
        let query = identity.constrain(query, &state.original)?;
        match model.getKey().filter(|key| **key != Value::Null) {
            Some(key)
                if !identity
                    .columns()
                    .any(|column| column == metadata.primary_key) =>
            {
                query.r#where(metadata.primary_key, "=", key.clone())
            }
            _ => query,
        }
    } else {
        let key = model
            .getKey()
            .filter(|key| !matches!(key, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("model primary key"))?;
        query.r#where(metadata.primary_key, "=", key.clone())
    };
    query
        .first()
        .await?
        .map(|row| row.into_columns().into_iter().collect())
        .ok_or(DatabaseError::ModelNotFound(persistence_table(model)))
}

async fn finish_save<M: Model>(
    model: &mut M,
    connection: &Connection,
    transaction: Option<&mut Transaction>,
) -> DatabaseResult<bool> {
    fire_with_transaction(model, connection, Saved, false, transaction).await?;
    let state = model.state_mut();
    state.original.clone_from(&state.attributes);
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
    let query = scoped_model(
        model,
        table(
            &connection,
            transaction.as_deref_mut(),
            persistence_table(model),
        ),
    )?;
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
