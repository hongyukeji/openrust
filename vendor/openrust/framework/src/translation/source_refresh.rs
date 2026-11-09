//! Loader快照的单flight捕获、完整校验与原子发布；不依赖任何数据库扩展。

#[cfg(test)]
use super::{
    CompositeTranslationLoader, Locale, MessageArgs, TranslationLookup, with_request_locale,
};
use super::{
    MessageCatalog, TranslationConfig, TranslationLines, TranslationLoadSession, TranslationLoader,
    TranslationResource, Translator, loader,
};
use crate::{Error, Result};
#[cfg(test)]
use std::future::Future;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fmt,
    ops::Deref,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

/// 同时到达的刷新调用共享的结果；普通翻译始终只读取已发布目录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationRefreshOutcome {
    /// 一个完整目录已发布。
    Updated {
        /// 当前发布代次。
        generation: u64,
    },
    /// 来源无需更新或未配置Loader。
    Unchanged {
        /// 当前发布代次。
        generation: u64,
    },
}
/// 可观察的脱敏来源状态，不包含SQL、路径或底层异常文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationRefreshStatus {
    /// 是否配置可刷新来源。
    pub configured: bool,
    /// 当前已验证目录的发布代次，亦包含模块资源登记。
    pub generation: u64,
    /// 已开始的来源尝试次数。
    pub attempts: u64,
    /// 是否正在捕获或检查来源。
    pub refreshing: bool,
    /// 最近失败的固定代码；恢复后清空。
    pub last_error_code: Option<&'static str>,
}

/// 解析无重复键的JSON对象；消息结构与资源地址由`translation_session`作完整校验。
pub fn parse_translation_lines(bytes: &[u8]) -> Result<TranslationLines> {
    loader::parse_lines(bytes)
}
/// 验证并创建不可变session；自定义Loader与独立扩展共用相同资源校验。
pub fn translation_session(
    resources: BTreeMap<TranslationResource, TranslationLines>,
) -> Result<Arc<dyn TranslationLoadSession>> {
    Ok(Arc::new(loader::MaterializedSession::new(resources)?))
}

pub(super) struct RefreshRuntime {
    loader: Arc<dyn TranslationLoader>,
    flight: std::sync::Mutex<Option<Arc<Flight>>>,
}
impl RefreshRuntime {
    // 先锁当前flight再锁后继；返回前释放两锁，等待不持锁。
    fn select_flight(&self, predecessor: Option<&Arc<Flight>>) -> Result<(Arc<Flight>, bool)> {
        let selected = {
            let mut slot = self.flight.lock().map_err(|_| catalog_error())?;
            let mut successor = match predecessor {
                Some(previous) => Some(previous.successor.lock().map_err(|_| catalog_error())?),
                None => None,
            };
            if let Some(shared) = successor.as_ref().and_then(|next| next.as_ref()) {
                (Arc::clone(shared), false)
            } else {
                // finish已写结果但尚未清slot的瞬间，不将旧检查链接到自身。
                let active = slot.as_ref().filter(|active| {
                    predecessor.is_none_or(|previous| !Arc::ptr_eq(active, previous))
                });
                let (flight, owner) = if let Some(active) = active {
                    (Arc::clone(active), false)
                } else {
                    let flight = Arc::new(Flight {
                        result: std::sync::Mutex::new(None),
                        completed: tokio::sync::Notify::new(),
                        successor: std::sync::Mutex::new(None),
                    });
                    *slot = Some(Arc::clone(&flight));
                    (flight, true)
                };
                if let Some(next) = successor.as_mut() {
                    **next = Some(Arc::clone(&flight));
                }
                (flight, owner)
            }
        };
        Ok(selected)
    }
}
impl fmt::Debug for RefreshRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefreshRuntime([configured])")
    }
}
pub(super) struct PublishedCatalog {
    current: Arc<MessageCatalog>,
    published: Option<Arc<dyn TranslationLoadSession>>,
    overlay: MessageCatalog,
    namespaces: HashSet<String>,
    generation: u64,
    attempts: u64,
    finished: u64,
    refreshing: bool,
    last_error: Option<&'static str>,
    failed_at: Option<Instant>,
}
impl fmt::Debug for PublishedCatalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublishedCatalog")
            .field("generation", &self.generation)
            .field("refreshing", &self.refreshing)
            .field("last_error", &self.last_error)
            .finish_non_exhaustive()
    }
}
impl Deref for PublishedCatalog {
    type Target = MessageCatalog;
    fn deref(&self) -> &Self::Target {
        &self.current
    }
}
impl PublishedCatalog {
    pub(super) fn new(current: MessageCatalog) -> Self {
        Self {
            current: Arc::new(current),
            published: None,
            overlay: empty_catalog(),
            namespaces: HashSet::new(),
            generation: 0,
            attempts: 0,
            finished: 0,
            refreshing: false,
            last_error: None,
            failed_at: None,
        }
    }
    pub(super) fn snapshot(&self) -> Arc<MessageCatalog> {
        self.current.clone()
    }
    fn complete(&mut self, _outcome: TranslationRefreshOutcome, error: Option<&'static str>) {
        // 开始尝试前已为finished预留一代，namespace登记不消耗此计数。
        self.finished += 1;
        self.refreshing = false;
        self.last_error = error;
        self.failed_at = error.map(|_| Instant::now());
    }
}
pub(super) fn empty_catalog() -> MessageCatalog {
    MessageCatalog {
        messages: BTreeMap::new(),
        resource_messages: BTreeMap::new(),
        source_global_keys: BTreeMap::new(),
    }
}
fn merge_catalog(target: &mut MessageCatalog, source: &MessageCatalog) {
    for (locale, lines) in &source.messages {
        target
            .messages
            .entry(locale.clone())
            .or_default()
            .extend(lines.clone());
    }
    for (resource, lines) in &source.resource_messages {
        target
            .resource_messages
            .entry(resource.clone())
            .or_default()
            .extend(lines.clone());
    }
}
fn merge_module_overlay(target: &mut MessageCatalog, overlay: &MessageCatalog) {
    let mut fallback = overlay.clone();
    for (locale, lines) in &mut fallback.messages {
        lines.retain(|key, _| {
            !(target
                .source_global_keys
                .get(locale)
                .is_some_and(|keys| keys.contains(key))
                || (key.contains("::") && target.group_message_at(locale, key).is_some()))
        });
    }
    merge_catalog(target, &fallback);
}

fn validate_catalog(config: &TranslationConfig, catalog: &MessageCatalog) -> Result<()> {
    if !catalog.supports(&config.default_locale) || !catalog.supports(&config.fallback_locale) {
        return Err(source_error("i18n_source_catalog_invalid"));
    }
    Ok(())
}
fn catalog_from_session(
    session: &dyn TranslationLoadSession,
    config: &TranslationConfig,
) -> Result<MessageCatalog> {
    let mut catalog = MessageCatalog::new();
    let mut seen = BTreeSet::new();
    for resource in session.resources() {
        if !seen.insert(resource.clone()) {
            return Err(source_error("i18n_source_catalog_invalid"));
        }
        let lines = session
            .load(
                &resource.locale,
                &resource.group,
                resource.namespace.as_deref(),
            )
            .map_err(|_| source_error("i18n_source_catalog_invalid"))?;
        let messages = loader::parse_resource(resource, &lines)
            .map_err(|_| source_error("i18n_source_catalog_invalid"))?;
        catalog.messages.entry(resource.locale.clone()).or_default();
        if resource.group == "*" && resource.namespace.is_none() {
            // 保留显式Source键身份，避免模块的旧JSON别名覆写数据库/文件Source。
            catalog
                .source_global_keys
                .entry(resource.locale.clone())
                .or_default()
                .extend(messages.keys().cloned());
            catalog
                .messages
                .entry(resource.locale.clone())
                .or_default()
                .extend(messages);
        } else {
            catalog.resource_messages.insert(resource.clone(), messages);
        }
    }
    validate_catalog(config, &catalog)?;
    Ok(catalog)
}
fn source_error(code: &'static str) -> Error {
    Error::custom_with_status(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        code,
        "翻译来源当前不可用",
    )
}
fn catalog_error() -> Error {
    source_error("i18n_catalog_unavailable")
}
type FlightResult = std::result::Result<TranslationRefreshOutcome, &'static str>;
#[derive(Clone, Copy)]
struct FlightCompletion {
    result: FlightResult,
    captured: bool,
}
struct Flight {
    result: std::sync::Mutex<Option<FlightCompletion>>,
    completed: tokio::sync::Notify,
    successor: std::sync::Mutex<Option<Arc<Flight>>>,
}
impl Flight {
    fn result(&self) -> Result<Option<FlightCompletion>> {
        self.result
            .lock()
            .map(|result| *result)
            .map_err(|_| catalog_error())
    }
    async fn wait(&self) -> Result<FlightCompletion> {
        loop {
            let notified = self.completed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(result) = self.result()? {
                return Ok(result);
            }
            notified.await;
        }
    }
}
tokio::task_local! { static ACTIVE_REFRESH: Vec<usize>; }
struct Attempt {
    state: Arc<RwLock<PublishedCatalog>>,
    runtime: Arc<RefreshRuntime>,
    flight: Arc<Flight>,
    armed: bool,
    started: bool,
    captured: bool,
}
impl Attempt {
    fn finish(&mut self, result: FlightResult) {
        *self
            .flight
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(FlightCompletion {
            result,
            captured: self.captured,
        });
        {
            let mut slot = self
                .runtime
                .flight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if slot
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, &self.flight))
            {
                *slot = None;
            }
        }
        self.armed = false;
        self.flight.completed.notify_waiters();
    }
    fn fail(&mut self, code: &'static str) -> Result<TranslationRefreshOutcome> {
        {
            let mut state = self.state.write().map_err(|_| catalog_error())?;
            let generation = state.generation;
            state.complete(
                TranslationRefreshOutcome::Unchanged { generation },
                Some(code),
            );
        }
        self.finish(Err(code));
        Err(source_error(code))
    }
    fn unchanged(&mut self) -> Result<TranslationRefreshOutcome> {
        let outcome = {
            let mut state = self.state.write().map_err(|_| catalog_error())?;
            let outcome = TranslationRefreshOutcome::Unchanged {
                generation: state.generation,
            };
            state.complete(outcome, None);
            outcome
        };
        self.finish(Ok(outcome));
        Ok(outcome)
    }
    fn publish(
        &mut self,
        mut next: MessageCatalog,
        session: Arc<dyn TranslationLoadSession>,
        config: &TranslationConfig,
    ) -> Result<TranslationRefreshOutcome> {
        let (outcome, previous) = {
            let mut state = self.state.write().map_err(|_| catalog_error())?;
            merge_module_overlay(&mut next, &state.overlay);
            validate_catalog(config, &next)?;
            let generation = state.generation.checked_add(1).ok_or_else(catalog_error)?;
            state.current = Arc::new(next);
            let previous = state.published.replace(session);
            state.generation = generation;
            let outcome = TranslationRefreshOutcome::Updated { generation };
            state.complete(outcome, None);
            (outcome, previous)
        };
        // 插件session的析构同样必须在目录锁外。
        // 目录与游标已提交；先不可逆地公布成功并解除取消收据，插件Drop panic仅向owner传播。
        self.finish(Ok(outcome));
        drop(previous);
        Ok(outcome)
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if self.armed {
            if self.started {
                let mut state = self
                    .state
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let generation = state.generation;
                state.complete(
                    TranslationRefreshOutcome::Unchanged { generation },
                    Some("i18n_source_refresh_cancelled"),
                );
            }
            self.finish(Err("i18n_source_refresh_cancelled"));
        }
    }
}
impl Translator {
    /// 从唯一Loader取得完整初始目录；初始失败明确返回错误，不静默切换来源。
    pub async fn from_loader(
        config: TranslationConfig,
        loader: Arc<dyn TranslationLoader>,
    ) -> Result<Self> {
        let session = loader
            .begin_snapshot()
            .await
            .map_err(|_| source_error("i18n_source_refresh_failed"))?;
        let catalog = catalog_from_session(session.as_ref(), &config)?;
        let mut service =
            Self::new(config, catalog).map_err(|_| source_error("i18n_source_catalog_invalid"))?;
        service.source = Some(Arc::new(RefreshRuntime {
            loader,
            flight: std::sync::Mutex::new(None),
        }));
        {
            let mut state = service.catalog.write().map_err(|_| catalog_error())?;
            state.published = Some(session);
            state.generation = 1;
            state.attempts = 1;
            state.finished = 1;
        }
        Ok(service)
    }
    /// 显式重新捕获来源，绕过失败冷却；并发捕获共享同一已完成尝试。
    ///
    /// 同一异步调用链内的来源回调重入立即返回固定错误。插件另起独立任务再等待该任务
    /// 反向刷新当前服务不继承调用链标记；插件不得构造这种循环等待。
    pub async fn refresh(&self) -> Result<TranslationRefreshOutcome> {
        self.refresh_source(true).await
    }
    /// 请求或后台异步边界按来源提示刷新；失败冷却30秒，翻译本身不会执行I/O。
    pub async fn refresh_if_due(&self) -> Result<TranslationRefreshOutcome> {
        self.refresh_source(false).await
    }
    /// 读取固定错误代码、发布代次与进行状态，不触发来源读取。
    pub fn refresh_status(&self) -> Result<TranslationRefreshStatus> {
        let state = self.catalog.read().map_err(|_| catalog_error())?;
        Ok(TranslationRefreshStatus {
            configured: self.source.is_some(),
            generation: state.generation,
            attempts: state.attempts,
            refreshing: state.refreshing,
            last_error_code: state.last_error,
        })
    }
    async fn refresh_source(&self, explicit: bool) -> Result<TranslationRefreshOutcome> {
        let Some(runtime) = &self.source else {
            return Ok(TranslationRefreshOutcome::Unchanged {
                generation: self.refresh_status()?.generation,
            });
        };
        let identity = Arc::as_ptr(runtime) as usize;
        if ACTIVE_REFRESH
            .try_with(|active| active.contains(&identity))
            .unwrap_or(false)
        {
            return Err(source_error("i18n_source_refresh_reentrant"));
        }
        let mut predecessor: Option<Arc<Flight>> = None;
        let flight = loop {
            let (flight, owner) = runtime.select_flight(predecessor.as_ref())?;
            if owner {
                break flight;
            }
            let completed = flight.wait().await?;
            // 显式refresh不复用只做检查的结果；取消仍向已有等待者明确传播。
            if !explicit
                || completed.captured
                || matches!(completed.result, Err("i18n_source_refresh_cancelled"))
            {
                return completed.result.map_err(source_error);
            }
            // 同一次检查的显式等待者共享保留的后继结果，调度较晚也不能再次捕获。
            predecessor = Some(flight);
        };
        let mut attempt = Attempt {
            state: Arc::clone(&self.catalog),
            runtime: Arc::clone(runtime),
            flight,
            armed: true,
            started: false,
            captured: false,
        };
        let mut active = ACTIVE_REFRESH.try_with(Clone::clone).unwrap_or_default();
        active.push(identity);
        ACTIVE_REFRESH
            .scope(active, async {
                let (retrying, published) = {
                    let mut state = self.catalog.write().map_err(|_| catalog_error())?;
                    if !explicit
                        && state
                            .failed_at
                            .is_some_and(|time| time.elapsed() < Duration::from_secs(30))
                    {
                        let code = state.last_error.unwrap_or("i18n_source_refresh_failed");
                        drop(state);
                        attempt.finish(Err(code));
                        return Err(source_error(code));
                    }
                    let attempts = state.attempts.checked_add(1).ok_or_else(catalog_error)?;
                    state.finished.checked_add(1).ok_or_else(catalog_error)?;
                    state.generation.checked_add(1).ok_or_else(catalog_error)?;
                    state.attempts = attempts;
                    state.refreshing = true;
                    attempt.started = true;
                    (state.last_error.is_some(), state.published.clone())
                };
                if !explicit && !retrying {
                    let result = match published {
                        Some(published) => {
                            runtime.loader.needs_refresh_since(published.as_ref()).await
                        }
                        None => runtime.loader.needs_refresh().await,
                    };
                    match result {
                        Ok(false) => return attempt.unchanged(),
                        Ok(true) => {}
                        Err(_) => return attempt.fail("i18n_source_check_failed"),
                    }
                }
                attempt.captured = true;
                let snapshot = runtime.loader.begin_snapshot().await;
                let Ok(session) = snapshot else {
                    return attempt.fail("i18n_source_refresh_failed");
                };
                let catalog_result = catalog_from_session(session.as_ref(), &self.config);
                let Ok(catalog) = catalog_result else {
                    return attempt.fail("i18n_source_catalog_invalid");
                };
                match attempt.publish(catalog, session, &self.config) {
                    Ok(outcome) => Ok(outcome),
                    Err(_) => attempt.fail("i18n_source_catalog_invalid"),
                }
            })
            .await
    }

    pub(super) fn merge_namespace(&self, namespace: &str, overlay: &MessageCatalog) -> Result<()> {
        let mut state = self.catalog.write().map_err(|_| catalog_error())?;
        if state.namespaces.contains(namespace) {
            return Err(Error::custom(
                "i18n_namespace_conflict",
                "语言资源命名空间重复",
            ));
        }
        let generation = state.generation.checked_add(1).ok_or_else(catalog_error)?;
        let mut current = (*state.current).clone();
        if self.source.is_some() {
            merge_module_overlay(&mut current, overlay);
        } else {
            merge_catalog(&mut current, overlay);
        }
        merge_catalog(&mut state.overlay, overlay);
        state.current = Arc::new(current);
        state.namespaces.insert(namespace.into());
        state.generation = generation;
        Ok(())
    }
}

#[cfg(test)]
#[path = "source_refresh_tests.rs"]
mod tests;
