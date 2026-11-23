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
    causerResolver: CauserResolver,
    defaultLogName: Option<String>,
    activity: Activity,
    causerWasSet: bool,
    action: crate::actions::LogActivityAction,
    buffer: Option<Arc<crate::ActivityBuffer>>,
}
impl ActivityLogger {
    pub fn new(connection: Connection) -> Self {
        Self::fromRepository(Arc::new(ActivityRepository::new(connection)))
    }
    pub fn fromRepository(repository: Arc<ActivityRepository>) -> Self {
        let defaultLogName = repository.defaultLogName.clone();
        let mut activity = Activity::new("").useLog(defaultLogName.clone());
        activity.description = None;
        Self {
            status: ActivityLogStatus::new(repository.isEnabled()),
            causerResolver: CauserResolver::default(),
            activity,
            defaultLogName,
            repository,
            causerWasSet: false,
            action: crate::actions::LogActivityAction::default(),
            buffer: None,
        }
    }
    pub fn withBuffer(&mut self, buffer: Arc<crate::ActivityBuffer>) -> &mut Self {
        self.buffer = Some(buffer);
        self
    }
    pub async fn flush(&self) -> Result<usize> {
        match &self.buffer {
            Some(buffer) => buffer.flush().await,
            None => Ok(0),
        }
    }
    pub fn setLogActivityAction(&mut self, action: crate::actions::LogActivityAction) -> &mut Self {
        self.action = action;
        self
    }
    pub fn setLogStatus(&mut self, status: ActivityLogStatus) -> &mut Self {
        self.status = status;
        self
    }
    pub fn setCauserResolver(&mut self, resolver: CauserResolver) -> &mut Self {
        self.causerResolver = resolver;
        self
    }
    pub fn withDefaultLogName(&mut self, name: impl IntoLogName) -> &mut Self {
        self.defaultLogName = name.intoLogName();
        self.activity.log_name = self.defaultLogName.clone();
        self
    }
    pub fn activity(&self) -> PendingActivityLog {
        let mut logger = Self::fromRepository(self.repository.clone());
        logger.status = self.status.clone();
        logger.causerResolver = self.causerResolver.clone();
        logger.action = self.action.clone();
        logger.buffer = self.buffer.clone();
        logger.withDefaultLogName(self.defaultLogName.clone());
        PendingActivityLog { logger }
    }
    pub fn performedOn(&mut self, subject: Subject) -> &mut Self {
        self.activity.subject = Some(subject);
        self.activity.subjectAttributes = serde_json::json!({});
        self
    }
    pub fn on(&mut self, subject: Subject) -> &mut Self {
        self.performedOn(subject)
    }
    pub fn causedBy(&mut self, subject: impl Into<Option<Subject>>) -> Result<&mut Self> {
        if let Some(subject) = subject.into() {
            self.activity.causer = self.causerResolver.resolve(Some(subject))?;
            if let Some(causer) = &self.activity.causer {
                self.activity.causerAttributes = self.causerResolver.attributes(causer)?;
            }
            self.causerWasSet = true;
        }
        Ok(self)
    }
    pub fn byModel<M: openrust::database::eloquent::Model>(
        &mut self,
        model: &M,
    ) -> Result<&mut Self> {
        let subject = crate::models::identity(model)?;
        self.causedBy(subject.clone())?;
        if self.activity.causer.as_ref() == Some(&subject) {
            self.activity.causerAttributes = model.to_array()?.into();
        }
        Ok(self)
    }
    pub fn onModel<M: openrust::database::eloquent::Model>(
        &mut self,
        model: &M,
    ) -> Result<&mut Self> {
        self.performedOn(crate::models::identity(model)?);
        self.activity.subjectAttributes = model.to_array()?.into();
        Ok(self)
    }
    pub async fn byId(
        &mut self,
        id: impl Into<openrust::auth::AuthIdentifier>,
    ) -> Result<&mut Self> {
        let (subject, attributes) = self.causerResolver.resolveId(id.into()).await?;
        self.activity.causer = subject;
        self.activity.causerAttributes = attributes;
        self.causerWasSet = true;
        Ok(self)
    }
    pub fn by(&mut self, subject: impl Into<Option<Subject>>) -> Result<&mut Self> {
        self.causedBy(subject)
    }
    pub fn causedByAnonymous(&mut self) -> &mut Self {
        self.activity.causer = None;
        self.causerWasSet = true;
        self
    }
    pub fn byAnonymous(&mut self) -> &mut Self {
        self.causedByAnonymous()
    }
    pub fn event(&mut self, event: impl Into<String>) -> &mut Self {
        self.setEvent(event)
    }
    pub fn setEvent(&mut self, event: impl Into<String>) -> &mut Self {
        self.activity.event = Some(event.into());
        self
    }
    pub fn withChanges(&mut self, changes: Value) -> &mut Self {
        self.activity.changes = changes;
        self
    }
    pub fn withProperties(&mut self, properties: Value) -> &mut Self {
        self.activity.properties = properties;
        self
    }
    pub fn withProperty(&mut self, key: impl Into<String>, value: impl Into<Value>) -> &mut Self {
        if let Some(properties) = self.activity.properties.as_object_mut() {
            properties.insert(key.into(), value.into());
        }
        self
    }
    pub fn createdAt(&mut self, time: jiff::Timestamp) -> &mut Self {
        self.activity.created_at = Some(time.as_millisecond());
        self
    }
    pub fn useLog(&mut self, name: impl IntoLogName) -> &mut Self {
        self.activity.log_name = name.intoLogName();
        self
    }
    pub fn inLog(&mut self, name: impl IntoLogName) -> &mut Self {
        self.useLog(name)
    }
    fn useDefaultCauserIfNoneWasGiven(&mut self) -> Result<()> {
        if !self.causerWasSet && self.activity.causer.is_none() {
            self.activity.causer = self.causerResolver.resolve(None)?;
            if let Some(causer) = &self.activity.causer {
                self.activity.causerAttributes = self.causerResolver.attributes(causer)?;
            }
        }
        Ok(())
    }
    pub fn tap(
        &mut self,
        callback: impl FnOnce(&mut Activity, Option<&str>),
        event: Option<&str>,
    ) -> Result<&mut Self> {
        self.useDefaultCauserIfNoneWasGiven()?;
        self.causerWasSet = true;
        callback(&mut self.activity, event);
        Ok(self)
    }
    pub fn enableLogging(&mut self) -> &mut Self {
        self.status.enable();
        self
    }
    pub fn disableLogging(&mut self) -> &mut Self {
        self.status.disable();
        self
    }
    pub async fn withoutLogging<F: Future>(&self, callback: impl FnOnce() -> F) -> F::Output {
        self.status.withoutLogging(callback).await
    }
    pub async fn log(&mut self, description: impl Into<String>) -> Result<Option<ActivityRecord>> {
        if self.status.disabled() {
            return Ok(None);
        }
        self.useDefaultCauserIfNoneWasGiven()?;
        self.action
            .execute(&mut self.activity, description.into())?;
        let record = if let Some(buffer) = &self.buffer {
            self.repository
                .prepare(self.activity.clone())?
                .map(|activity| buffer.add(activity))
        } else {
            self.repository.log(self.activity.clone()).await?
        };
        self.activity = Activity::new("").useLog(self.defaultLogName.clone());
        self.activity.description = None;
        self.causerWasSet = false;
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
    pub fn defaultCauser(&mut self, causer: Option<Subject>) -> &mut Self {
        self.logger.causerResolver.setCauser(causer);
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
    causerResolver: CauserResolver,
    defaultLogName: Option<String>,
    action: crate::actions::LogActivityAction,
    buffer: Option<Arc<crate::ActivityBuffer>>,
}
tokio::task_local! { static CONTEXT: ActivityLogContext; }
impl ActivityLogger {
    pub async fn scope<F: Future>(&self, work: F) -> F::Output {
        let context = ActivityLogContext {
            repository: self.repository.clone(),
            status: self.status.clone(),
            causerResolver: self.causerResolver.clone(),
            defaultLogName: self.defaultLogName.clone(),
            action: self.action.clone(),
            buffer: self.buffer.clone(),
        };
        CONTEXT.scope(context, work).await
    }
    pub(crate) fn current() -> Option<Self> {
        CONTEXT
            .try_with(|context| {
                let mut logger = Self::fromRepository(context.repository.clone());
                logger.status = context.status.clone();
                logger.causerResolver = context.causerResolver.clone();
                logger.defaultLogName = context.defaultLogName.clone();
                logger.activity.log_name = context.defaultLogName.clone();
                logger.action = context.action.clone();
                logger.buffer = context.buffer.clone();
                logger
            })
            .ok()
    }
    pub(crate) fn buffer(&self) -> Option<Arc<crate::ActivityBuffer>> {
        self.buffer.clone()
    }
    pub(crate) fn bufferPrepared(
        &self,
        activity: crate::PreparedActivity,
    ) -> Option<ActivityRecord> {
        self.buffer.as_ref().map(|buffer| buffer.add(activity))
    }
    pub(crate) fn prepareModel(
        &mut self,
        mut activity: Activity,
        before: impl FnOnce(&mut Activity) -> Result<()>,
    ) -> Result<Option<crate::PreparedActivity>> {
        if self.status.disabled() {
            return Ok(None);
        }
        activity.causer = self.causerResolver.resolve(None)?;
        if let Some(causer) = &activity.causer {
            activity.causerAttributes = self.causerResolver.attributes(causer)?;
        }
        if activity.log_name.is_none() {
            activity.log_name = self.defaultLogName.clone();
        }
        let description = activity.description.take().unwrap_or_default();
        self.action
            .executeWith(&mut activity, description, before)?;
        self.repository.prepare(activity)
    }
}

impl ActivityLogContext {
    pub(crate) fn new(
        repository: Arc<ActivityRepository>,
        status: ActivityLogStatus,
        causerResolver: CauserResolver,
        action: crate::actions::LogActivityAction,
    ) -> Self {
        Self {
            defaultLogName: repository.defaultLogName.clone(),
            repository,
            status,
            causerResolver,
            action,
            buffer: None,
        }
    }
    pub fn withBuffer(mut self, buffer: Arc<crate::ActivityBuffer>) -> Self {
        self.buffer = Some(buffer);
        self
    }
    pub fn logger(&self) -> ActivityLogger {
        let mut logger = ActivityLogger::fromRepository(self.repository.clone());
        logger
            .setLogStatus(self.status.clone())
            .setCauserResolver(self.causerResolver.clone())
            .setLogActivityAction(self.action.clone())
            .withDefaultLogName(self.defaultLogName.clone());
        logger.buffer = self.buffer.clone();
        logger
    }
    pub fn repository(&self) -> Arc<ActivityRepository> {
        self.repository.clone()
    }
}
