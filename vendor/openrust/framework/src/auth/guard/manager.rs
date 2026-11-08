//! 配置具名 driver 的请求级解析；注册表可共享，已登录状态不可共享。

use super::contracts::{AuthUser, Guard, UserProvider};
use crate::container::Resolver;
use crate::{Error, Result, http::axum::http::StatusCode, support::BoxFuture};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock, Weak},
};
use tokio::sync::OnceCell;

/// 同一 `ConfigRepository` 的 `auth.defaults` 区段。
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthDefaults {
    /// 默认具名 Guard。
    pub guard: String,
    /// 默认密码 broker；本区段不创建独立密码重置流程。
    pub passwords: String,
    /// `create_user_provider(None)` 的显式默认；不从默认Guard反向推断。
    #[serde(default)]
    pub provider: Option<String>,
}

/// auth.guards 中单个 Guard 的声明；额外配置原样传给 driver。
#[derive(Clone, Serialize, Deserialize)]
pub struct GuardConfig {
    /// 内置或通过 `AuthDrivers` 注册的 driver 名。
    pub driver: String,
    /// 账户 Provider 名；无账户来源的自定义 driver 可不设置。
    pub provider: Option<String>,
    /// driver 专有设置。
    #[serde(flatten)]
    pub options: Map<String, Value>,
}

/// auth.providers 中单个账户来源的声明。
#[derive(Clone, Serialize, Deserialize)]
pub struct UserProviderConfig {
    /// 已注册的 Provider driver 名。
    pub driver: String,
    /// 连接、表、模型等由 driver 解读的设置。
    #[serde(flatten)]
    pub options: Map<String, Value>,
}

/// Guard driver 工厂所用当前请求解析上下文。
pub struct GuardContext {
    /// 当前请求的同一容器 resolver，不得保存到应用级已登录缓存。
    pub resolver: Resolver,
    /// 当前具名 Guard。
    pub name: String,
    /// 原始 Guard 配置。
    pub config: GuardConfig,
    drivers: AuthDrivers,
}

impl GuardContext {
    /// 返回当前请求快照；自定义driver无需另建请求或从全局读取凭据。
    pub async fn request(&self) -> Result<Arc<crate::http::RequestInfo>> {
        super::registration::request_info(&self.resolver).await
    }

    /// 返回本driver实际选择账户来源时使用的名称，不触发账户Provider工厂。
    pub fn user_provider_name(&self) -> Result<Option<String>> {
        selected_provider_name(&self.resolver, self.config.provider.as_deref())
    }
    /// 仅在driver实际需要时解析配置账户来源，不让独立custom guard触发Provider副作用。
    pub async fn user_provider(&self) -> Result<Option<Arc<dyn UserProvider>>> {
        create_user_provider(
            &self.resolver,
            &self.drivers,
            self.config.provider.as_deref(),
        )
        .await
    }
}

type GuardCreator =
    Arc<dyn Fn(GuardContext) -> BoxFuture<'static, Result<Arc<dyn Guard>>> + Send + Sync>;
type ProviderCreator = Arc<
    dyn Fn(Resolver, UserProviderConfig) -> BoxFuture<'static, Result<Arc<dyn UserProvider>>>
        + Send
        + Sync,
>;

/// 应用级 driver 注册表；只保存工厂，不保存请求用户、Session 或 Guard。
#[derive(Clone, Default)]
pub struct AuthDrivers {
    guards: Arc<RwLock<BTreeMap<String, GuardCreator>>>,
    providers: Arc<RwLock<BTreeMap<String, ProviderCreator>>>,
}

impl AuthDrivers {
    /// 注册或替换 Guard driver；已构造 Guard 保持，`forget_guards` 后按新工厂解析。
    pub fn extend<F>(&self, driver: impl Into<String>, creator: F) -> Result<()>
    where
        F: Fn(GuardContext) -> BoxFuture<'static, Result<Arc<dyn Guard>>> + Send + Sync + 'static,
    {
        self.guards
            .write()
            .map_err(|_| state_error())?
            .insert(driver.into(), Arc::new(creator));
        Ok(())
    }
    /// 注册或替换账户 Provider driver，不按名称猜测不支持的模型实现。
    pub fn provider<F>(&self, driver: impl Into<String>, creator: F) -> Result<()>
    where
        F: Fn(Resolver, UserProviderConfig) -> BoxFuture<'static, Result<Arc<dyn UserProvider>>>
            + Send
            + Sync
            + 'static,
    {
        self.providers
            .write()
            .map_err(|_| state_error())?
            .insert(driver.into(), Arc::new(creator));
        Ok(())
    }
    fn guard_creator(&self, name: &str) -> Result<GuardCreator> {
        self.guards
            .read()
            .map_err(|_| state_error())?
            .get(name)
            .cloned()
            .ok_or_else(|| {
                Error::custom_with_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "auth_guard_driver_undefined",
                    "认证Guard driver未注册",
                )
            })
    }
    fn provider_creator(&self, name: &str) -> Result<ProviderCreator> {
        self.providers
            .read()
            .map_err(|_| state_error())?
            .get(name)
            .cloned()
            .ok_or_else(|| {
                Error::custom_with_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "auth_user_provider_driver_undefined",
                    "认证UserProvider driver未注册",
                )
            })
    }
}

type GuardCell = Arc<GuardSlot>;
#[derive(Default)]
struct GuardSlot {
    value: OnceCell<Arc<dyn Guard>>,
    status: Mutex<ResolutionStatus>,
}
#[derive(Default)]
struct ResolutionStatus {
    active: usize,
    cyclic: bool,
}
impl GuardSlot {
    fn enter(self: &Arc<Self>) -> Result<ActiveResolution> {
        let mut status = self.status.lock().map_err(|_| state_error())?;
        if status.active == 0 {
            status.cyclic = false;
        }
        status.active = status.active.checked_add(1).ok_or_else(state_error)?;
        Ok(ActiveResolution(Arc::clone(self)))
    }
    fn cyclic(&self) -> Result<bool> {
        Ok(self.status.lock().map_err(|_| state_error())?.cyclic)
    }
    fn mark_cycle(&self) -> Result<()> {
        self.status.lock().map_err(|_| state_error())?.cyclic = true;
        Ok(())
    }
}
struct ActiveResolution(GuardCell);
impl Drop for ActiveResolution {
    fn drop(&mut self) {
        if let Ok(mut status) = self.0.status.lock() {
            status.active = status.active.saturating_sub(1);
        }
    }
}
#[derive(Clone)]
struct ResolutionNode {
    manager: usize,
    cell: GuardCell,
}
impl ResolutionNode {
    fn identity(&self) -> usize {
        Arc::as_ptr(&self.cell) as usize
    }
}
tokio::task_local! { static RESOLVING: Vec<ResolutionNode>; }

#[derive(Default)]
struct WaitGraph {
    edges: BTreeMap<usize, BTreeMap<usize, usize>>,
    nodes: BTreeMap<usize, Weak<GuardSlot>>,
}
impl WaitGraph {
    fn path(&self, start: usize, end: usize) -> Option<Vec<usize>> {
        let mut pending = vec![vec![start]];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(path) = pending.pop() {
            let node = *path.last()?;
            if node == end {
                return Some(path);
            }
            if !seen.insert(node) {
                continue;
            }
            if let Some(targets) = self.edges.get(&node) {
                for target in targets.keys() {
                    let mut next = path.clone();
                    next.push(*target);
                    pending.push(next);
                }
            }
        }
        None
    }
    fn connect(&mut self, source: &GuardCell, target: &GuardCell) -> Result<()> {
        let from = Arc::as_ptr(source) as usize;
        let to = Arc::as_ptr(target) as usize;
        self.nodes.insert(from, Arc::downgrade(source));
        self.nodes.insert(to, Arc::downgrade(target));
        if let Some(path) = self.path(to, from) {
            // 标记整条环，阻止 OnceCell 的在途 waiter 在失败后重新执行屏障工厂。
            for node in path {
                if let Some(slot) = self.nodes.get(&node).and_then(Weak::upgrade) {
                    slot.mark_cycle()?;
                }
            }
            return Err(cycle_error());
        }
        let count = self.edges.entry(from).or_default().entry(to).or_default();
        *count = count.checked_add(1).ok_or_else(state_error)?;
        Ok(())
    }
    fn disconnect(&mut self, from: usize, to: usize) {
        if let Some(targets) = self.edges.get_mut(&from) {
            if let Some(count) = targets.get_mut(&to) {
                if *count > 1 {
                    *count -= 1;
                } else {
                    targets.remove(&to);
                }
            }
            if targets.is_empty() {
                self.edges.remove(&from);
            }
        }
        for node in [from, to] {
            if !self.edges.contains_key(&node)
                && !self
                    .edges
                    .values()
                    .any(|targets| targets.contains_key(&node))
            {
                self.nodes.remove(&node);
            }
        }
    }
}
struct ResolutionWait<'a> {
    graph: &'a Mutex<WaitGraph>,
    from: usize,
    to: usize,
}
impl Drop for ResolutionWait<'_> {
    fn drop(&mut self) {
        if let Ok(mut graph) = self.graph.lock() {
            graph.disconnect(self.from, self.to);
        }
    }
}

/// 当前请求的用户解析回调，收到属性或调用方提供的原始 Guard 选择。
pub type UserResolver = dyn for<'a> Fn(&'a AuthManager, Option<&'a str>) -> BoxFuture<'a, Result<Option<AuthUser>>>
    + Send
    + Sync;

#[derive(Default)]
struct UserSelection {
    default_guard: Option<String>,
    resolver: Option<Arc<UserResolver>>,
}

/// 当前请求的具名 Guard 缓存；必须通过容器 scoped 绑定创建。
///
/// 共享应用只保存 `AuthDrivers`；`should_use`、`forget_guards` 与已解析 Guard 均不影响其他请求。
pub struct AuthManager {
    resolver: Resolver,
    drivers: AuthDrivers,
    selection: RwLock<UserSelection>,
    guards: Mutex<BTreeMap<String, GuardCell>>,
    waits: Mutex<WaitGraph>,
    identity: Arc<()>,
}

impl AuthManager {
    /// 绑定当前请求 resolver；调用方不得将该实例注册为应用 singleton。
    pub fn new(resolver: Resolver, drivers: AuthDrivers) -> Self {
        Self {
            resolver,
            drivers,
            selection: RwLock::new(UserSelection::default()),
            guards: Mutex::new(BTreeMap::new()),
            waits: Mutex::new(WaitGraph::default()),
            identity: Arc::new(()),
        }
    }
    /// 返回当前请求默认名；未覆写时读同一 auth.defaults 配置。
    pub fn default_driver(&self) -> Result<String> {
        if let Some(name) = self
            .selection
            .read()
            .map_err(|_| state_error())?
            .default_guard
            .clone()
        {
            return Ok(name);
        }
        self.configured_default_driver()
    }

    fn configured_default_driver(&self) -> Result<String> {
        Ok(self
            .resolver
            .section::<AuthDefaults>("auth.defaults")?
            .guard)
    }

    pub(in crate::auth) fn guard_name(&self, name: Option<&str>) -> Result<String> {
        match name {
            None | Some("" | "0") => self.default_driver(),
            Some(name) => Ok(name.to_owned()),
        }
    }

    /// 选择当前请求的默认 Guard 并恢复内置用户解析器。
    /// 空字符串或 `"0"` 沿用当前默认名。
    pub fn should_use(&self, name: impl Into<String>) -> Result<()> {
        self.select_guard(name.into(), false)
    }

    pub(in crate::auth) fn use_configured_guard(&self, name: String) -> Result<()> {
        self.select_guard(name, true)
    }

    fn select_guard(&self, name: String, configured: bool) -> Result<()> {
        let retired = {
            let mut selection = self.selection.write().map_err(|_| state_error())?;
            let name = match name.as_str() {
                "" | "0" if !configured => match &selection.default_guard {
                    Some(current) => current.clone(),
                    None => self.configured_default_driver()?,
                },
                _ => name,
            };
            selection.default_guard = Some(name);
            selection.resolver.take()
        };
        drop(retired);
        Ok(())
    }

    /// 替换当前请求的用户解析器，供 `Authenticated`、`CurrentUser` 及 `user` 使用。
    pub fn resolve_users_using(
        &self,
        resolver: impl for<'a> Fn(
            &'a AuthManager,
            Option<&'a str>,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        let retired = self
            .selection
            .write()
            .map_err(|_| state_error())?
            .resolver
            .replace(Arc::new(resolver));
        drop(retired);
        Ok(())
    }

    /// 返回当前用户解析回调；默认回调读取选中 Guard 的用户。
    pub fn user_resolver(&self) -> Result<Arc<UserResolver>> {
        Ok(self
            .selection
            .read()
            .map_err(|_| state_error())?
            .resolver
            .clone()
            .unwrap_or_else(|| Arc::new(default_user)))
    }

    /// 通过当前回调读取账户；访客返回 `None`，解析错误原样传播。
    pub async fn user(&self, guard: Option<&str>) -> Result<Option<AuthUser>> {
        (self.user_resolver()?)(self, guard).await
    }
    /// 清除当前请求的 Guard 缓存；已有外部句柄仍代表其原实例。
    pub fn forget_guards(&self) -> Result<()> {
        self.guards.lock().map_err(|_| state_error())?.clear();
        Ok(())
    }
    /// 按配置名精确解析；`None` 或空名使用默认 Guard。同请求复用 Guard 实例。
    pub fn guard<'a>(&'a self, name: Option<&'a str>) -> BoxFuture<'a, Result<Arc<dyn Guard>>> {
        Box::pin(async move {
            let name = match name.filter(|name| !name.is_empty()) {
                Some(name) => name.to_owned(),
                None => self.default_driver()?,
            };
            self.resolve_guard(name).await
        })
    }

    pub(crate) async fn guard_for_attribute(&self, name: Option<&str>) -> Result<Arc<dyn Guard>> {
        self.resolve_guard(self.guard_name(name)?).await
    }
    async fn resolve_guard(&self, name: String) -> Result<Arc<dyn Guard>> {
        let manager = Arc::as_ptr(&self.identity) as usize;
        let cell = {
            let mut guards = self.guards.lock().map_err(|_| state_error())?;
            guards
                .entry(name.clone())
                .or_insert_with(|| Arc::new(GuardSlot::default()))
                .clone()
        };
        let path = RESOLVING.try_with(Clone::clone).unwrap_or_default();
        if let Some(index) = path
            .iter()
            .position(|node| node.manager == manager && Arc::ptr_eq(&node.cell, &cell))
        {
            for node in &path[index..] {
                if node.manager == manager {
                    node.cell.mark_cycle()?;
                }
            }
            return Err(cycle_error());
        }
        let _active = cell.enter()?;
        let _waiting = if let Some(parent) = path.iter().rev().find(|node| node.manager == manager)
        {
            self.waits
                .lock()
                .map_err(|_| state_error())?
                .connect(&parent.cell, &cell)?;
            Some(ResolutionWait {
                graph: &self.waits,
                from: parent.identity(),
                to: Arc::as_ptr(&cell) as usize,
            })
        } else {
            None
        };
        let mut creator_path = path;
        creator_path.push(ResolutionNode {
            manager,
            cell: Arc::clone(&cell),
        });
        let value = cell
            .value
            .get_or_try_init(|| {
                RESOLVING.scope(creator_path, async {
                    if cell.cyclic()? {
                        return Err(cycle_error());
                    }
                    let guards = self
                        .resolver
                        .section::<BTreeMap<String, GuardConfig>>("auth.guards")?;
                    let config = guards.get(&name).cloned().ok_or_else(|| {
                        Error::custom_with_status(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "auth_guard_undefined",
                            "具名认证Guard未配置",
                        )
                    })?;
                    let creator = self.drivers.guard_creator(&config.driver)?;
                    let guard = creator(GuardContext {
                        resolver: self.resolver.clone(),
                        name,
                        config,
                        drivers: self.drivers.clone(),
                    })
                    .await?;
                    if cell.cyclic()? {
                        return Err(cycle_error());
                    }
                    Ok(guard)
                })
            })
            .await?;
        Ok(Arc::clone(value))
    }
    /// 按同一 auth.providers 配置创建账户来源，不缓存用户或绑定全局Session。
    pub async fn create_user_provider(
        &self,
        name: Option<&str>,
    ) -> Result<Option<Arc<dyn UserProvider>>> {
        create_user_provider(&self.resolver, &self.drivers, name).await
    }
}

fn default_user<'a>(
    manager: &'a AuthManager,
    guard: Option<&'a str>,
) -> BoxFuture<'a, Result<Option<AuthUser>>> {
    Box::pin(async move { manager.guard_for_attribute(guard).await?.user().await })
}

fn cycle_error() -> Error {
    Error::custom(
        "auth_guard_resolution_cycle",
        "认证Guard工厂存在递归或并发等待环",
    )
}
fn state_error() -> Error {
    Error::custom("auth_manager_state_poisoned", "认证管理器状态不可用")
}

#[cfg(test)]
#[path = "../../../tests/unit/auth/guard_manager.rs"]
mod tests;

async fn create_user_provider(
    resolver: &Resolver,
    drivers: &AuthDrivers,
    name: Option<&str>,
) -> Result<Option<Arc<dyn UserProvider>>> {
    let selected = selected_provider_name(resolver, name)?;
    let Some(selected) = selected.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let providers = resolver.section::<BTreeMap<String, UserProviderConfig>>("auth.providers")?;
    let Some(config) = providers.get(&selected).cloned() else {
        return Ok(None);
    };
    Ok(Some(
        drivers.provider_creator(&config.driver)?(resolver.clone(), config).await?,
    ))
}

fn selected_provider_name(resolver: &Resolver, name: Option<&str>) -> Result<Option<String>> {
    Ok(match name.filter(|value| !value.is_empty()) {
        Some(name) => Some(name.to_owned()),
        None => resolver.section::<AuthDefaults>("auth.defaults")?.provider,
    }
    .filter(|value| !value.is_empty()))
}
