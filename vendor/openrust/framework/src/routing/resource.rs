//! 根据显式注册的控制器操作生成约定式资源路由。

use crate::foundation::ApplicationState;
use crate::routing::methods as routing;
use crate::routing::methods::ManagedHandler;
use crate::routing::methods::RouteMethods;

use crate::routing::router::Router;

/// 约定式资源操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// 列出集合路径下的资源。
    List,
    /// 按 ID 获取单个资源。
    Get,
    /// 创建资源。
    Create,
    /// 替换或部分更新资源。
    Update,
    /// 删除资源。
    Delete,
}

/// 收集资源控制器实际实现的处理器。
///
/// `only` 和 `except` 用于筛选已注册处理器，结果不受注册顺序影响。
#[derive(Default)]
pub struct ResourceActions {
    list: Option<RouteMethods>,
    get: Option<RouteMethods>,
    create: Option<RouteMethods>,
    update: Option<RouteMethods>,
    delete: Option<RouteMethods>,
    only: Option<Vec<Action>>,
    excluded: Vec<Action>,
}

impl ResourceActions {
    /// 以不包含任何操作的状态开始。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册 `GET {path}`。
    #[must_use]
    pub fn list<H, T>(mut self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.list = Some(routing::get(handler));
        self
    }

    /// 注册 `GET {path}/{id}`。
    #[must_use]
    pub fn get<H, T>(mut self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.get = Some(routing::get(handler));
        self
    }

    /// 注册 `POST {path}`。
    #[must_use]
    pub fn create<H, T>(mut self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.create = Some(routing::post(handler));
        self
    }

    /// 使用同一处理器注册 `PUT` 和 `PATCH {path}/{id}`。
    #[must_use]
    pub fn update<H, T>(mut self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.update = Some(routing::put(handler.clone()).patch(handler));
        self
    }

    /// 注册 `DELETE {path}/{id}`。
    #[must_use]
    pub fn delete<H, T>(mut self, handler: H) -> Self
    where
        H: ManagedHandler<T, ApplicationState>,
        T: 'static,
    {
        self.delete = Some(routing::delete(handler));
        self
    }

    /// 只保留列出的已注册操作；重复调用时取列表交集。
    #[must_use]
    pub fn only(mut self, actions: &[Action]) -> Self {
        self.only = Some(match self.only {
            Some(previous) => previous
                .into_iter()
                .filter(|action| actions.contains(action))
                .collect(),
            None => actions.to_vec(),
        });
        self
    }

    /// 移除列出的已注册操作。
    #[must_use]
    pub fn except(mut self, actions: &[Action]) -> Self {
        self.excluded.extend_from_slice(actions);
        self
    }

    fn apply_filters(&mut self) {
        let keep = |action| {
            self.only.as_ref().is_none_or(|only| only.contains(&action))
                && !self.excluded.contains(&action)
        };
        if !keep(Action::List) {
            self.list = None;
        }
        if !keep(Action::Get) {
            self.get = None;
        }
        if !keep(Action::Create) {
            self.create = None;
        }
        if !keep(Action::Update) {
            self.update = None;
        }
        if !keep(Action::Delete) {
            self.delete = None;
        }
    }

    pub(crate) fn mount(mut self, mut routes: Router, path: &str) -> Router {
        self.apply_filters();
        let base = path.trim_end_matches('/');
        let collection_path = if base.is_empty() { "/" } else { base };
        let collection = [self.list, self.create]
            .into_iter()
            .flatten()
            .reduce(RouteMethods::merge);
        let member = [self.get, self.update, self.delete]
            .into_iter()
            .flatten()
            .reduce(RouteMethods::merge);
        if let Some(router) = collection {
            routes = routes.route(collection_path, router);
        }
        if let Some(router) = member {
            let member_path = format!("{base}/{{id}}");
            routes = routes.route(&member_path, router);
        }
        routes
    }
}

/// 显式注册其可用资源操作的控制器。
///
/// ```
/// use openrust::routing::{Resource, ResourceActions, Router};
///
/// struct Posts;
/// async fn list() -> &'static str { "posts" }
///
/// impl Resource for Posts {
///     fn actions(actions: ResourceActions) -> ResourceActions {
///         actions.list(list)
///     }
/// }
///
/// let routes = Router::new().resource::<Posts>("/posts");
/// ```
pub trait Resource {
    /// 将已实现的操作添加到提供的构建器中。
    fn actions(actions: ResourceActions) -> ResourceActions;
}
