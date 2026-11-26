use crate::{
    ByteValue, Entry, Error, Item, PulseFuture, Result, Value, ingests::Ingest, storage::Storage,
};
use std::{
    future::Future,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
pub(crate) type Filter = Arc<dyn Fn(&Item) -> Result<bool> + Send + Sync>;
type Lazy = Box<dyn for<'a> FnOnce(&'a Pulse) -> PulseFuture<'a, ()> + Send>;
pub(crate) type Handler = Arc<dyn Fn(&Error) + Send + Sync>;
/// Lifecycle-scoped metric buffer. Shared settings belong to its application registry.
pub struct Options {
    pub(crate) recorders: RwLock<Vec<Arc<dyn crate::recorders::Recorder>>>,
    css: RwLock<Vec<crate::Css>>,
    legacy_users: RwLock<Option<Arc<crate::LegacyUserResolver>>>,
    pub(crate) registers_routes: AtomicBool,
    filters: RwLock<Vec<Filter>>,
    handler: RwLock<Option<Handler>>,
    pub(crate) user_fields: Arc<RwLock<Option<Arc<crate::users::FieldResolver>>>>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            recorders: RwLock::default(),
            css: RwLock::default(),
            legacy_users: RwLock::default(),
            registers_routes: AtomicBool::new(true),
            filters: RwLock::default(),
            handler: RwLock::default(),
            user_fields: Arc::default(),
        }
    }
}
pub struct Pulse {
    pub storage: Arc<dyn Storage>,
    users: Arc<dyn crate::ResolvesUsers>,
    pub(crate) automatic_recording: AtomicBool,
    ingest: Arc<dyn Ingest>,
    entries: Mutex<Vec<Item>>,
    remembered_user_id: RwLock<Option<String>>,
    lazy: Mutex<Vec<Lazy>>,
    should_record: AtomicBool,
    ignoring: AtomicUsize,
    evaluating_buffer: AtomicBool,
    buffer: usize,
    options: Arc<Options>,
    lottery: openrust::support::Lottery,
}
tokio::task_local! {
    static IGNORED: Vec<Arc<IgnoreScope>>;
}
struct IgnoreScope {
    options: Arc<Options>,
    owner: usize,
    recording: AtomicBool,
}
struct Ignore<'a>(&'a AtomicUsize);
impl Drop for Ignore<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
impl Pulse {
    pub fn new(storage: Arc<dyn Storage>, ingest: Arc<dyn Ingest>) -> Self {
        Self {
            storage,
            ingest,
            users: Arc::new(crate::Users::default()),
            automatic_recording: AtomicBool::new(false),
            entries: Mutex::default(),
            remembered_user_id: RwLock::default(),
            lazy: Mutex::default(),
            should_record: AtomicBool::new(true),
            ignoring: AtomicUsize::new(0),
            evaluating_buffer: AtomicBool::new(false),
            buffer: 5000,
            options: Arc::default(),
            lottery: openrust::support::Lottery::odds(1, 1000).expect("valid default odds"),
        }
    }
    pub fn register(&self, recorders: Vec<Arc<dyn crate::recorders::Recorder>>) -> Result<&Self> {
        use openrust::support::facades::{Event, Facade};
        let dispatcher = Event::get_facade_root()?;
        for recorder in recorders {
            crate::recorders::register_recorder(&dispatcher, recorder, self.options.clone())?;
        }
        Ok(self)
    }
    pub fn recorders(&self) -> Vec<Arc<dyn crate::recorders::Recorder>> {
        self.options
            .recorders
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    /// Adds trusted application CSS paths or HTML, retaining the first occurrence.
    pub fn with_css(&self, css: impl IntoIterator<Item = crate::Css>) -> &Self {
        let mut registered = self.options.css.write().unwrap_or_else(|e| e.into_inner());
        for css in css {
            if !registered.contains(&css) {
                registered.push(css);
            }
        }
        self
    }
    pub fn css(&self) -> Result<String> {
        let mut html = format!(
            "<style>{}</style>\n",
            include_str!("../resources/css/pulse.css")
        );
        for css in self
            .options
            .css
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            match css {
                crate::Css::Path(path) => {
                    let content = std::fs::read_to_string(path).map_err(crate::Error::Io)?;
                    html.push_str(&format!("<style>{content}</style>\n"));
                }
                crate::Css::Html(content) => {
                    html.push_str(content);
                    if !content.ends_with('\n') {
                        html.push('\n');
                    }
                }
            }
        }
        Ok(html)
    }
    pub fn js(&self) -> String {
        format!(
            "<script>{}</script>\n",
            include_str!("../resources/js/pulse.js")
        )
    }
    pub fn with_user_resolver(mut self, resolver: Arc<dyn crate::ResolvesUsers>) -> Self {
        self.users = resolver;
        self
    }
    pub async fn resolve_users(&self, keys: &[String]) -> Result<Arc<dyn crate::ResolvesUsers>> {
        let users = self.user_resolver();
        self.ignore(users.load(keys)).await?;
        Ok(users)
    }
    pub fn user(
        &self,
        resolver: impl Fn(&dyn openrust::contracts::auth::Authenticatable) -> crate::User
        + Send
        + Sync
        + 'static,
    ) -> Result<&Self> {
        self.user_resolver()
            .set_field_resolver(Arc::new(resolver))?;
        Ok(self)
    }
    fn user_resolver(&self) -> Arc<dyn crate::ResolvesUsers> {
        self.options
            .legacy_users
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .map(|resolver| {
                Arc::new(crate::LegacyUsers::new(resolver)) as Arc<dyn crate::ResolvesUsers>
            })
            .unwrap_or_else(|| self.users.clone())
    }
    /// Prefer user or a native ResolvesUsers binding for new applications.
    pub fn users(
        &self,
        resolver: impl Fn(&[String]) -> Result<std::collections::BTreeMap<String, crate::User>>
        + Send
        + Sync
        + 'static,
    ) -> &Self {
        *self
            .options
            .legacy_users
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(resolver));
        self
    }
    pub fn with_options(mut self, options: Arc<Options>) -> Self {
        self.options = options;
        self
    }

    pub(crate) fn has_options(&self, options: &Arc<Options>) -> bool {
        Arc::ptr_eq(&self.options, options)
    }
    pub fn with_trim_lottery(mut self, lottery: openrust::support::Lottery) -> Self {
        self.lottery = lottery;
        self
    }
    pub fn with_buffer(mut self, buffer: usize) -> Self {
        self.buffer = buffer;
        self
    }
    pub fn default_vendor_cache_keys() -> Vec<String> {
        vec![
            r"/(^laravel_vapor_job_attemp(t?)s:)/",
            r"/^.+@.+\|(?:(?:\d+\.\d+\.\d+\.\d+)|[0-9a-fA-F:]+)(?::timer)?$/",
            r"/^[a-zA-Z0-9]{40}$/",
            r"/^illuminate:/",
            r"/^openrust:/",
            r"/^laravel:pulse:/",
            r"/^laravel:reverb:/",
            r"/^nova/",
            r"/^telescope:/",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    pub fn ignore_routes(&self) {
        self.options
            .registers_routes
            .store(false, Ordering::Relaxed);
    }
    pub fn registers_routes(&self) -> bool {
        self.options.registers_routes.load(Ordering::Relaxed)
    }
    pub fn start_recording(&self) {
        self.set_recording(true);
    }
    pub fn stop_recording(&self) {
        self.set_recording(false);
    }
    fn set_recording(&self, enabled: bool) {
        let owner = std::ptr::from_ref(self).addr();
        let changed = IGNORED
            .try_with(|ignored| {
                let scope = ignored.iter().rev().find(|scope| scope.owner == owner)?;
                scope.recording.store(enabled, Ordering::Relaxed);
                Some(())
            })
            .ok()
            .flatten();
        if changed.is_none() {
            self.should_record.store(enabled, Ordering::Relaxed);
        }
    }
    fn ignore_scope(options: &Arc<Options>) -> Option<Arc<IgnoreScope>> {
        IGNORED
            .try_with(|ignored| {
                ignored
                    .iter()
                    .rev()
                    .find(|scope| Arc::ptr_eq(&scope.options, options))
                    .cloned()
            })
            .ok()
            .flatten()
    }
    pub(crate) fn is_ignoring(options: &Arc<Options>) -> bool {
        Self::ignore_scope(options).is_some_and(|scope| !scope.recording.load(Ordering::Relaxed))
    }
    pub fn is_recording(&self) -> bool {
        if let Some(scope) = Self::ignore_scope(&self.options) {
            return scope.owner == std::ptr::from_ref(self).addr()
                && scope.recording.load(Ordering::Relaxed);
        }
        self.should_record.load(Ordering::Relaxed) && self.ignoring.load(Ordering::Relaxed) == 0
    }
    pub async fn ignore<T>(&self, callback: impl Future<Output = T>) -> T {
        self.ignoring.fetch_add(1, Ordering::Relaxed);
        let _guard = Ignore(&self.ignoring);
        // 延迟回调可能解析新容器作用域。只有当前实例可在回调内临时恢复采集。
        let mut ignored = IGNORED.try_with(Clone::clone).unwrap_or_default();
        ignored.push(Arc::new(IgnoreScope {
            options: self.options.clone(),
            owner: std::ptr::from_ref(self).addr(),
            recording: AtomicBool::new(false),
        }));
        IGNORED.scope(ignored, callback).await
    }
    pub fn handle_exceptions_using(&self, callback: impl Fn(&Error) + Send + Sync + 'static) {
        *self
            .options
            .handler
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(callback));
    }
    pub(crate) fn report_error(&self, error: &Error) {
        let handler = self
            .options
            .handler
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(handler) = handler {
            handler(error);
        }
    }
    pub async fn rescue<T>(&self, callback: impl Future<Output = Result<T>>) -> Option<T> {
        match callback.await {
            Ok(value) => Some(value),
            Err(error) => {
                self.report_error(&error);
                None
            }
        }
    }
    pub fn filter(&self, filter: impl Fn(&Item) -> Result<bool> + Send + Sync + 'static) {
        self.options
            .filters
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::new(filter));
    }
    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }
    pub async fn record(
        &self,
        r#type: impl Into<String>,
        key: impl Into<String>,
        value: Option<i64>,
        timestamp: Option<i64>,
    ) -> Entry {
        self.buffer_entry(Entry::new(
            timestamp.unwrap_or_else(Self::now),
            r#type,
            key,
            value,
        ))
        .await
    }

    pub(crate) async fn record_key_bytes(
        &self,
        r#type: &str,
        key: Vec<u8>,
        value: Option<i64>,
        timestamp: i64,
    ) -> Entry {
        self.buffer_entry(Entry::from_key_bytes(timestamp, r#type, key, value))
            .await
    }

    async fn buffer_entry(&self, entry: Entry) -> Entry {
        if self.is_recording() {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(entry.clone().into());
            self.ingest_when_over_buffer_size().await;
        }
        entry
    }
    pub async fn set(
        &self,
        r#type: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
        timestamp: Option<i64>,
    ) -> Value {
        self.buffer_value(Value::new(
            timestamp.unwrap_or_else(Self::now),
            r#type,
            key,
            value,
        ))
        .await
    }
    pub async fn set_bytes(
        &self,
        r#type: impl Into<String>,
        key: Vec<u8>,
        value: impl Into<String>,
        timestamp: Option<i64>,
    ) -> ByteValue {
        self.buffer_value(ByteValue::from_key_bytes(
            timestamp.unwrap_or_else(Self::now),
            r#type,
            key,
            value,
        ))
        .await
    }
    async fn buffer_value<K: Clone + Send>(&self, value: Value<K>) -> Value<K>
    where
        Item: From<Value<K>>,
    {
        if self.is_recording() {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(value.clone().into());
            self.ingest_when_over_buffer_size().await;
        }
        value
    }
    pub async fn lazy(
        &self,
        callback: impl for<'a> FnOnce(&'a Pulse) -> PulseFuture<'a, ()> + Send + 'static,
    ) {
        if self.is_recording() {
            self.lazy
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Box::new(callback));
            self.ingest_when_over_buffer_size().await;
        }
    }
    pub(crate) fn defer(
        &self,
        callback: impl for<'a> FnOnce(&'a Pulse) -> PulseFuture<'a, ()> + Send + 'static,
    ) {
        if self.is_recording() {
            self.lazy
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Box::new(callback));
        }
    }
    pub fn remember_user(&self, user: &dyn openrust::contracts::auth::Authenticatable) -> &Self {
        *self
            .remembered_user_id
            .write()
            .unwrap_or_else(|e| e.into_inner()) = self.user_resolver().key(user);
        self
    }
    pub fn authenticated_user_id_resolver(
        &self,
    ) -> impl Fn(&Pulse) -> Option<String> + Send + Sync + use<> {
        use openrust::support::facades::Auth;
        let key = Auth::manager()
            .ok()
            .and_then(|auth| auth.resolved_user(None))
            .and_then(|user| self.user_resolver().key(user.as_ref()));
        move |pulse| {
            key.clone()
                .or_else(|| pulse.resolve_authenticated_user_id())
        }
    }
    pub fn resolve_authenticated_user_id(&self) -> Option<String> {
        use openrust::support::facades::Auth;
        Auth::manager()
            .ok()
            .and_then(|a| a.resolved_user(None))
            .and_then(|u| self.user_resolver().key(u.as_ref()))
            .or_else(|| {
                self.remembered_user_id
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
            })
    }
    pub async fn report(&self, class: impl Into<String>, location: Option<String>) -> &Self {
        self.dispatch_exception(crate::events::ExceptionReported {
            class: class.into(),
            location,
        })
        .await
    }

    /// 将原生错误报告给 Pulse 的监听器和记录器。
    pub async fn report_exception(
        &self,
        exception: impl std::error::Error + Send + Sync + 'static,
    ) -> &Self {
        self.dispatch_exception(crate::events::NativeExceptionReported {
            exception: openrust::contracts::debug::SharedException::new(exception),
        })
        .await
    }

    async fn dispatch_exception(&self, event: impl openrust::contracts::events::Event) -> &Self {
        self.rescue(async {
            use openrust::{
                contracts::events::Dispatcher as _,
                support::facades::{Event, Facade},
            };
            Event::get_facade_root()?
                .dispatch(Arc::new(event), &mut openrust::events::EventContext::new())
                .await?;
            Ok(())
        })
        .await;
        self
    }
    pub fn wants_ingesting(&self) -> bool {
        !self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
            || !self
                .lazy
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
    }
    pub fn flush(&self) {
        *self
            .remembered_user_id
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.lazy.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    async fn resolve_lazy_entries(&self) {
        let lazy = std::mem::take(&mut *self.lazy.lock().unwrap_or_else(|e| e.into_inner()));
        for callback in lazy {
            if let Err(e) = callback(self).await {
                self.report_error(&e);
                break;
            }
        }
        self.lazy.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    fn ingest_when_over_buffer_size(
        &self,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if self.evaluating_buffer.swap(true, Ordering::Relaxed) {
                return;
            }
            struct Reset<'a>(&'a AtomicBool);
            impl Drop for Reset<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Relaxed);
                }
            }
            let _guard = Reset(&self.evaluating_buffer);
            let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner()).len();
            let lazy = self.lazy.lock().unwrap_or_else(|e| e.into_inner()).len();
            if entries.saturating_add(lazy) > self.buffer {
                self.resolve_lazy_entries().await;
            }
            if self.entries.lock().unwrap_or_else(|e| e.into_inner()).len() > self.buffer {
                self.ingest().await;
            }
        })
    }
    pub fn ingest(&self) -> std::pin::Pin<Box<dyn Future<Output = usize> + Send + '_>> {
        Box::pin(async move {
            self.resolve_lazy_entries().await;
            self.ignore(async {
                let entries =
                    std::mem::take(&mut *self.entries.lock().unwrap_or_else(|e| e.into_inner()));
                let filters = self
                    .options
                    .filters
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let mut accepted = Vec::new();
                for entry in entries {
                    let mut include = true;
                    for filter in &filters {
                        match filter(&entry) {
                            Ok(true) => {}
                            Ok(false) => {
                                include = false;
                                break;
                            }
                            Err(e) => {
                                self.report_error(&e);
                                self.flush();
                                return 0;
                            }
                        }
                    }
                    if include {
                        accepted.push(entry);
                    }
                }
                if accepted.is_empty() {
                    self.flush();
                    return 0;
                }
                let count = match self.ingest.ingest(&accepted).await {
                    Ok(()) => accepted.len(),
                    Err(e) => {
                        self.report_error(&e);
                        0
                    }
                };
                if self.lottery.choose()
                    && let Err(e) = self.ingest.trim().await
                {
                    self.report_error(&e);
                }
                self.flush();
                count
            })
            .await
        })
    }
    pub async fn values(
        &self,
        r#type: &str,
        keys: Option<&[String]>,
    ) -> Result<Vec<openrust::database::Row>> {
        self.ignore(self.storage.values(r#type, keys)).await
    }
    pub async fn values_bytes(
        &self,
        r#type: &str,
        keys: Option<&[Vec<u8>]>,
    ) -> Result<Vec<openrust::database::Row>> {
        self.ignore(self.storage.values_bytes(r#type, keys)).await
    }
    pub async fn graph(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> Result<crate::storage::Graph> {
        self.ignore(self.storage.graph(types, aggregate, interval))
            .await
    }
    pub async fn graph_bytes(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> Result<crate::storage::ByteGraph> {
        self.ignore(self.storage.graph_bytes(types, aggregate, interval))
            .await
    }
    pub async fn aggregate(
        &self,
        r#type: &str,
        aggregates: &[String],
        interval: i64,
        order_by: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> Result<Vec<openrust::database::Row>> {
        self.ignore(
            self.storage
                .aggregate(r#type, aggregates, interval, order_by, direction, limit),
        )
        .await
    }
    pub async fn aggregate_types(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
        order_by: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> Result<Vec<openrust::database::Row>> {
        self.ignore(
            self.storage
                .aggregate_types(types, aggregate, interval, order_by, direction, limit),
        )
        .await
    }
    pub async fn aggregate_total(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> Result<indexmap::IndexMap<String, Option<f64>>> {
        self.ignore(self.storage.aggregate_total(types, aggregate, interval))
            .await
    }
    pub async fn trim(&self) -> Result<()> {
        self.ignore(self.storage.trim()).await
    }
    pub async fn purge(&self, types: Option<&[String]>) -> Result<()> {
        self.ignore(self.storage.purge(types)).await
    }
    pub async fn digest(&self) -> Result<usize> {
        self.ignore(self.ingest.digest(self.storage.as_ref())).await
    }
}
