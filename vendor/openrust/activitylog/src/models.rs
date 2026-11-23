// Compiled model identities preserve the application's morph map.
pub(crate) fn identity<M: openrust::database::eloquent::Model>(
    model: &M,
) -> crate::Result<crate::Subject> {
    use openrust::database::Value as Sql;
    let id = match model.getKey() {
        Some(Sql::Text(value)) => value.clone(),
        Some(Sql::Integer(value)) => value.to_string(),
        Some(Sql::Unsigned(value)) => value.to_string(),
        _ => return Err(crate::Error::InvalidActivity),
    };
    crate::Subject::new(model.getMorphClass()?, id)
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

    /// Implement on a model, then register `bootLogsActivity` in the application's Provider.
    pub trait LogsActivity: Model + Clone {
        fn activitylogState(&self) -> &super::LoggingState;
        fn disableLogging(&mut self) -> &mut Self {
            self.activitylogState()
                .0
                .store(false, std::sync::atomic::Ordering::Relaxed);
            self
        }
        fn enableLogging(&mut self) -> &mut Self {
            self.activitylogState()
                .0
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self
        }
        fn getActivitylogOptions(&self) -> LogOptions {
            LogOptions::defaults()
        }
        fn enableLoggingModelsEvents(&self) -> bool {
            self.activitylogState()
                .0
                .load(std::sync::atomic::Ordering::Relaxed)
        }
        fn eventsToBeRecorded() -> &'static [ModelEvent] {
            &[
                ModelEvent::Created,
                ModelEvent::Updated,
                ModelEvent::Deleted,
                ModelEvent::Restored,
            ]
        }
        fn doNotRecordEvents() -> &'static [ModelEvent] {
            &[]
        }
        fn bootLogsActivity(
            app: &Application,
            repository: Arc<ActivityRepository>,
        ) -> DatabaseResult<()> {
            for event in Self::eventsToBeRecorded() {
                if !Self::doNotRecordEvents().contains(event) {
                    Self::registerModelEvent(
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
        fn attributesToBeLogged(&self) -> Vec<String> {
            self.attributesToBeLoggedWith(&[])
        }
        fn attributesToBeLoggedWith(&self, defaults: &[String]) -> Vec<String> {
            let options = self.getActivitylogOptions();
            let mut attributes = BTreeSet::new();
            if options.logFillable {
                attributes.extend(self.getFillable().iter().map(|name| (*name).to_owned()));
            }
            if options.logUnguarded {
                let guarded = self.getGuarded();
                if Self::isUnguarded() || !guarded.contains(&"*") {
                    attributes.extend(
                        self.state()
                            .getAttributes()
                            .keys()
                            .filter(|name| Self::isUnguarded() || !guarded.contains(&name.as_str()))
                            .cloned(),
                    );
                }
            }
            if options.logAttributes.iter().any(|name| name == "*") {
                attributes.extend(self.state().getAttributes().keys().cloned());
            }
            attributes.extend(
                options
                    .logAttributes
                    .iter()
                    .filter(|name| name.as_str() != "*")
                    .cloned(),
            );
            attributes.retain(|name| {
                !options.logExceptAttributes.contains(name) && !defaults.contains(name)
            });
            attributes.into_iter().collect()
        }
        fn getDescriptionForEvent(&self, event: &str) -> String {
            self.getActivitylogOptions()
                .descriptionForEvent
                .map_or_else(|| event.into(), |callback| callback(event))
        }
        fn beforeActivityLogged(&self, _activity: &mut Activity, _event: &str) -> Result<()> {
            Ok(())
        }
        fn activitiesAsSubject(&self, repository: &ActivityRepository) -> Result<ActivityQuery> {
            Ok(repository.query().forSubject(&super::identity(self)?))
        }
        fn extractChanges(&self) -> Result<Map<String, Value>> {
            self.extractChangesWithDefaults(&[])
        }
        fn extractChangesWithDefaults(&self, defaults: &[String]) -> Result<Map<String, Value>> {
            let options = self.getActivitylogOptions();
            let mut values = Map::new();
            for attribute in self.attributesToBeLoggedWith(defaults) {
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
                        .getAttributeValue(base)?
                        .as_ref()
                        .map(ChangeDetector::json)
                        .transpose()?
                        .unwrap_or(Value::Null);
                    let root = Map::from_iter([(base.into(), value)]);
                    let value = ChangeDetector::resolveJsonAttribute(&root, &attribute);
                    setPath(
                        &mut values,
                        &attribute.split("->").collect::<Vec<_>>(),
                        value,
                    );
                } else {
                    let raw = if options.attributeRawValues.contains(&attribute) {
                        self.getAttribute(&attribute).cloned()
                    } else {
                        self.getAttributeValue(&attribute)?
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
    fn setPath(values: &mut Map<String, Value>, path: &[&str], value: Value) {
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
        setPath(child.as_object_mut().unwrap(), &path[1..], value);
    }
    fn eventName(event: ModelEvent) -> Option<&'static str> {
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
        let Some(name) = eventName(event) else {
            return Ok(None);
        };
        if !model.enableLoggingModelsEvents() {
            return Ok(None);
        }
        let options = model.getActivitylogOptions();
        if matches!(event, ModelEvent::Created | ModelEvent::Updated) {
            let dirty = model.getDirty()?;
            if !dirty
                .keys()
                .any(|name| !options.dontLogIfAttributesChangedOnly.contains(name))
            {
                return Ok(None);
            }
            // Restoration triggers an update as well; record only its restored event.
            if model.getDeletedAtColumn().is_some_and(|column| {
                model
                    .state()
                    .getOriginal()
                    .get(column)
                    .is_some_and(|value| !matches!(value, openrust::database::Value::Null))
                    && matches!(
                        model.getAttribute(column),
                        Some(openrust::database::Value::Null)
                    )
            }) {
                return Ok(None);
            }
        }
        let description = model.getDescriptionForEvent(name);
        if description.is_empty() {
            return Ok(None);
        }
        let attributes = model.extractChangesWithDefaults(defaults)?;
        let changes = if model.attributesToBeLoggedWith(defaults).is_empty() {
            serde_json::json!({})
        } else if event == ModelEvent::Deleted {
            serde_json::json!({"old": attributes})
        } else if event == ModelEvent::Updated {
            let mut old = M::newInstanceOn(&model.getConnection()?, Default::default())?;
            for (name, value) in model.state().getOriginal() {
                old.state_mut().setAttribute(name, value.clone());
            }
            loadRelated(&mut old, defaults, transaction).await?;
            let mut old = old.extractChangesWithDefaults(defaults)?;
            for name in attributes.keys() {
                old.entry(name.clone()).or_insert(Value::Null);
            }
            if options.logOnlyDirty {
                ChangeDetector::filterDirty(&attributes, &old)
            } else {
                serde_json::json!({"attributes":attributes,"old":old})
            }
        } else {
            serde_json::json!({"attributes":attributes})
        };
        if !options.logEmptyChanges
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
            .performedOn(super::identity(model)?);
        activity.log_name = options.logName;
        activity.changes = changes;
        activity.subjectAttributes = model
            .state()
            .getAttributes()
            .iter()
            .map(|(key, value)| Ok((key.clone(), ChangeDetector::json(value)?)))
            .collect::<Result<Map<_, _>>>()?
            .into();
        Ok(Some(activity))
    }
    async fn buildStored<M: LogsActivity>(
        model: &M,
        event: ModelEvent,
        mut transaction: Option<&mut Transaction>,
        defaults: &[String],
    ) -> Result<Option<Activity>> {
        if !model.enableLoggingModelsEvents() {
            return Ok(None);
        }
        let mut snapshot = model.clone();
        if event != ModelEvent::Retrieved
            && model.state().exists()
            && let Some(key) = model.getKey()
        {
            let row = match transaction.as_deref_mut() {
                Some(tx) => {
                    tx.table(model.getTable())
                        .r#where(model.getKeyName(), "=", key.clone())
                        .first()
                        .await?
                }
                None => {
                    model
                        .getConnection()?
                        .table(model.getTable())
                        .r#where(model.getKeyName(), "=", key.clone())
                        .first()
                        .await?
                }
            };
            if let Some(row) = row {
                for (name, value) in row.columns() {
                    snapshot.state_mut().setAttribute(name, value.clone());
                }
            }
        }
        loadRelated(&mut snapshot, defaults, transaction.as_deref_mut()).await?;
        build(&snapshot, event, defaults, transaction).await
    }
    async fn loadRelated<M: LogsActivity>(
        model: &mut M,
        defaults: &[String],
        transaction: Option<&mut Transaction>,
    ) -> Result<()> {
        let paths: Vec<String> = model
            .attributesToBeLoggedWith(defaults)
            .iter()
            .filter_map(|attribute| attribute.rsplit_once('.').map(|(path, _)| path.to_owned()))
            .collect();
        if !paths.is_empty() {
            let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
            match transaction {
                Some(tx) => model.loadPathsInTransaction(&paths, tx).await?,
                None => model.load_missing_paths(&paths).await?,
            }
        }
        Ok(())
    }
    // Match the upstream current-savepoint marker plus outermost ordered save.
    fn saveAfterCommit(
        transaction: &mut Transaction,
        prepared: crate::PreparedActivity,
        repository: Arc<ActivityRepository>,
        buffer: Option<Arc<crate::ActivityBuffer>>,
    ) -> DatabaseResult<()> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let committed = Arc::new(AtomicBool::new(false));
        let marker = committed.clone();
        transaction.afterCommit(move || {
            Box::pin(async move {
                marker.store(true, Ordering::SeqCst);
                Ok(())
            })
        })?;
        transaction.afterCommitAtLevel(1, move || {
            Box::pin(async move {
                if committed.load(Ordering::SeqCst) {
                    if let Some(buffer) = buffer {
                        buffer.add(prepared);
                    } else {
                        repository.savePrepared(prepared).await.map_err(error)?;
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
                if let Some(activity) =
                    buildStored(model, event, None, &self.repository.defaultExceptAttributes)
                        .await
                        .map_err(error)?
                {
                    let mut logger = ActivityLogger::current()
                        .or_else(|| crate::facades::Activitylog::logger().ok())
                        .unwrap_or_else(|| ActivityLogger::fromRepository(self.repository.clone()));
                    if let Some(prepared) = logger
                        .prepareModel(activity, |activity| {
                            model.beforeActivityLogged(activity, eventName(event).unwrap_or(""))
                        })
                        .map_err(error)?
                    {
                        if logger.bufferPrepared(prepared.clone()).is_some() {
                            return Ok(ModelEventResponse::Continue);
                        }
                        self.repository
                            .savePrepared(prepared)
                            .await
                            .map_err(error)?;
                    }
                }
                Ok(ModelEventResponse::Continue)
            })
        }
        fn handleInTransaction<'a>(
            &'a self,
            event: ModelEvent,
            model: &'a mut M,
            transaction: &'a mut Transaction,
        ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
            Box::pin(async move {
                if let Some(activity) = buildStored(
                    model,
                    event,
                    Some(&mut *transaction),
                    &self.repository.defaultExceptAttributes,
                )
                .await
                .map_err(error)?
                {
                    let mut logger = ActivityLogger::current()
                        .or_else(|| crate::facades::Activitylog::logger().ok())
                        .unwrap_or_else(|| ActivityLogger::fromRepository(self.repository.clone()));
                    if let Some(prepared) = logger
                        .prepareModel(activity, |activity| {
                            model.beforeActivityLogged(activity, eventName(event).unwrap_or(""))
                        })
                        .map_err(error)?
                    {
                        if let Some(buffer) = logger.buffer() {
                            saveAfterCommit(
                                transaction,
                                prepared,
                                self.repository.clone(),
                                Some(buffer),
                            )?;
                            return Ok(ModelEventResponse::Continue);
                        }
                        if self.repository.usesConnection(&model.getConnection()?) {
                            prepared.logIn(transaction).await?;
                        } else {
                            saveAfterCommit(transaction, prepared, self.repository.clone(), None)?;
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
                .targetMapped::<R>(
                    &self.getConnection()?,
                    R::from_state(Default::default()).getKeyName(),
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
            .targetMapped::<R>(
                &self.getConnection()?,
                R::from_state(Default::default()).getKeyName(),
            )
    }
    pub fn getProperty(
        &self,
        path: &str,
        default: serde_json::Value,
    ) -> crate::Result<serde_json::Value> {
        use openrust::database::eloquent::Model;
        let value = self
            .getAttributeValue("properties")?
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
    pub(crate) saveIn: for<'a> fn(
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
            table: M::from_state(Default::default()).getTable(),
            connection: M::from_state(Default::default())
                .getConnectionName()
                .map(str::to_owned),
            save: saveModel::<M>,
            saveIn: saveModelIn::<M>,
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

fn preparedModel<M: openrust::database::eloquent::Model>(
    connection: &openrust::database::Connection,
    activity: &crate::PreparedActivity,
) -> openrust::database::DatabaseResult<M> {
    let mut model = M::newInstanceOn(connection, Default::default())?;
    for (key, value) in activity.row() {
        model.setAttribute(&key, value);
    }
    Ok(model)
}
fn saveModel<'a, M: openrust::database::eloquent::Model>(
    connection: &'a openrust::database::Connection,
    activity: &'a crate::PreparedActivity,
) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<crate::ActivityRecord>>
{
    Box::pin(async move {
        let mut model = preparedModel::<M>(connection, activity)?;
        model.save().await?;
        crate::repository::decodeAttributes(model.state().getAttributes()).map_err(|_| {
            openrust::database::DatabaseError::InvalidQuery("Invalid activity model attributes")
        })
    })
}
fn saveModelIn<'a, M: openrust::database::eloquent::Model>(
    activity: &'a crate::PreparedActivity,
    tx: &'a mut openrust::database::Transaction,
) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<crate::ActivityRecord>>
{
    Box::pin(async move {
        let mut model = preparedModel::<M>(tx.getConnection(), activity)?;
        model.saveInTransaction(tx).await?;
        crate::repository::decodeAttributes(model.state().getAttributes()).map_err(|_| {
            openrust::database::DatabaseError::InvalidQuery("Invalid activity model attributes")
        })
    })
}
