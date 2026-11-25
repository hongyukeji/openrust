use crate::{
    EntryType, EntryUpdate, IncomingEntry, IncomingExceptionEntry, Result,
    contracts::EntriesRepository,
};
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;

type EntryFilter = Arc<dyn Fn(&IncomingEntry) -> bool + Send + Sync>;
type BatchFilter = Arc<dyn Fn(&[IncomingEntry]) -> bool + Send + Sync>;
type TagCallback = Arc<dyn Fn(&IncomingEntry) -> Vec<String> + Send + Sync>;
type RecordingHook = Arc<dyn Fn(&Telescope, &IncomingEntry) + Send + Sync>;
type StoringHook = Arc<dyn Fn(&Telescope, &[IncomingEntry]) + Send + Sync>;
#[derive(Default)]
struct Callbacks {
    filters: Vec<EntryFilter>,
    batches: Vec<BatchFilter>,
    tags: Vec<TagCallback>,
    after_recording: Option<RecordingHook>,
    after_storing: Vec<StoringHook>,
}
#[derive(Default)]
struct Pending {
    entries: Vec<IncomingEntry>,
    updates: Vec<EntryUpdate>,
}
struct Hidden {
    request_headers: Vec<String>,
    request_parameters: Vec<String>,
    response_parameters: Vec<String>,
}
pub(crate) struct Options {
    callbacks: Arc<RwLock<Callbacks>>,
    hidden: Arc<RwLock<Hidden>>,
    authorization: Arc<RwLock<Option<crate::http::Authorize>>>,
    ignore_framework_events: Arc<AtomicBool>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            callbacks: Default::default(),
            hidden: Default::default(),
            authorization: Default::default(),
            ignore_framework_events: Arc::new(AtomicBool::new(true)),
        }
    }
}
impl Default for Hidden {
    fn default() -> Self {
        Self {
            request_headers: vec!["authorization".into(), "php-auth-pw".into()],
            request_parameters: vec!["password".into(), "password_confirmation".into()],
            response_parameters: Vec::new(),
        }
    }
}

/// One HTTP request, console invocation or queue job's Telescope recording state.
pub struct Telescope {
    /// Shared application setting matching Telescope::$ignoreFrameworkEvents.
    pub ignore_framework_events: Arc<AtomicBool>,
    pub(crate) repository: Arc<dyn EntriesRepository>,
    enabled: AtomicBool,
    started: AtomicBool,
    pub(crate) automatic_recording: AtomicBool,
    suppressed: AtomicUsize,
    pending: Mutex<Pending>,
    callbacks: Arc<RwLock<Callbacks>>,
    storing: tokio::sync::Mutex<()>,
    hidden: Arc<RwLock<Hidden>>,
    authorization: Arc<RwLock<Option<crate::http::Authorize>>>,
}
struct WithoutRecording<'a>(&'a AtomicUsize);
impl Drop for WithoutRecording<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Telescope {
    pub fn new(repository: Arc<dyn EntriesRepository>, enabled: bool) -> Self {
        Self::with_options(repository, enabled, Arc::new(Options::default()))
    }
    pub(crate) fn with_options(
        repository: Arc<dyn EntriesRepository>,
        enabled: bool,
        options: Arc<Options>,
    ) -> Self {
        Self {
            repository,
            ignore_framework_events: options.ignore_framework_events.clone(),
            enabled: AtomicBool::new(enabled),
            started: AtomicBool::new(false),
            automatic_recording: AtomicBool::new(false),
            suppressed: AtomicUsize::new(0),
            pending: Mutex::default(),
            callbacks: options.callbacks.clone(),
            storing: tokio::sync::Mutex::new(()),
            hidden: options.hidden.clone(),
            authorization: options.authorization.clone(),
        }
    }
    pub fn auth(&self, callback: crate::http::Authorize) {
        *self
            .authorization
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(callback);
    }
    pub async fn check(
        &self,
        request: &mut openrust::http::Request,
    ) -> std::result::Result<bool, openrust::http::HttpError> {
        let callback = *self
            .authorization
            .read()
            .unwrap_or_else(|error| error.into_inner());
        match callback {
            Some(callback) => callback(request).await,
            None => Ok(openrust::support::facades::Config::get("app.env")? == Some("local".into())),
        }
    }
    pub fn hide_request_headers(&self, headers: &[&str]) {
        self.hidden
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .request_headers
            .extend(headers.iter().map(|s| s.to_ascii_lowercase()));
    }
    pub fn hide_request_parameters(&self, parameters: &[&str]) {
        self.hidden
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .request_parameters
            .extend(parameters.iter().map(|s| (*s).into()));
    }
    pub fn hide_response_parameters(&self, parameters: &[&str]) {
        self.hidden
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .response_parameters
            .extend(parameters.iter().map(|s| (*s).into()));
    }
    pub(crate) fn headers(&self, headers: &openrust::http::HeaderMap) -> serde_json::Value {
        let hidden = self
            .hidden
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .request_headers
            .clone();
        let mut result = serde_json::Map::new();
        for key in headers.keys() {
            let key = key.as_str();
            let value = if hidden.iter().any(|name| name == key) {
                "********".into()
            } else {
                headers
                    .get_all(key)
                    .iter()
                    .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            result.insert(key.into(), value.into());
        }
        result.into()
    }
    pub(crate) fn request_parameters(&self, value: serde_json::Value) -> serde_json::Value {
        redact(
            value,
            &self
                .hidden
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .request_parameters,
        )
    }
    pub(crate) fn response_parameters(&self, value: serde_json::Value) -> serde_json::Value {
        redact(
            value,
            &self
                .hidden
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .response_parameters,
        )
    }
    pub(crate) fn is_storing() -> bool {
        STORING.try_with(|_| ()).is_ok()
    }
    pub fn is_recording(&self) -> bool {
        !Self::is_storing()
            && self.enabled.load(Ordering::Acquire)
            && self.suppressed.load(Ordering::Acquire) == 0
    }
    pub(crate) fn has_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }
    pub fn stop_recording(&self) {
        self.enabled.store(false, Ordering::Release);
    }
    pub async fn start_recording(&self, load_monitored_tags: bool) {
        self.started.store(true, Ordering::Release);
        let _guard = self.suppress();
        if load_monitored_tags {
            self.repository.load_monitored_tags().await;
        }
        let paused = matches!(
            openrust::cache::Cache::get("telescope:pause-recording").await,
            Ok(Some(openrust::cache::Value::Json(serde_json::Value::Bool(
                true
            ))))
        );
        self.enabled.store(!paused, Ordering::Release);
    }
    pub(crate) async fn initialize(&self) {
        if !self.started.swap(true, Ordering::AcqRel) && self.enabled.load(Ordering::Acquire) {
            self.start_recording(true).await;
            if !self.enabled.load(Ordering::Acquire) {
                *self
                    .pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Pending::default();
            }
        }
    }
    fn suppress(&self) -> WithoutRecording<'_> {
        self.suppressed.fetch_add(1, Ordering::AcqRel);
        WithoutRecording(&self.suppressed)
    }
    pub async fn without_recording<T>(&self, future: impl std::future::Future<Output = T>) -> T {
        let _guard = self.suppress();
        future.await
    }
    pub fn filter(
        &self,
        callback: impl Fn(&IncomingEntry) -> bool + Send + Sync + 'static,
    ) -> &Self {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .filters
            .push(Arc::new(callback));
        self
    }
    pub fn filter_batch(
        &self,
        callback: impl Fn(&[IncomingEntry]) -> bool + Send + Sync + 'static,
    ) -> &Self {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .batches
            .push(Arc::new(callback));
        self
    }
    pub fn tag(
        &self,
        callback: impl Fn(&IncomingEntry) -> Vec<String> + Send + Sync + 'static,
    ) -> &Self {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .tags
            .push(Arc::new(callback));
        self
    }
    pub fn after_recording(
        &self,
        callback: impl Fn(&Self, &IncomingEntry) + Send + Sync + 'static,
    ) -> &Self {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .after_recording = Some(Arc::new(callback));
        self
    }
    pub fn after_storing(
        &self,
        callback: impl Fn(&Self, &[IncomingEntry]) + Send + Sync + 'static,
    ) -> &Self {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .after_storing
            .push(Arc::new(callback));
        self
    }
    pub fn record(&self, kind: EntryType, mut entry: IncomingEntry) {
        if !self.is_recording() {
            return;
        }
        entry.r#type = Some(kind);
        if let Ok(auth) = openrust::auth::Auth::manager()
            && let Some(user) = auth.resolved_user(None)
            && let (Ok(id), Ok(attributes)) = (
                serde_json::to_value(user.get_auth_identifier()),
                auth.user_attributes(user.as_ref()),
            )
        {
            entry = entry.user(
                id,
                attributes.get("name").cloned().unwrap_or_default(),
                attributes.get("email").cloned().unwrap_or_default(),
            );
        }
        let (tags, filters, hook) = {
            let callbacks = self.callbacks.read().unwrap_or_else(|e| e.into_inner());
            (
                callbacks.tags.clone(),
                callbacks.filters.clone(),
                callbacks.after_recording.clone(),
            )
        };
        for callback in tags {
            let extra = callback(&entry);
            entry = entry.tags(extra);
        }
        entry.monitored = self.repository.has_monitored_tag(&entry.tags);
        let _guard = self.suppress();
        if filters.iter().all(|filter| filter(&entry)) {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entries
                .push(entry.clone());
        }
        if let Some(hook) = hook {
            hook(self, &entry);
        }
    }
    pub fn record_update(&self, update: EntryUpdate) {
        if self.enabled.load(Ordering::Acquire) {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .updates
                .push(update);
        }
    }
    pub fn record_dump(&self, entry: crate::IncomingDumpEntry) {
        self.record(EntryType::Dump, entry.into());
    }
    pub(crate) fn record_hydration(&self, model: &str) -> crate::Result<()> {
        {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(entry) = pending.entries.iter_mut().find(|entry| {
                entry.r#type == Some(EntryType::Model)
                    && entry.content.get("action") == Some(&serde_json::json!("retrieved"))
                    && entry
                        .content
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        == Some(model)
            }) {
                let count = entry
                    .content
                    .get("count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                entry
                    .content
                    .insert("count".into(), count.saturating_add(1).into());
                return Ok(());
            }
        }
        self.record_model_event(
            IncomingEntry::make(
                serde_json::json!({"action":"retrieved","model":model,"count":1})
                    .as_object()
                    .unwrap()
                    .clone(),
            )?
            .tags([model]),
        );
        Ok(())
    }
    pub fn record_exception(&self, entry: IncomingExceptionEntry) {
        self.record(EntryType::Exception, entry.into());
    }
    pub fn flush_entries(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .clear();
    }
    /// Stores this execution's collected entries. Returns updates whose entry is not yet available.
    pub async fn store(&self) -> Result<Vec<EntryUpdate>> {
        let _serial = self.storing.lock().await;
        let mut pending =
            std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()));
        if pending.entries.is_empty() && pending.updates.is_empty() {
            return Ok(Vec::new());
        }
        let _guard = self.suppress();
        let (filters, hooks) = {
            let callbacks = self.callbacks.read().unwrap_or_else(|e| e.into_inner());
            (callbacks.batches.clone(), callbacks.after_storing.clone())
        };
        if !filters.iter().all(|filter| filter(&pending.entries)) {
            pending.entries.clear();
        }
        let batch = Uuid::now_v7();
        for entry in &mut pending.entries {
            entry.batch_id = Some(batch);
        }
        let entry_points = pending
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry.r#type,
                    Some(EntryType::Request | EntryType::Job | EntryType::Command)
                )
            })
            .take(1)
            .cloned()
            .collect::<Vec<_>>();
        for entry in &mut pending.entries {
            if entry.is_dump() {
                let mut dump = crate::IncomingDumpEntry(entry.clone());
                dump.assign_entry_point_from_batch(&entry_points);
                *entry = dump.into();
            }
        }
        for update in &mut pending.updates {
            update
                .changes
                .insert("updated_batch_id".into(), batch.to_string().into());
        }
        // 数据库事件可能在另一个容器执行作用域中解析出 Telescope。
        let result = STORING
            .scope((), async {
                self.repository.store(&pending.entries).await?;
                let failed = self.repository.update(&pending.updates).await?;
                for hook in hooks {
                    hook(self, &pending.entries);
                }
                Ok(failed)
            })
            .await;
        self.repository.terminate();
        result
    }
}
tokio::task_local! { static STORING: (); }
macro_rules! recorders {
    ($($method:ident => $kind:ident),* $(,)?) => { impl Telescope { $(pub fn $method(&self, entry: IncomingEntry) { self.record(EntryType::$kind, entry); })* } };
}
recorders!(record_batch => Batch, record_cache => Cache, record_command => Command, record_event => Event, record_gate => Gate, record_job => Job, record_log => Log, record_mail => Mail, record_notification => Notification, record_query => Query, record_model_event => Model, record_redis => Redis, record_request => Request, record_scheduled_command => ScheduledTask, record_view => View, record_client_request => ClientRequest);
fn redact(mut value: serde_json::Value, hidden: &[String]) -> serde_json::Value {
    for path in hidden {
        if path.contains('.')
            && let Some(literal) = value.get_mut(path)
        {
            mask_parameter(literal);
        }
        if let Some(nested) = path
            .split('.')
            .try_fold(&mut value, |value, key| match value {
                serde_json::Value::Object(values) => values.get_mut(key),
                serde_json::Value::Array(values) => key
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| values.get_mut(index)),
                _ => None,
            })
        {
            mask_parameter(nested);
        }
    }
    value
}

fn mask_parameter(value: &mut serde_json::Value) {
    let present = match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Number(value) => value.as_f64() != Some(0.0),
        serde_json::Value::String(value) => !value.is_empty() && value != "0",
        serde_json::Value::Array(value) => !value.is_empty(),
        serde_json::Value::Object(value) => !value.is_empty(),
    };
    if present {
        *value = "********".into();
    }
}
