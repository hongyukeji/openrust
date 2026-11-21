//! State owned by one request, command, or job. Never store mutable logging state globally.
use crate::{IntoLogName, Result, Subject};
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone)]
pub struct ActivityLogStatus(Arc<AtomicBool>);
impl Default for ActivityLogStatus {
    fn default() -> Self {
        Self::new(true)
    }
}
impl ActivityLogStatus {
    pub fn new(enabled: bool) -> Self {
        Self(Arc::new(AtomicBool::new(enabled)))
    }
    pub fn enable(&self) -> bool {
        self.0.store(true, Ordering::SeqCst);
        true
    }
    pub fn disable(&self) -> bool {
        self.0.store(false, Ordering::SeqCst);
        false
    }
    pub fn disabled(&self) -> bool {
        !self.0.load(Ordering::SeqCst)
    }
    /// Restores the prior status on success, error, panic, and future cancellation.
    pub async fn withoutLogging<F: Future>(&self, callback: impl FnOnce() -> F) -> F::Output {
        if self.disabled() {
            return callback().await;
        }
        let _restore = RestoreStatus(self.clone(), true);
        self.disable();
        callback().await
    }
}
struct RestoreStatus(ActivityLogStatus, bool);
impl Drop for RestoreStatus {
    fn drop(&mut self) {
        self.0.0.store(self.1, Ordering::SeqCst);
    }
}

type IdResolver = Arc<
    dyn Fn(
            openrust::auth::AuthIdentifier,
        )
            -> std::pin::Pin<Box<dyn Future<Output = Result<(Subject, serde_json::Value)>> + Send>>
        + Send
        + Sync,
>;
type AttributeResolver = Arc<dyn Fn(&Subject) -> Result<serde_json::Value> + Send + Sync>;
type Resolver = Arc<dyn Fn(Option<Subject>) -> Result<Option<Subject>> + Send + Sync>;
#[derive(Clone, Default)]
pub struct CauserResolver {
    default: Option<Subject>,
    idResolver: Option<IdResolver>,
    attributesResolver: Option<AttributeResolver>,
    defaultResolver: Option<Arc<dyn Fn() -> Result<Option<Subject>> + Send + Sync>>,
    resolverOverride: Option<Resolver>,
    causerOverride: Arc<Mutex<Option<Subject>>>,
}
impl CauserResolver {
    pub fn new(default: Option<Subject>) -> Self {
        Self {
            default,
            ..Self::default()
        }
    }
    pub fn resolve(&self, subject: Option<Subject>) -> Result<Option<Subject>> {
        if let Some(causer) = self.causerOverride.lock().unwrap().clone() {
            return Ok(Some(causer));
        }
        if let Some(resolve) = &self.resolverOverride {
            return resolve(subject);
        }
        if let Some(subject) = subject {
            return Ok(Some(subject));
        }
        if let Some(resolve) = &self.defaultResolver {
            return resolve();
        }
        Ok(self.default.clone())
    }
    pub(crate) fn withIdentityResolvers(
        mut self,
        id: IdResolver,
        attributes: AttributeResolver,
    ) -> Self {
        self.idResolver = Some(id);
        self.attributesResolver = Some(attributes);
        self
    }
    pub async fn resolveId(
        &self,
        id: openrust::auth::AuthIdentifier,
    ) -> Result<(Option<Subject>, serde_json::Value)> {
        if self.causerOverride.lock().unwrap().is_some() || self.resolverOverride.is_some() {
            let subject = self.resolve(None)?;
            let attributes = subject.as_ref().map_or_else(
                || Ok(serde_json::json!({})),
                |subject| self.attributes(subject),
            )?;
            return Ok((subject, attributes));
        }
        let (subject, attributes) = self
            .idResolver
            .as_ref()
            .ok_or(crate::Error::InvalidConfiguration)?(id)
        .await?;
        Ok((Some(subject), attributes))
    }
    pub(crate) fn attributes(&self, subject: &Subject) -> Result<serde_json::Value> {
        self.attributesResolver
            .as_ref()
            .map_or_else(|| Ok(serde_json::json!({})), |resolve| resolve(subject))
    }
    pub(crate) fn withDefaultResolver(
        mut self,
        callback: impl Fn() -> Result<Option<Subject>> + Send + Sync + 'static,
    ) -> Self {
        self.defaultResolver = Some(Arc::new(callback));
        self
    }
    pub fn resolveUsing(
        mut self,
        callback: impl Fn(Option<Subject>) -> Result<Option<Subject>> + Send + Sync + 'static,
    ) -> Self {
        self.resolverOverride = Some(Arc::new(callback));
        self
    }
    pub fn setCauser(&self, causer: Option<Subject>) -> &Self {
        *self.causerOverride.lock().unwrap() = causer;
        self
    }
    pub async fn withCauser<F: Future>(
        &self,
        causer: Option<Subject>,
        callback: impl FnOnce() -> F,
    ) -> F::Output {
        let previous = std::mem::replace(&mut *self.causerOverride.lock().unwrap(), causer);
        let _restore = RestoreCauser(self.clone(), previous);
        callback().await
    }
}
struct RestoreCauser(CauserResolver, Option<Subject>);
impl Drop for RestoreCauser {
    fn drop(&mut self) {
        *self.0.causerOverride.lock().unwrap() = self.1.take();
    }
}

pub type DescriptionForEvent = Arc<dyn Fn(&str) -> String + Send + Sync>;
#[derive(Clone)]
pub struct LogOptions {
    pub logName: Option<String>,
    pub logEmptyChanges: bool,
    pub logFillable: bool,
    pub logOnlyDirty: bool,
    pub logUnguarded: bool,
    pub logAttributes: Vec<String>,
    pub logExceptAttributes: Vec<String>,
    pub dontLogIfAttributesChangedOnly: Vec<String>,
    pub attributeRawValues: Vec<String>,
    pub descriptionForEvent: Option<DescriptionForEvent>,
}
impl Default for LogOptions {
    fn default() -> Self {
        Self {
            logName: None,
            logEmptyChanges: true,
            logFillable: false,
            logOnlyDirty: false,
            logUnguarded: false,
            logAttributes: vec![],
            logExceptAttributes: vec![],
            dontLogIfAttributesChangedOnly: vec![],
            attributeRawValues: vec![],
            descriptionForEvent: None,
        }
    }
}
impl LogOptions {
    pub fn defaults() -> Self {
        Self::default()
    }
    pub fn logAll(self) -> Self {
        self.logOnly(&["*"])
    }
    pub fn logUnguarded(mut self) -> Self {
        self.logUnguarded = true;
        self
    }
    pub fn logFillable(mut self) -> Self {
        self.logFillable = true;
        self
    }
    pub fn dontLogFillable(mut self) -> Self {
        self.logFillable = false;
        self
    }
    pub fn logOnlyDirty(mut self) -> Self {
        self.logOnlyDirty = true;
        self
    }
    pub fn logOnly(mut self, attributes: &[&str]) -> Self {
        self.logAttributes = strings(attributes);
        self
    }
    pub fn logExcept(mut self, attributes: &[&str]) -> Self {
        self.logExceptAttributes = strings(attributes);
        self
    }
    pub fn dontLogIfAttributesChangedOnly(mut self, attributes: &[&str]) -> Self {
        self.dontLogIfAttributesChangedOnly = strings(attributes);
        self
    }
    pub fn dontLogEmptyChanges(mut self) -> Self {
        self.logEmptyChanges = false;
        self
    }
    pub fn logEmptyChanges(mut self) -> Self {
        self.logEmptyChanges = true;
        self
    }
    pub fn useLogName(mut self, name: impl IntoLogName) -> Self {
        self.logName = name.intoLogName();
        self
    }
    pub fn setDescriptionForEvent(
        mut self,
        callback: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> Self {
        self.descriptionForEvent = Some(Arc::new(callback));
        self
    }
    pub fn useAttributeRawValues(mut self, attributes: &[&str]) -> Self {
        self.attributeRawValues = strings(attributes);
        self
    }
}
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
