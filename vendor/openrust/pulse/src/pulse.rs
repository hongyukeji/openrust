use crate::{Entry, Error, Item, PulseFuture, Result, Value, ingests::Ingest, storage::Storage};
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
    legacyUsers: RwLock<Option<Arc<crate::LegacyUserResolver>>>,
    pub(crate) registersRoutes: AtomicBool,
    filters: RwLock<Vec<Filter>>,
    handler: RwLock<Option<Handler>>,
    pub(crate) userFields: Arc<RwLock<Option<Arc<crate::users::FieldResolver>>>>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            recorders: RwLock::default(),
            css: RwLock::default(),
            legacyUsers: RwLock::default(),
            registersRoutes: AtomicBool::new(true),
            filters: RwLock::default(),
            handler: RwLock::default(),
            userFields: Arc::default(),
        }
    }
}
pub struct Pulse {
    pub storage: Arc<dyn Storage>,
    users: Arc<dyn crate::ResolvesUsers>,
    pub(crate) automaticRecording: AtomicBool,
    ingest: Arc<dyn Ingest>,
    entries: Mutex<Vec<Item>>,
    rememberedUserId: RwLock<Option<String>>,
    lazy: Mutex<Vec<Lazy>>,
    shouldRecord: AtomicBool,
    ignoring: AtomicUsize,
    evaluatingBuffer: AtomicBool,
    buffer: usize,
    options: Arc<Options>,
    lottery: openrust::support::Lottery,
}
tokio::task_local! {
    static IGNORED: Vec<Arc<Options>>;
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
            automaticRecording: AtomicBool::new(false),
            entries: Mutex::default(),
            rememberedUserId: RwLock::default(),
            lazy: Mutex::default(),
            shouldRecord: AtomicBool::new(true),
            ignoring: AtomicUsize::new(0),
            evaluatingBuffer: AtomicBool::new(false),
            buffer: 5000,
            options: Arc::default(),
            lottery: openrust::support::Lottery::odds(1, 1000).expect("valid default odds"),
        }
    }
    pub fn register(&self, recorders: Vec<Arc<dyn crate::recorders::Recorder>>) -> Result<&Self> {
        use openrust::support::facades::{Event, Facade};
        let dispatcher = Event::get_facade_root()?;
        for recorder in recorders {
            crate::recorders::registerRecorder(&dispatcher, recorder, self.options.clone())?;
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
    pub fn withCss(&self, css: impl IntoIterator<Item = crate::Css>) -> &Self {
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
    pub fn withUserResolver(mut self, resolver: Arc<dyn crate::ResolvesUsers>) -> Self {
        self.users = resolver;
        self
    }
    pub async fn resolveUsers(&self, keys: &[String]) -> Result<Arc<dyn crate::ResolvesUsers>> {
        let users = self.userResolver();
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
        self.userResolver().setFieldResolver(Arc::new(resolver))?;
        Ok(self)
    }
    fn userResolver(&self) -> Arc<dyn crate::ResolvesUsers> {
        self.options
            .legacyUsers
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
            .legacyUsers
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(resolver));
        self
    }
    pub fn withOptions(mut self, options: Arc<Options>) -> Self {
        self.options = options;
        self
    }
    pub fn withTrimLottery(mut self, lottery: openrust::support::Lottery) -> Self {
        self.lottery = lottery;
        self
    }
    pub fn withBuffer(mut self, buffer: usize) -> Self {
        self.buffer = buffer;
        self
    }
    pub fn defaultVendorCacheKeys() -> Vec<String> {
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
    pub fn ignoreRoutes(&self) {
        self.options.registersRoutes.store(false, Ordering::Relaxed);
    }
    pub fn registersRoutes(&self) -> bool {
        self.options.registersRoutes.load(Ordering::Relaxed)
    }
    pub fn startRecording(&self) {
        self.shouldRecord.store(true, Ordering::Relaxed);
    }
    pub fn stopRecording(&self) {
        self.shouldRecord.store(false, Ordering::Relaxed);
    }
    pub(crate) fn isIgnoring(options: &Arc<Options>) -> bool {
        IGNORED
            .try_with(|ignored| ignored.iter().any(|owner| Arc::ptr_eq(owner, options)))
            .unwrap_or(false)
    }
    pub fn isRecording(&self) -> bool {
        self.shouldRecord.load(Ordering::Relaxed)
            && self.ignoring.load(Ordering::Relaxed) == 0
            && !Self::isIgnoring(&self.options)
    }
    pub async fn ignore<T>(&self, callback: impl Future<Output = T>) -> T {
        self.ignoring.fetch_add(1, Ordering::Relaxed);
        let _guard = Ignore(&self.ignoring);
        // Deferred callbacks run after the original container scope closes. Keep
        // suppression with this application while native events resolve a fresh scope.
        let mut ignored = IGNORED.try_with(Clone::clone).unwrap_or_default();
        ignored.push(self.options.clone());
        IGNORED.scope(ignored, callback).await
    }
    pub fn handleExceptionsUsing(&self, callback: impl Fn(&Error) + Send + Sync + 'static) {
        *self
            .options
            .handler
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(callback));
    }
    pub(crate) fn reportError(&self, error: &Error) {
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
                self.reportError(&error);
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
        let entry = Entry::new(timestamp.unwrap_or_else(Self::now), r#type, key, value);
        if self.isRecording() {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(entry.clone().into());
            self.ingestWhenOverBufferSize().await;
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
        let value = Value::new(timestamp.unwrap_or_else(Self::now), r#type, key, value);
        if self.isRecording() {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(value.clone().into());
            self.ingestWhenOverBufferSize().await;
        }
        value
    }
    pub async fn lazy(
        &self,
        callback: impl for<'a> FnOnce(&'a Pulse) -> PulseFuture<'a, ()> + Send + 'static,
    ) {
        if self.isRecording() {
            self.lazy
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Box::new(callback));
            self.ingestWhenOverBufferSize().await;
        }
    }
    pub(crate) fn defer(
        &self,
        callback: impl for<'a> FnOnce(&'a Pulse) -> PulseFuture<'a, ()> + Send + 'static,
    ) {
        if self.isRecording() {
            self.lazy
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Box::new(callback));
        }
    }
    pub fn rememberUser(&self, user: &dyn openrust::contracts::auth::Authenticatable) -> &Self {
        *self
            .rememberedUserId
            .write()
            .unwrap_or_else(|e| e.into_inner()) = self.userResolver().key(user);
        self
    }
    pub fn authenticatedUserIdResolver(
        &self,
    ) -> impl Fn(&Pulse) -> Option<String> + Send + Sync + use<> {
        use openrust::support::facades::Auth;
        let key = Auth::manager()
            .ok()
            .and_then(|auth| auth.resolved_user(None))
            .and_then(|user| self.userResolver().key(user.as_ref()));
        move |pulse| key.clone().or_else(|| pulse.resolveAuthenticatedUserId())
    }
    pub fn resolveAuthenticatedUserId(&self) -> Option<String> {
        use openrust::support::facades::Auth;
        Auth::manager()
            .ok()
            .and_then(|a| a.resolved_user(None))
            .and_then(|u| self.userResolver().key(u.as_ref()))
            .or_else(|| {
                self.rememberedUserId
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
            })
    }
    pub async fn report(&self, class: impl Into<String>, location: Option<String>) -> &Self {
        self.rescue(async {
            use openrust::{
                contracts::events::Dispatcher as _,
                support::facades::{Event, Facade},
            };
            Event::get_facade_root()?
                .dispatch(
                    Arc::new(crate::events::ExceptionReported {
                        class: class.into(),
                        location,
                    }),
                    &mut openrust::events::EventContext::new(),
                )
                .await?;
            Ok(())
        })
        .await;
        self
    }
    pub fn wantsIngesting(&self) -> bool {
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
            .rememberedUserId
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.lazy.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    async fn resolveLazyEntries(&self) {
        let lazy = std::mem::take(&mut *self.lazy.lock().unwrap_or_else(|e| e.into_inner()));
        for callback in lazy {
            if let Err(e) = callback(self).await {
                self.reportError(&e);
                break;
            }
        }
        self.lazy.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    fn ingestWhenOverBufferSize(&self) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if self.evaluatingBuffer.swap(true, Ordering::Relaxed) {
                return;
            }
            struct Reset<'a>(&'a AtomicBool);
            impl Drop for Reset<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Relaxed);
                }
            }
            let _guard = Reset(&self.evaluatingBuffer);
            let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner()).len();
            let lazy = self.lazy.lock().unwrap_or_else(|e| e.into_inner()).len();
            if entries.saturating_add(lazy) > self.buffer {
                self.resolveLazyEntries().await;
            }
            if self.entries.lock().unwrap_or_else(|e| e.into_inner()).len() > self.buffer {
                self.ingest().await;
            }
        })
    }
    pub fn ingest(&self) -> std::pin::Pin<Box<dyn Future<Output = usize> + Send + '_>> {
        Box::pin(async move {
            self.resolveLazyEntries().await;
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
                                self.reportError(&e);
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
                        self.reportError(&e);
                        0
                    }
                };
                if self.lottery.choose()
                    && let Err(e) = self.ingest.trim().await
                {
                    self.reportError(&e);
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
    pub async fn graph(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> Result<crate::storage::Graph> {
        self.ignore(self.storage.graph(types, aggregate, interval))
            .await
    }
    pub async fn aggregate(
        &self,
        r#type: &str,
        aggregates: &[String],
        interval: i64,
        orderBy: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> Result<Vec<openrust::database::Row>> {
        self.ignore(
            self.storage
                .aggregate(r#type, aggregates, interval, orderBy, direction, limit),
        )
        .await
    }
    pub async fn aggregateTypes(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
        orderBy: Option<&str>,
        direction: &str,
        limit: u64,
    ) -> Result<Vec<openrust::database::Row>> {
        self.ignore(
            self.storage
                .aggregateTypes(types, aggregate, interval, orderBy, direction, limit),
        )
        .await
    }
    pub async fn aggregateTotal(
        &self,
        types: &[String],
        aggregate: &str,
        interval: i64,
    ) -> Result<indexmap::IndexMap<String, Option<f64>>> {
        self.ignore(self.storage.aggregateTotal(types, aggregate, interval))
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
