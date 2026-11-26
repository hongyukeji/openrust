//! 在当前连接或事务中准备模型快照与审计数据。
use crate::{Activity, ChangeDetector, Result, models::concerns::LogsActivity};
use openrust::database::{Transaction, eloquent::concerns::ModelEvent};
use serde_json::{Map, Value};

pub(crate) fn event_name(event: ModelEvent) -> Option<&'static str> {
    match event {
        ModelEvent::Created => Some("created"),
        ModelEvent::Updated => Some("updated"),
        ModelEvent::Deleted => Some("deleted"),
        ModelEvent::Restored => Some("restored"),
        ModelEvent::Retrieved => Some("retrieved"),
        ModelEvent::Creating => Some("creating"),
        ModelEvent::Updating => Some("updating"),
        ModelEvent::Saving => Some("saving"),
        ModelEvent::Saved => Some("saved"),
        ModelEvent::Restoring => Some("restoring"),
        ModelEvent::Replicating => Some("replicating"),
        ModelEvent::Trashed => Some("trashed"),
        ModelEvent::Deleting => Some("deleting"),
        ModelEvent::ForceDeleting => Some("forceDeleting"),
        ModelEvent::ForceDeleted => Some("forceDeleted"),
    }
}
async fn build<M: LogsActivity>(
    model: &M,
    event: ModelEvent,
    defaults: &[String],
    transaction: Option<&mut Transaction>,
) -> Result<Option<Activity>> {
    let Some(name) = event_name(event) else {
        return Ok(None);
    };
    if !model.enable_logging_models_events() {
        return Ok(None);
    }
    let options = model.get_activitylog_options();
    if matches!(event, ModelEvent::Created | ModelEvent::Updated) {
        let dirty = model.get_dirty()?;
        if !dirty
            .keys()
            .any(|name| !options.dont_log_if_attributes_changed_only.contains(name))
        {
            return Ok(None);
        }
        // Restoration triggers an update as well; record only its restored event.
        if model.get_deleted_at_column().is_some_and(|column| {
            model
                .state()
                .get_original()
                .get(column)
                .is_some_and(|value| !matches!(value, openrust::database::Value::Null))
                && matches!(
                    model.get_attribute(column),
                    Some(openrust::database::Value::Null)
                )
        }) {
            return Ok(None);
        }
    }
    let description = model.get_description_for_event(name);
    if description.is_empty() {
        return Ok(None);
    }
    let attributes = model.extract_changes_with_defaults(defaults)?;
    let changes = if model.attributes_to_be_logged_with(defaults).is_empty() {
        serde_json::json!({})
    } else if event == ModelEvent::Deleted {
        serde_json::json!({"old": attributes})
    } else if event == ModelEvent::Updated {
        let mut old = M::new_instance_on(&model.get_connection()?, Default::default())?;
        for (name, value) in model.state().get_original() {
            old.state_mut().set_attribute(name, value.clone());
        }
        load_related(&mut old, defaults, transaction).await?;
        let mut old = old.extract_changes_with_defaults(defaults)?;
        for name in attributes.keys() {
            old.entry(name.clone()).or_insert(Value::Null);
        }
        if options.log_only_dirty {
            ChangeDetector::filter_dirty(&attributes, &old)
        } else {
            serde_json::json!({"attributes":attributes,"old":old})
        }
    } else {
        serde_json::json!({"attributes":attributes})
    };
    if !options.log_empty_changes
        && ["old", "attributes"].iter().all(|key| {
            changes
                .get(key)
                .and_then(Value::as_object)
                .is_none_or(Map::is_empty)
        })
    {
        return Ok(None);
    }
    let mut activity = Activity::new(description)
        .event(name)
        .performed_on(crate::models::identity(model)?);
    activity.log_name = options.log_name;
    activity.changes = changes;
    activity.subject_attributes = model
        .state()
        .get_attributes()
        .iter()
        .map(|(key, value)| Ok((key.clone(), ChangeDetector::json(value)?)))
        .collect::<Result<Map<_, _>>>()?
        .into();
    Ok(Some(activity))
}
pub(crate) async fn build_stored<M: LogsActivity>(
    model: &M,
    event: ModelEvent,
    mut transaction: Option<&mut Transaction>,
    defaults: &[String],
) -> Result<Option<Activity>> {
    if !model.enable_logging_models_events() {
        return Ok(None);
    }
    let mut snapshot = model.clone();
    if event != ModelEvent::Retrieved
        && model.state().exists()
        && let Some(key) = model.get_key()
    {
        let row = match transaction.as_deref_mut() {
            Some(tx) => {
                tx.table(model.get_table())
                    .r#where(model.get_key_name(), "=", key.clone())
                    .first()
                    .await?
            }
            None => {
                model
                    .get_connection()?
                    .table(model.get_table())
                    .r#where(model.get_key_name(), "=", key.clone())
                    .first()
                    .await?
            }
        };
        if let Some(row) = row {
            for (name, value) in row.columns() {
                snapshot.state_mut().set_attribute(name, value.clone());
            }
        }
    }
    load_related(&mut snapshot, defaults, transaction.as_deref_mut()).await?;
    build(&snapshot, event, defaults, transaction).await
}
async fn load_related<M: LogsActivity>(
    model: &mut M,
    defaults: &[String],
    transaction: Option<&mut Transaction>,
) -> Result<()> {
    let paths: Vec<String> = model
        .attributes_to_be_logged_with(defaults)
        .iter()
        .filter_map(|attribute| attribute.rsplit_once('.').map(|(path, _)| path.to_owned()))
        .collect();
    if !paths.is_empty() {
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        match transaction {
            Some(tx) => model.load_paths_in_transaction(&paths, tx).await?,
            None => model.load_missing_paths(&paths).await?,
        }
    }
    Ok(())
}
