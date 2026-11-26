use crate::{
    Activity, ActivityLogStatus, ActivityRecord, ActivityRepository, CauserResolver, IntoLogName,
    Result, Subject,
};
use openrust::database::Connection;
use serde_json::Value;
use std::{future::Future, sync::Arc};

/// Fluent logger owned by one execution scope. `activity()` starts a fresh entry,
/// sharing only its scope's status and causer resolver.
pub struct ActivityLogger {
    repository: Arc<ActivityRepository>,
    status: ActivityLogStatus,
    causer_resolver: CauserResolver,
    default_log_name: Option<String>,
    activity: Activity,
    causer_was_set: bool,
    action: crate::actions::LogActivityAction,
    buffer: Option<Arc<crate::ActivityBuffer>>,
}
impl ActivityLogger {
    pub fn new(connection: Connection) -> Self {
        Self::from_repository(Arc::new(ActivityRepository::new(connection)))
    }
    pub fn from_repository(repository: Arc<ActivityRepository>) -> Self {
        let default_log_name = repository.default_log_name.clone();
        let mut activity = Activity::new("").use_log(default_log_name.clone());
        activity.description = None;
        Self {
            status: ActivityLogStatus::new(repository.is_enabled()),
            causer_resolver: CauserResolver::default(),
            activity,
            default_log_name,
            repository,
            causer_was_set: false,
            action: crate::actions::LogActivityAction::default(),
            buffer: None,
        }
    }
    pub fn with_buffer(&mut self, buffer: Arc<crate::ActivityBuffer>) -> &mut Self {
        self.buffer = Some(buffer);
        self
    }
    pub async fn flush(&self) -> Result<usize> {
        match &self.buffer {
            Some(buffer) => buffer.flush().await,
            None => Ok(0),
        }
    }
    pub fn set_log_activity_action(
        &mut self,
        action: crate::actions::LogActivityAction,
    ) -> &mut Self {
        self.action = action;
        self
    }
    pub fn set_log_status(&mut self, status: ActivityLogStatus) -> &mut Self {
        self.status = status;
        self
    }
    pub fn set_causer_resolver(&mut self, resolver: CauserResolver) -> &mut Self {
        self.causer_resolver = resolver;
        self
    }
    pub fn with_default_log_name(&mut self, name: impl IntoLogName) -> &mut Self {
        self.default_log_name = name.into_log_name();
        self.activity.log_name = self.default_log_name.clone();
        self
    }
    pub fn activity(&self) -> PendingActivityLog {
        let mut logger = Self::from_repository(self.repository.clone());
        logger.status = self.status.clone();
        logger.causer_resolver = self.causer_resolver.clone();
        logger.action = self.action.clone();
        logger.buffer = self.buffer.clone();
        logger.with_default_log_name(self.default_log_name.clone());
        PendingActivityLog { logger }
    }
    pub fn performed_on(&mut self, subject: Subject) -> &mut Self {
        self.activity.subject = Some(subject);
        self.activity.subject_attributes = serde_json::json!({});
        self
    }
    pub fn on(&mut self, subject: Subject) -> &mut Self {
        self.performed_on(subject)
    }
    pub fn caused_by(&mut self, subject: impl Into<Option<Subject>>) -> Result<&mut Self> {
        if let Some(subject) = subject.into() {
            self.activity.causer = self.causer_resolver.resolve(Some(subject))?;
            if let Some(causer) = &self.activity.causer {
                self.activity.causer_attributes = self.causer_resolver.attributes(causer)?;
            }
            self.causer_was_set = true;
        }
        Ok(self)
    }
    pub fn by_model<M: openrust::database::eloquent::Model>(
        &mut self,
        model: &M,
    ) -> Result<&mut Self> {
        let subject = crate::models::identity(model)?;
        self.caused_by(subject.clone())?;
        if self.activity.causer.as_ref() == Some(&subject) {
            self.activity.causer_attributes = model.to_array()?.into();
        }
        Ok(self)
    }
    pub fn on_model<M: openrust::database::eloquent::Model>(
        &mut self,
        model: &M,
    ) -> Result<&mut Self> {
        self.performed_on(crate::models::identity(model)?);
        self.activity.subject_attributes = model.to_array()?.into();
        Ok(self)
    }
    pub async fn by_id(
        &mut self,
        id: impl Into<openrust::auth::AuthIdentifier>,
    ) -> Result<&mut Self> {
        let (subject, attributes) = self.causer_resolver.resolve_id(id.into()).await?;
        self.activity.causer = subject;
        self.activity.causer_attributes = attributes;
        self.causer_was_set = true;
        Ok(self)
    }
    pub fn by(&mut self, subject: impl Into<Option<Subject>>) -> Result<&mut Self> {
        self.caused_by(subject)
    }
    pub fn caused_by_anonymous(&mut self) -> &mut Self {
        self.activity.causer = None;
        self.causer_was_set = true;
        self
    }
    pub fn by_anonymous(&mut self) -> &mut Self {
        self.caused_by_anonymous()
    }
    pub fn event(&mut self, event: impl Into<String>) -> &mut Self {
        self.set_event(event)
    }
    pub fn set_event(&mut self, event: impl Into<String>) -> &mut Self {
        self.activity.event = Some(event.into());
        self
    }
    pub fn with_changes(&mut self, changes: Value) -> &mut Self {
        self.activity.changes = changes;
        self
    }
    pub fn with_properties(&mut self, properties: Value) -> &mut Self {
        self.activity.properties = properties;
        self
    }
    pub fn with_property(&mut self, key: impl Into<String>, value: impl Into<Value>) -> &mut Self {
        if let Some(properties) = self.activity.properties.as_object_mut() {
            properties.insert(key.into(), value.into());
        }
        self
    }
    pub fn created_at(&mut self, time: jiff::Timestamp) -> &mut Self {
        self.activity.created_at = Some(time.as_millisecond());
        self
    }
    pub fn use_log(&mut self, name: impl IntoLogName) -> &mut Self {
        self.activity.log_name = name.into_log_name();
        self
    }
    pub fn in_log(&mut self, name: impl IntoLogName) -> &mut Self {
        self.use_log(name)
    }
    fn use_default_causer_if_none_was_given(&mut self) -> Result<()> {
        if !self.causer_was_set && self.activity.causer.is_none() {
            self.activity.causer = self.causer_resolver.resolve(None)?;
            if let Some(causer) = &self.activity.causer {
                self.activity.causer_attributes = self.causer_resolver.attributes(causer)?;
            }
        }
        Ok(())
    }
    pub fn tap(
        &mut self,
        callback: impl FnOnce(&mut Activity, Option<&str>),
        event: Option<&str>,
    ) -> Result<&mut Self> {
        self.use_default_causer_if_none_was_given()?;
        self.causer_was_set = true;
        callback(&mut self.activity, event);
        Ok(self)
    }
    pub fn enable_logging(&mut self) -> &mut Self {
        self.status.enable();
        self
    }
    pub fn disable_logging(&mut self) -> &mut Self {
        self.status.disable();
        self
    }
    pub async fn without_logging<F: Future>(&self, callback: impl FnOnce() -> F) -> F::Output {
        self.status.without_logging(callback).await
    }
    pub async fn log(&mut self, description: impl Into<String>) -> Result<Option<ActivityRecord>> {
        if self.status.disabled() {
            return Ok(None);
        }
        self.use_default_causer_if_none_was_given()?;
        self.action
            .execute(&mut self.activity, description.into())?;
        let record = if let Some(buffer) = &self.buffer {
            self.repository
                .prepare(self.activity.clone())?
                .map(|activity| buffer.add(activity))
        } else {
            self.repository.log(self.activity.clone()).await?
        };
        self.activity = Activity::new("").use_log(self.default_log_name.clone());
        self.activity.description = None;
        self.causer_was_set = false;
        Ok(record)
    }
}
pub struct PendingActivityLog {
    logger: ActivityLogger,
}
impl PendingActivityLog {
    pub fn logger(&mut self) -> &mut ActivityLogger {
        &mut self.logger
    }
    pub fn default_causer(&mut self, causer: Option<Subject>) -> &mut Self {
        self.logger.causer_resolver.set_causer(causer);
        self
    }
}
impl std::ops::Deref for PendingActivityLog {
    type Target = ActivityLogger;
    fn deref(&self) -> &Self::Target {
        &self.logger
    }
}
impl std::ops::DerefMut for PendingActivityLog {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.logger
    }
}

#[derive(Clone)]
pub struct ActivityLogContext {
    repository: Arc<ActivityRepository>,
    status: ActivityLogStatus,
    causer_resolver: CauserResolver,
    default_log_name: Option<String>,
    action: crate::actions::LogActivityAction,
    buffer: Option<Arc<crate::ActivityBuffer>>,
}
tokio::task_local! { static CONTEXT: ActivityLogContext; }
impl ActivityLogger {
    pub async fn scope<F: Future>(&self, work: F) -> F::Output {
        let context = ActivityLogContext {
            repository: self.repository.clone(),
            status: self.status.clone(),
            causer_resolver: self.causer_resolver.clone(),
            default_log_name: self.default_log_name.clone(),
            action: self.action.clone(),
            buffer: self.buffer.clone(),
        };
        CONTEXT.scope(context, work).await
    }
    pub(crate) fn current() -> Option<Self> {
        CONTEXT
            .try_with(|context| {
                let mut logger = Self::from_repository(context.repository.clone());
                logger.status = context.status.clone();
                logger.causer_resolver = context.causer_resolver.clone();
                logger.default_log_name = context.default_log_name.clone();
                logger.activity.log_name = context.default_log_name.clone();
                logger.action = context.action.clone();
                logger.buffer = context.buffer.clone();
                logger
            })
            .ok()
    }
    pub(crate) fn buffer(&self) -> Option<Arc<crate::ActivityBuffer>> {
        self.buffer.clone()
    }
    pub(crate) fn buffer_prepared(
        &self,
        activity: crate::PreparedActivity,
    ) -> Option<ActivityRecord> {
        self.buffer.as_ref().map(|buffer| buffer.add(activity))
    }
    pub(crate) fn prepare_model(
        &mut self,
        mut activity: Activity,
        before: impl FnOnce(&mut Activity) -> Result<()>,
    ) -> Result<Option<crate::PreparedActivity>> {
        if self.status.disabled() {
            return Ok(None);
        }
        activity.causer = self.causer_resolver.resolve(None)?;
        if let Some(causer) = &activity.causer {
            activity.causer_attributes = self.causer_resolver.attributes(causer)?;
        }
        if activity.log_name.is_none() {
            activity.log_name = self.default_log_name.clone();
        }
        let description = activity.description.take().unwrap_or_default();
        self.action
            .execute_with(&mut activity, description, before)?;
        self.repository.prepare(activity)
    }
}

impl ActivityLogContext {
    pub(crate) fn new(
        repository: Arc<ActivityRepository>,
        status: ActivityLogStatus,
        causer_resolver: CauserResolver,
        action: crate::actions::LogActivityAction,
    ) -> Self {
        Self {
            default_log_name: repository.default_log_name.clone(),
            repository,
            status,
            causer_resolver,
            action,
            buffer: None,
        }
    }
    pub fn with_buffer(mut self, buffer: Arc<crate::ActivityBuffer>) -> Self {
        self.buffer = Some(buffer);
        self
    }
    pub fn logger(&self) -> ActivityLogger {
        let mut logger = ActivityLogger::from_repository(self.repository.clone());
        logger
            .set_log_status(self.status.clone())
            .set_causer_resolver(self.causer_resolver.clone())
            .set_log_activity_action(self.action.clone())
            .with_default_log_name(self.default_log_name.clone());
        logger.buffer = self.buffer.clone();
        logger
    }
    pub fn repository(&self) -> Arc<ActivityRepository> {
        self.repository.clone()
    }
}
