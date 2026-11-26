use std::{any::type_name, collections::HashMap, panic::Location, sync::Arc};

use crate::{
    contracts::{routing::Handler, support::Responsable},
    foundation::http::FormRequest,
    http::Method,
    support::Str,
};

use super::{
    ControllerDispatcher, ControllerMiddleware, HasMiddleware, Route, Router, RoutingError,
    controller_dispatcher::{Action, FormAction},
    controller_middleware::for_action,
};

/// An action in a framework resource controller.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResourceAction {
    Index,
    Create,
    Store,
    Show,
    Edit,
    Update,
    Destroy,
}

impl ResourceAction {
    const DEFAULTS: &[Self] = &[
        Self::Index,
        Self::Create,
        Self::Store,
        Self::Show,
        Self::Edit,
        Self::Update,
        Self::Destroy,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Create => "create",
            Self::Store => "store",
            Self::Show => "show",
            Self::Edit => "edit",
            Self::Update => "update",
            Self::Destroy => "destroy",
        }
    }

    fn member(self) -> bool {
        matches!(self, Self::Show | Self::Edit | Self::Update | Self::Destroy)
    }

    fn methods(self) -> Vec<Method> {
        match self {
            Self::Store => vec![Method::POST],
            Self::Update => vec![Method::PUT, Method::PATCH],
            Self::Destroy => vec![Method::DELETE],
            _ => vec![Method::GET, Method::HEAD],
        }
    }
}

type Configure = dyn Fn(ResourceAction, &mut Route) -> Result<(), RoutingError>;

#[derive(Default)]
struct ResourceMiddleware {
    common: Vec<String>,
    actions: HashMap<ResourceAction, Vec<String>>,
    excluded: Vec<String>,
    excluded_actions: HashMap<ResourceAction, Vec<String>>,
}

impl ResourceMiddleware {
    fn apply(&self, action: ResourceAction, route: &mut Route) {
        let included = self.actions.get(&action).unwrap_or(&self.common);
        for name in included {
            route.middleware_name(name);
        }
        for name in self
            .excluded
            .iter()
            .chain(self.excluded_actions.get(&action).into_iter().flatten())
        {
            route.without_middleware(name);
        }
    }
}

fn extend_unique(target: &mut Vec<String>, names: &[String]) {
    for name in names {
        if !target.contains(name) {
            target.push(name.clone());
        }
    }
}

#[derive(Clone, Copy, Default)]
enum SingletonLifecycle {
    #[default]
    Existing,
    Destroyable,
    Creatable,
}

#[derive(Clone, Copy)]
pub(crate) enum ResourceKind {
    Resource,
    ApiResource,
    Singleton,
    ApiSingleton,
}

impl ResourceKind {
    fn singleton(self) -> bool {
        matches!(self, Self::Singleton | Self::ApiSingleton)
    }

    fn actions(self, lifecycle: SingletonLifecycle) -> &'static [ResourceAction] {
        use ResourceAction::{Create, Destroy, Edit, Show, Store, Update};
        if !self.singleton() {
            return ResourceAction::DEFAULTS;
        }
        match lifecycle {
            SingletonLifecycle::Existing => &[Show, Edit, Update],
            SingletonLifecycle::Destroyable => &[Show, Edit, Update, Destroy],
            SingletonLifecycle::Creatable => &[Show, Edit, Update, Create, Store, Destroy],
        }
    }

    fn uses_shallow(self, action: ResourceAction) -> bool {
        action.member() && !(self.singleton() && action == ResourceAction::Show)
    }
}

/// Typed controller actions and options for registering a named resource.
pub struct ResourceController<C> {
    actions: HashMap<ResourceAction, Arc<dyn Handler>>,
    controller: std::marker::PhantomData<fn() -> C>,
    only: Option<Vec<ResourceAction>>,
    except: Vec<ResourceAction>,
    names: HashMap<ResourceAction, String>,
    parameters: HashMap<String, String>,
    bindings: HashMap<String, String>,
    shallow: bool,
    trashed: Vec<ResourceAction>,
    missing: Option<Arc<dyn Handler>>,
    singleton_lifecycle: SingletonLifecycle,
    middleware: ResourceMiddleware,
    configure: Option<Box<Configure>>,
    controller_middleware: Option<fn() -> Vec<ControllerMiddleware>>,
}

impl<C> Default for ResourceController<C> {
    fn default() -> Self {
        Self {
            actions: HashMap::new(),
            controller: std::marker::PhantomData,
            only: None,
            except: Vec::new(),
            names: HashMap::new(),
            parameters: HashMap::new(),
            bindings: HashMap::new(),
            shallow: false,
            trashed: Vec::new(),
            missing: None,
            singleton_lifecycle: SingletonLifecycle::default(),
            middleware: ResourceMiddleware::default(),
            configure: None,
            controller_middleware: None,
        }
    }
}

impl<C> ResourceController<C> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn action(
        mut self,
        action: ResourceAction,
        handler: Action<C, impl Responsable + 'static>,
    ) -> Self
    where
        C: Send + Sync + 'static,
    {
        self.actions
            .insert(action, Arc::new(ControllerDispatcher::new(handler)));
        self
    }

    pub fn action_with_form_request<F: FormRequest + Default + 'static>(
        mut self,
        action: ResourceAction,
        handler: FormAction<C, F, impl Responsable + 'static>,
    ) -> Self
    where
        C: Send + Sync + 'static,
    {
        self.actions.insert(
            action,
            Arc::new(ControllerDispatcher::with_form_request(handler)),
        );
        self
    }

    pub fn only(mut self, actions: &[ResourceAction]) -> Self {
        self.only = Some(actions.to_vec());
        self
    }

    pub fn except(mut self, actions: &[ResourceAction]) -> Self {
        self.except = actions.to_vec();
        self
    }

    pub fn names(mut self, names: &[(ResourceAction, &str)]) -> Self {
        self.names.extend(
            names
                .iter()
                .map(|(action, name)| (*action, (*name).to_owned())),
        );
        self
    }

    pub fn parameters(mut self, parameters: &[(&str, &str)]) -> Self {
        self.parameters.extend(
            parameters
                .iter()
                .map(|(resource, parameter)| ((*resource).to_owned(), (*parameter).to_owned())),
        );
        self
    }

    pub fn scoped(mut self, bindings: &[(&str, &str)]) -> Self {
        self.bindings.extend(
            bindings
                .iter()
                .map(|(parameter, field)| ((*parameter).to_owned(), (*field).to_owned())),
        );
        self
    }

    pub fn shallow(mut self) -> Self {
        self.shallow = true;
        self
    }

    /// 为常规和 API 资源的成员动作设置模型找不到时的处理器。
    pub fn missing(mut self, handler: impl Handler + 'static) -> Self {
        self.missing = Some(Arc::new(handler));
        self
    }

    /// 允许常规和 API 资源的 show、edit 和 update 绑定软删除模型。
    pub fn with_trashed(self) -> Self {
        self.with_trashed_for(&[])
    }

    /// 只允许指定动作绑定软删除模型。空切片采用默认动作。
    pub fn with_trashed_for(mut self, actions: &[ResourceAction]) -> Self {
        use ResourceAction::{Edit, Show, Update};

        self.trashed = if actions.is_empty() {
            vec![Show, Edit, Update]
        } else {
            actions.to_vec()
        };
        self
    }

    /// Enables creation, storage, and deletion for a singleton.
    pub fn creatable(mut self) -> Self {
        self.singleton_lifecycle = SingletonLifecycle::Creatable;
        self
    }

    /// Enables deletion for a singleton.
    pub fn destroyable(mut self) -> Self {
        if matches!(self.singleton_lifecycle, SingletonLifecycle::Existing) {
            self.singleton_lifecycle = SingletonLifecycle::Destroyable;
        }
        self
    }

    /// 为该资源的所有动作设置路由中间件。
    pub fn middleware(mut self, middleware: &[&str]) -> Self {
        let names = middleware
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>();
        for selected in self.middleware.actions.values_mut() {
            extend_unique(selected, &names);
        }
        self.middleware.common = names;
        self
    }

    /// 为指定动作设置额外中间件。再次设置同一动作会替换该动作的设置。
    pub fn middleware_for(mut self, actions: &[ResourceAction], middleware: &[&str]) -> Self {
        let mut selected = self.middleware.common.clone();
        let names = middleware
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>();
        extend_unique(&mut selected, &names);
        for &action in actions {
            self.middleware.actions.insert(action, selected.clone());
        }
        self
    }

    /// 从该资源的所有动作移除所列路由中间件。
    pub fn without_middleware(mut self, middleware: &[&str]) -> Self {
        self.middleware
            .excluded
            .extend(middleware.iter().map(|name| (*name).to_owned()));
        self
    }

    /// 从指定动作移除所列路由中间件。
    pub fn without_middleware_for(
        mut self,
        actions: &[ResourceAction],
        middleware: &[&str],
    ) -> Self {
        let names: Vec<_> = middleware.iter().map(|name| (*name).to_owned()).collect();
        for &action in actions {
            self.middleware
                .excluded_actions
                .insert(action, names.clone());
        }
        self
    }

    /// 为该资源的各个动作加载控制器的静态中间件声明。
    pub fn with_controller_middleware(mut self) -> Self
    where
        C: HasMiddleware,
    {
        self.controller_middleware = Some(C::middleware);
        self
    }

    /// Configures each prepared route before it is registered.
    pub fn configure(
        mut self,
        configure: impl Fn(ResourceAction, &mut Route) -> Result<(), RoutingError> + 'static,
    ) -> Self {
        self.configure = Some(Box::new(configure));
        self
    }

    fn wildcard(&self, segment: &str) -> String {
        let name = self
            .parameters
            .get(segment)
            .cloned()
            .unwrap_or_else(|| Str::singular(segment))
            .replace('-', "_");
        match self.bindings.get(&name) {
            Some(field) => format!("{{{name}:{field}}}"),
            None => format!("{{{name}}}"),
        }
    }
}

type RegisterResource = dyn FnOnce(
    &mut Router,
    ResourceKind,
    bool,
    &'static Location<'static>,
) -> Result<(), RoutingError>;

/// 一个可与不同控制器一起批量登记的资源。
pub struct ResourceRegistration(Box<RegisterResource>);

impl ResourceRegistration {
    pub fn new<C: Send + Sync + 'static>(
        name: impl Into<String>,
        controller: ResourceController<C>,
    ) -> Self {
        let name = name.into();
        Self(Box::new(move |router, kind, allow_trashed, definition| {
            let controller = if allow_trashed {
                controller.with_trashed()
            } else {
                controller
            };
            router.register_resource(&name, controller, kind, definition)
        }))
    }
}

impl Router {
    #[track_caller]
    pub fn resources(
        &mut self,
        resources: impl IntoIterator<Item = ResourceRegistration>,
    ) -> Result<(), RoutingError> {
        self.register_resources(resources, ResourceKind::Resource, false, Location::caller())
    }

    #[track_caller]
    pub fn api_resources(
        &mut self,
        resources: impl IntoIterator<Item = ResourceRegistration>,
    ) -> Result<(), RoutingError> {
        self.register_resources(
            resources,
            ResourceKind::ApiResource,
            false,
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn soft_deletable_resources(
        &mut self,
        resources: impl IntoIterator<Item = ResourceRegistration>,
    ) -> Result<(), RoutingError> {
        self.register_resources(resources, ResourceKind::Resource, true, Location::caller())
    }

    pub(crate) fn register_resources(
        &mut self,
        resources: impl IntoIterator<Item = ResourceRegistration>,
        kind: ResourceKind,
        allow_trashed: bool,
        definition: &'static Location<'static>,
    ) -> Result<(), RoutingError> {
        for ResourceRegistration(register) in resources {
            register(self, kind, allow_trashed, definition)?;
        }
        Ok(())
    }

    #[track_caller]
    pub fn resource<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
    ) -> Result<(), RoutingError> {
        self.register_resource(name, controller, ResourceKind::Resource, Location::caller())
    }

    #[track_caller]
    pub fn api_resource<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
    ) -> Result<(), RoutingError> {
        self.register_resource(
            name,
            controller,
            ResourceKind::ApiResource,
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn singleton<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
    ) -> Result<(), RoutingError> {
        self.register_resource(
            name,
            controller,
            ResourceKind::Singleton,
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn api_singleton<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
    ) -> Result<(), RoutingError> {
        self.register_resource(
            name,
            controller,
            ResourceKind::ApiSingleton,
            Location::caller(),
        )
    }

    pub(crate) fn register_resource<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
        kind: ResourceKind,
        definition: &'static Location<'static>,
    ) -> Result<(), RoutingError> {
        let (prefix, name) = name.rsplit_once('/').unwrap_or(("", name));
        let mut segments: Vec<_> = name.split('.').collect();
        if segments.iter().any(|segment| {
            segment.is_empty()
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }) {
            return Err(RoutingError::InvalidUri {
                uri: name.to_owned(),
            });
        }
        let leaf = segments.pop().expect("resource name always has a segment");
        let mut collection = String::new();
        for parent in segments {
            use std::fmt::Write as _;
            write!(collection, "{parent}/{}/", controller.wildcard(parent))
                .expect("writing to String");
        }
        collection.push_str(leaf);
        let member_key = (!kind.singleton()).then(|| controller.wildcard(leaf));
        let mut prepared = Vec::new();
        let default_api_filter =
            matches!(kind, ResourceKind::ApiResource | ResourceKind::ApiSingleton)
                && controller.only.is_none();
        for &action in kind.actions(controller.singleton_lifecycle) {
            if (default_api_filter
                && matches!(action, ResourceAction::Create | ResourceAction::Edit))
                || controller
                    .only
                    .as_ref()
                    .is_some_and(|actions| !actions.contains(&action))
                || controller.except.contains(&action)
            {
                continue;
            }
            let handler = controller.actions.get(&action).ok_or_else(|| {
                RoutingError::MissingResourceAction {
                    resource: name.to_owned(),
                    action: action.name(),
                }
            })?;
            let shallow = controller.shallow && kind.uses_shallow(action);
            let base = if shallow { leaf } else { &collection };
            let member = || {
                member_key
                    .as_deref()
                    .map_or_else(|| base.to_owned(), |key| format!("{base}/{key}"))
            };
            let path = match action {
                ResourceAction::Create => format!("{base}/create"),
                ResourceAction::Edit => format!("{}/edit", member()),
                action if action.member() => member(),
                _ => base.to_owned(),
            };
            let uri = format!("{prefix}/{path}");
            let mut route = self.prepare_route(&uri, Arc::clone(handler))?;
            let route_name = controller.names.get(&action).cloned().unwrap_or_else(|| {
                format!("{}.{}", if shallow { leaf } else { name }, action.name())
            });
            route.definition = definition;
            route.name(&route_name);
            route.set_action(&format!("{}@{}", type_name::<C>(), action.name()))?;
            if !kind.singleton() {
                if controller.trashed.contains(&action) {
                    route.with_trashed();
                }
                if action.member()
                    && let Some(missing) = &controller.missing
                {
                    route.set_missing_handler(Arc::clone(missing));
                }
            }
            controller.middleware.apply(action, &mut route);
            if let Some(configure) = &controller.configure {
                configure(action, &mut route)?;
            }
            if let Some(middleware) = controller.controller_middleware {
                route
                    .middleware
                    .extend(for_action(middleware(), action.name()));
            }
            prepared.push((action, route));
        }
        for (action, route) in prepared {
            self.insert_route(&action.methods(), route);
        }
        Ok(())
    }
}
