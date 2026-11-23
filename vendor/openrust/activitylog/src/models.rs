// Compiled model identities preserve the application's morph map.
pub(crate) fn identity<M: openrust::database::eloquent::Model>(
    model: &M,
) -> crate::Result<crate::Subject> {
    use openrust::database::Value as Sql;
    let id = match model.get_key() {
        Some(Sql::Text(value)) => value.clone(),
        Some(Sql::Integer(value)) => value.to_string(),
        Some(Sql::Unsigned(value)) => value.to_string(),
        _ => return Err(crate::Error::InvalidActivity),
    };
    crate::Subject::new(model.get_morph_class()?, id)
}

/// Per-model logging switch. Clone copies the value, so two model instances remain independent.
pub struct LoggingState(std::sync::atomic::AtomicBool);
impl Default for LoggingState {
    fn default() -> Self {
        Self(std::sync::atomic::AtomicBool::new(true))
    }
}
impl Clone for LoggingState {
    fn clone(&self) -> Self {
        Self(std::sync::atomic::AtomicBool::new(
            self.0.load(std::sync::atomic::Ordering::Relaxed),
        ))
    }
}
pub mod concerns {
    use crate::{
        Activity, ActivityLogger, ActivityQuery, ActivityRepository, ChangeDetector, Error,
        LogOptions, Result,
    };
    use openrust::{
        database::{
            Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction,
            eloquent::{
                Model,
                concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            },
        },
        foundation::Application,
    };
    use serde_json::{Map, Value};
    use std::{collections::BTreeSet, sync::Arc};

    /// Implement on a model, then register `boot_logs_activity` in the application's Provider.
    pub trait LogsActivity: Model + Clone {
        fn activitylog_state(&self) -> &super::LoggingState;
        fn disable_logging(&mut self) -> &mut Self {
            self.activitylog_state()
                .0
                .store(false, std::sync::atomic::Ordering::Relaxed);
            self
        }
        fn enable_logging(&mut self) -> &mut Self {
            self.activitylog_state()
                .0
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self
        }
        fn get_activitylog_options(&self) -> LogOptions {
            LogOptions::defaults()
        }
        fn enable_logging_models_events(&self) -> bool {
            self.activitylog_state()
                .0
                .load(std::sync::atomic::Ordering::Relaxed)
        }
        fn events_to_be_recorded() -> &'static [ModelEvent] {
            &[
                ModelEvent::Created,
                ModelEvent::Updated,
                ModelEvent::Deleted,
                ModelEvent::Restored,
            ]
        }
        fn do_not_record_events() -> &'static [ModelEvent] {
            &[]
        }
        fn boot_logs_activity(
            app: &Application,
            repository: Arc<ActivityRepository>,
        ) -> DatabaseResult<()> {
            for event in Self::events_to_be_recorded() {
                if !Self::do_not_record_events().contains(event) {
                    Self::register_model_event(
                        app,
                        *event,
                        LogsActivityListener {
                            repository: repository.clone(),
                        },
                    )?;
                }
            }
            Ok(())
        }
        fn attributes_to_be_logged(&self) -> Vec<String> {
            self.attributes_to_be_logged_with(&[])
        }
        fn attributes_to_be_logged_with(&self, defaults: &[String]) -> Vec<String> {
            let options = self.get_activitylog_options();
            let mut attributes = BTreeSet::new();
            if options.log_fillable {
                attributes.extend(self.get_fillable().iter().map(|name| (*name).to_owned()));
            }
            if options.log_unguarded {
                let guarded = self.get_guarded();
                if Self::is_unguarded() || !guarded.contains(&"*") {
                    attributes.extend(
                        self.state()
                            .get_attributes()
                            .keys()
                            .filter(|name| {
                                Self::is_unguarded() || !guarded.contains(&name.as_str())
                            })
                            .cloned(),
                    );
                }
            }
            if options.log_attributes.iter().any(|name| name == "*") {
                attributes.extend(self.state().get_attributes().keys().cloned());
            }
            attributes.extend(
                options
                    .log_attributes
                    .iter()
                    .filter(|name| name.as_str() != "*")
                    .cloned(),
            );
            attributes.retain(|name| {
                !options.log_except_attributes.contains(name) && !defaults.contains(name)
            });
            attributes.into_iter().collect()
        }
        fn get_description_for_event(&self, event: &str) -> String {
            self.get_activitylog_options()
                .description_for_event
                .map_or_else(|| event.into(), |callback| callback(event))
        }
        fn before_activity_logged(&self, _activity: &mut Activity, _event: &str) -> Result<()> {
            Ok(())
        }
        fn activities_as_subject(&self, repository: &ActivityRepository) -> Result<ActivityQuery> {
            Ok(repository.query().for_subject(&super::identity(self)?))
        }
        fn extract_changes(&self) -> Result<Map<String, Value>> {
            self.extract_changes_with_defaults(&[])
        }
        fn extract_changes_with_defaults(&self, defaults: &[String]) -> Result<Map<String, Value>> {
            let options = self.get_activitylog_options();
            let mut values = Map::new();
            for attribute in self.attributes_to_be_logged_with(defaults) {
                if attribute.contains('.') {
                    let snapshot = Value::Object(self.to_array()?);
                    let mut value = &snapshot;
                    for key in attribute.split('.') {
                        value = value.get(key).unwrap_or(&Value::Null);
                    }
                    values.insert(attribute, value.clone());
                } else if attribute.contains("->") {
                    let base = attribute.split("->").next().unwrap();
                    let value = self
                        .get_attribute_value(base)?
                        .as_ref()
                        .map(ChangeDetector::json)
                        .transpose()?
                        .unwrap_or(Value::Null);
                    let root = Map::from_iter([(base.into(), value)]);
                    let value = ChangeDetector::resolve_json_attribute(&root, &attribute);
                    set_path(
                        &mut values,
                        &attribute.split("->").collect::<Vec<_>>(),
                        value,
                    );
                } else {
                    let raw = if options.attribute_raw_values.contains(&attribute) {
                        self.get_attribute(&attribute).cloned()
                    } else {
                        self.get_attribute_value(&attribute)?
                    };
                    values.insert(
                        attribute,
                        raw.as_ref()
                            .map(ChangeDetector::json)
                            .transpose()?
                            .unwrap_or(Value::Null),
                    );
                }
            }
            Ok(values)
        }
    }
    fn set_path(values: &mut Map<String, Value>, path: &[&str], value: Value) {
        if path.len() == 1 {
            values.insert(path[0].into(), value);
            return;
        }
        let child = values
            .entry(path[0])
            .or_insert_with(|| Value::Object(Map::new()));
        if !child.is_object() {
            *child = Value::Object(Map::new());
        }
        set_path(child.as_object_mut().unwrap(), &path[1..], value);
    }
    fn event_name(event: ModelEvent) -> Option<&'static str> {
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
            .performed_on(super::identity(model)?);
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
    async fn build_stored<M: LogsActivity>(
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
    // Match the upstream current-savepoint marker plus outermost ordered save.
    fn save_after_commit(
        transaction: &mut Transaction,
        prepared: crate::PreparedActivity,
        repository: Arc<ActivityRepository>,
        buffer: Option<Arc<crate::ActivityBuffer>>,
    ) -> DatabaseResult<()> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let committed = Arc::new(AtomicBool::new(false));
        let marker = committed.clone();
        transaction.after_commit(move || {
            Box::pin(async move {
                marker.store(true, Ordering::SeqCst);
                Ok(())
            })
        })?;
        transaction.after_commit_at_level(1, move || {
            Box::pin(async move {
                if committed.load(Ordering::SeqCst) {
                    if let Some(buffer) = buffer {
                        buffer.add(prepared);
                    } else {
                        repository.save_prepared(prepared).await.map_err(error)?;
                    }
                }
                Ok(())
            })
        })
    }
    struct LogsActivityListener {
        repository: Arc<ActivityRepository>,
    }
    fn error(error: Error) -> DatabaseError {
        DatabaseError::Unavailable(
            openrust::contracts::container::BindingResolutionError::FactoryFailed {
                service: "Activitylog",
                source: Box::new(error),
            },
        )
    }
    impl<M: LogsActivity> ModelEventListener<M> for LogsActivityListener {
        fn handle<'a>(
            &'a self,
            event: ModelEvent,
            model: &'a mut M,
            _: &'a Connection,
        ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
            Box::pin(async move {
                if let Some(activity) = build_stored(
                    model,
                    event,
                    None,
                    &self.repository.default_except_attributes,
                )
                .await
                .map_err(error)?
                {
                    let mut logger = ActivityLogger::current()
                        .or_else(|| crate::facades::Activitylog::logger().ok())
                        .unwrap_or_else(|| {
                            ActivityLogger::from_repository(self.repository.clone())
                        });
                    if let Some(prepared) = logger
                        .prepare_model(activity, |activity| {
                            model.before_activity_logged(activity, event_name(event).unwrap_or(""))
                        })
                        .map_err(error)?
                    {
                        if logger.buffer_prepared(prepared.clone()).is_some() {
                            return Ok(ModelEventResponse::Continue);
                        }
                        self.repository
                            .save_prepared(prepared)
                            .await
                            .map_err(error)?;
                    }
                }
                Ok(ModelEventResponse::Continue)
            })
        }
        fn handle_in_transaction<'a>(
            &'a self,
            event: ModelEvent,
            model: &'a mut M,
            transaction: &'a mut Transaction,
        ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
            Box::pin(async move {
                if let Some(activity) = build_stored(
                    model,
                    event,
                    Some(&mut *transaction),
                    &self.repository.default_except_attributes,
                )
                .await
                .map_err(error)?
                {
                    let mut logger = ActivityLogger::current()
                        .or_else(|| crate::facades::Activitylog::logger().ok())
                        .unwrap_or_else(|| {
                            ActivityLogger::from_repository(self.repository.clone())
                        });
                    if let Some(prepared) = logger
                        .prepare_model(activity, |activity| {
                            model.before_activity_logged(activity, event_name(event).unwrap_or(""))
                        })
                        .map_err(error)?
                    {
                        if let Some(buffer) = logger.buffer() {
                            save_after_commit(
                                transaction,
                                prepared,
                                self.repository.clone(),
                                Some(buffer),
                            )?;
                            return Ok(ModelEventResponse::Continue);
                        }
                        if self.repository.uses_connection(&model.get_connection()?) {
                            prepared.log_in(transaction).await?;
                        } else {
                            save_after_commit(
                                transaction,
                                prepared,
                                self.repository.clone(),
                                None,
                            )?;
                        }
                    }
                }
                Ok(ModelEventResponse::Continue)
            })
        }
    }
}

/// The native Eloquent activity model. Custom models keep these columns and may use another table.
#[derive(Clone)]
pub struct Activity(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for Activity {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("activity_log").guarded(&[]);
        &META
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        use openrust::database::eloquent::casts::AttributeCast;
        &[
            ("properties", AttributeCast::Json),
            ("attribute_changes", AttributeCast::Json),
        ]
    }
}
impl Activity {
    pub fn subject<R: openrust::database::eloquent::Model>(
        &self,
    ) -> openrust::database::DatabaseResult<openrust::database::eloquent::relations::MorphTo<Self>>
    {
        use openrust::database::eloquent::Model;
        let relation =
            openrust::database::eloquent::relations::MorphTo::new("subject_type", "subject_id")
                .target_mapped::<R>(
                    &self.get_connection()?,
                    R::from_state(Default::default()).get_key_name(),
                )?;
        let include =
            openrust::support::facades::Config::get("activitylog.include_soft_deleted_subjects")?
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        Ok(if include {
            relation.with_trashed()
        } else {
            relation
        })
    }
    pub fn causer<R: openrust::database::eloquent::Model>(
        &self,
    ) -> openrust::database::DatabaseResult<openrust::database::eloquent::relations::MorphTo<Self>>
    {
        use openrust::database::eloquent::Model;
        openrust::database::eloquent::relations::MorphTo::new("causer_type", "causer_id")
            .target_mapped::<R>(
                &self.get_connection()?,
                R::from_state(Default::default()).get_key_name(),
            )
    }
    pub fn get_property(
        &self,
        path: &str,
        default: serde_json::Value,
    ) -> crate::Result<serde_json::Value> {
        use openrust::database::eloquent::Model;
        let value = self
            .get_attribute_value("properties")?
            .as_ref()
            .map(crate::ChangeDetector::json)
            .transpose()?
            .unwrap_or_default();
        Ok(property(&value, path).cloned().unwrap_or(default))
    }
}

/// Register under the configuration's `activity_model` name using the native container.
pub struct ActivityModel {
    pub(crate) table: &'static str,
    pub(crate) connection: Option<String>,
    pub(crate) save: for<'a> fn(
        &'a openrust::database::Connection,
        &'a crate::PreparedActivity,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<crate::ActivityRecord>,
    >,
    pub(crate) save_in: for<'a> fn(
        &'a crate::PreparedActivity,
        &'a mut openrust::database::Transaction,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<crate::ActivityRecord>,
    >,
}
impl ActivityModel {
    pub fn of<M: openrust::database::eloquent::Model>() -> Self {
        Self {
            table: M::from_state(Default::default()).get_table(),
            connection: M::from_state(Default::default())
                .get_connection_name()
                .map(str::to_owned),
            save: save_model::<M>,
            save_in: save_model_in::<M>,
        }
    }
}
pub(crate) fn property<'a>(
    mut value: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(value) = value.get(path) {
        return Some(value);
    }
    for key in path.split('.') {
        value = match value {
            serde_json::Value::Array(array) => array.get(key.parse::<usize>().ok()?)?,
            _ => value.get(key)?,
        };
    }
    Some(value)
}

fn prepared_model<M: openrust::database::eloquent::Model>(
    connection: &openrust::database::Connection,
    activity: &crate::PreparedActivity,
) -> openrust::database::DatabaseResult<M> {
    let mut model = M::new_instance_on(connection, Default::default())?;
    for (key, value) in activity.row() {
        model.set_attribute(&key, value);
    }
    Ok(model)
}
fn save_model<'a, M: openrust::database::eloquent::Model>(
    connection: &'a openrust::database::Connection,
    activity: &'a crate::PreparedActivity,
) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<crate::ActivityRecord>>
{
    Box::pin(async move {
        let mut model = prepared_model::<M>(connection, activity)?;
        model.save().await?;
        crate::repository::decode_attributes(model.state().get_attributes()).map_err(|_| {
            openrust::database::DatabaseError::InvalidQuery("Invalid activity model attributes")
        })
    })
}
fn save_model_in<'a, M: openrust::database::eloquent::Model>(
    activity: &'a crate::PreparedActivity,
    tx: &'a mut openrust::database::Transaction,
) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<crate::ActivityRecord>>
{
    Box::pin(async move {
        let mut model = prepared_model::<M>(tx.get_connection(), activity)?;
        model.save_in_transaction(tx).await?;
        crate::repository::decode_attributes(model.state().get_attributes()).map_err(|_| {
            openrust::database::DatabaseError::InvalidQuery("Invalid activity model attributes")
        })
    })
}
