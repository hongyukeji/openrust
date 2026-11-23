//! State owned by one request, command, or job. Never store mutable logging state globally.
pub use crate::{ActivityLogger, ChangeDetector, PendingActivityLog};
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
    pub async fn without_logging<F: Future>(&self, callback: impl FnOnce() -> F) -> F::Output {
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
    id_resolver: Option<IdResolver>,
    attributes_resolver: Option<AttributeResolver>,
    default_resolver: Option<Arc<dyn Fn() -> Result<Option<Subject>> + Send + Sync>>,
    resolver_override: Option<Resolver>,
    causer_override: Arc<Mutex<Option<Subject>>>,
}
impl CauserResolver {
    pub fn new(default: Option<Subject>) -> Self {
        Self {
            default,
            ..Self::default()
        }
    }
    pub fn resolve(&self, subject: Option<Subject>) -> Result<Option<Subject>> {
        if let Some(causer) = self.causer_override.lock().unwrap().clone() {
            return Ok(Some(causer));
        }
        if let Some(resolve) = &self.resolver_override {
            return resolve(subject);
        }
        if let Some(subject) = subject {
            return Ok(Some(subject));
        }
        if let Some(resolve) = &self.default_resolver {
            return resolve();
        }
        Ok(self.default.clone())
    }
    pub(crate) fn with_identity_resolvers(
        mut self,
        id: IdResolver,
        attributes: AttributeResolver,
    ) -> Self {
        self.id_resolver = Some(id);
        self.attributes_resolver = Some(attributes);
        self
    }
    pub async fn resolve_id(
        &self,
        id: openrust::auth::AuthIdentifier,
    ) -> Result<(Option<Subject>, serde_json::Value)> {
        if self.causer_override.lock().unwrap().is_some() || self.resolver_override.is_some() {
            let subject = self.resolve(None)?;
            let attributes = subject.as_ref().map_or_else(
                || Ok(serde_json::json!({})),
                |subject| self.attributes(subject),
            )?;
            return Ok((subject, attributes));
        }
        let (subject, attributes) = self
            .id_resolver
            .as_ref()
            .ok_or(crate::Error::InvalidConfiguration)?(id)
        .await?;
        Ok((Some(subject), attributes))
    }
    pub(crate) fn attributes(&self, subject: &Subject) -> Result<serde_json::Value> {
        self.attributes_resolver
            .as_ref()
            .map_or_else(|| Ok(serde_json::json!({})), |resolve| resolve(subject))
    }
    pub(crate) fn with_default_resolver(
        mut self,
        callback: impl Fn() -> Result<Option<Subject>> + Send + Sync + 'static,
    ) -> Self {
        self.default_resolver = Some(Arc::new(callback));
        self
    }
    pub fn resolve_using(
        mut self,
        callback: impl Fn(Option<Subject>) -> Result<Option<Subject>> + Send + Sync + 'static,
    ) -> Self {
        self.resolver_override = Some(Arc::new(callback));
        self
    }
    pub fn set_causer(&self, causer: Option<Subject>) -> &Self {
        *self.causer_override.lock().unwrap() = causer;
        self
    }
    pub async fn with_causer<F: Future>(
        &self,
        causer: Option<Subject>,
        callback: impl FnOnce() -> F,
    ) -> F::Output {
        let previous = std::mem::replace(&mut *self.causer_override.lock().unwrap(), causer);
        let _restore = RestoreCauser(self.clone(), previous);
        callback().await
    }
}
struct RestoreCauser(CauserResolver, Option<Subject>);
impl Drop for RestoreCauser {
    fn drop(&mut self) {
        *self.0.causer_override.lock().unwrap() = self.1.take();
    }
}

pub type DescriptionForEvent = Arc<dyn Fn(&str) -> String + Send + Sync>;
#[derive(Clone)]
pub struct LogOptions {
    pub log_name: Option<String>,
    pub log_empty_changes: bool,
    pub log_fillable: bool,
    pub log_only_dirty: bool,
    pub log_unguarded: bool,
    pub log_attributes: Vec<String>,
    pub log_except_attributes: Vec<String>,
    pub dont_log_if_attributes_changed_only: Vec<String>,
    pub attribute_raw_values: Vec<String>,
    pub description_for_event: Option<DescriptionForEvent>,
}
impl Default for LogOptions {
    fn default() -> Self {
        Self {
            log_name: None,
            log_empty_changes: true,
            log_fillable: false,
            log_only_dirty: false,
            log_unguarded: false,
            log_attributes: vec![],
            log_except_attributes: vec![],
            dont_log_if_attributes_changed_only: vec![],
            attribute_raw_values: vec![],
            description_for_event: None,
        }
    }
}
impl LogOptions {
    pub fn defaults() -> Self {
        Self::default()
    }
    pub fn log_all(self) -> Self {
        self.log_only(&["*"])
    }
    pub fn log_unguarded(mut self) -> Self {
        self.log_unguarded = true;
        self
    }
    pub fn log_fillable(mut self) -> Self {
        self.log_fillable = true;
        self
    }
    pub fn dont_log_fillable(mut self) -> Self {
        self.log_fillable = false;
        self
    }
    pub fn log_only_dirty(mut self) -> Self {
        self.log_only_dirty = true;
        self
    }
    pub fn log_only(mut self, attributes: &[&str]) -> Self {
        self.log_attributes = strings(attributes);
        self
    }
    pub fn log_except(mut self, attributes: &[&str]) -> Self {
        self.log_except_attributes = strings(attributes);
        self
    }
    pub fn dont_log_if_attributes_changed_only(mut self, attributes: &[&str]) -> Self {
        self.dont_log_if_attributes_changed_only = strings(attributes);
        self
    }
    pub fn dont_log_empty_changes(mut self) -> Self {
        self.log_empty_changes = false;
        self
    }
    pub fn log_empty_changes(mut self) -> Self {
        self.log_empty_changes = true;
        self
    }
    pub fn use_log_name(mut self, name: impl IntoLogName) -> Self {
        self.log_name = name.into_log_name();
        self
    }
    pub fn set_description_for_event(
        mut self,
        callback: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> Self {
        self.description_for_event = Some(Arc::new(callback));
        self
    }
    pub fn use_attribute_raw_values(mut self, attributes: &[&str]) -> Self {
        self.attribute_raw_values = strings(attributes);
        self
    }
}
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
