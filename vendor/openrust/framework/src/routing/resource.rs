use std::{any::type_name, collections::HashMap, panic::Location, sync::Arc};

use crate::{
    contracts::{routing::Handler, support::Responsable},
    foundation::http::FormRequest,
    http::Method,
    support::Str,
};

use super::{
    ControllerDispatcher, Route, Router, RoutingError,
    controller_dispatcher::{Action, FormAction},
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
    const DEFAULTS: [Self; 7] = [
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
            return &ResourceAction::DEFAULTS;
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
    singleton_lifecycle: SingletonLifecycle,
    configure: Option<Box<Configure>>,
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
            singleton_lifecycle: SingletonLifecycle::default(),
            configure: None,
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

impl Router {
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
            if let Some(configure) = &controller.configure {
                configure(action, &mut route)?;
            }
            prepared.push((action, route));
        }
        for (action, route) in prepared {
            self.insert_route(&action.methods(), route);
        }
        Ok(())
    }
}
