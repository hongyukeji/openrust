use std::{any::type_name, collections::HashMap, panic::Location, sync::Arc};

use crate::{http::Method, support::Str};

use super::{ControllerDispatcher, Route, Router, RoutingError, controller_dispatcher::Action};

/// An action in a Laravel-style resource controller.
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

/// Typed controller actions and options for registering a named resource.
pub struct ResourceController<C> {
    actions: HashMap<ResourceAction, Action<C>>,
    only: Option<Vec<ResourceAction>>,
    except: Vec<ResourceAction>,
    names: HashMap<ResourceAction, String>,
    parameters: HashMap<String, String>,
    bindings: HashMap<String, String>,
    shallow: bool,
    configure: Option<Box<Configure>>,
}

impl<C> Default for ResourceController<C> {
    fn default() -> Self {
        Self {
            actions: HashMap::new(),
            only: None,
            except: Vec::new(),
            names: HashMap::new(),
            parameters: HashMap::new(),
            bindings: HashMap::new(),
            shallow: false,
            configure: None,
        }
    }
}

impl<C> ResourceController<C> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn action(mut self, action: ResourceAction, handler: Action<C>) -> Self {
        self.actions.insert(action, handler);
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

#[allow(non_snake_case, reason = "Matches Laravel's resource router")]
impl Router {
    #[track_caller]
    pub fn resource<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
    ) -> Result<(), RoutingError> {
        self.register_resource(name, controller, false, Location::caller())
    }

    #[track_caller]
    pub fn apiResource<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
    ) -> Result<(), RoutingError> {
        self.register_resource(name, controller, true, Location::caller())
    }

    pub(crate) fn register_resource<C: Send + Sync + 'static>(
        &mut self,
        name: &str,
        controller: ResourceController<C>,
        api: bool,
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
        let member_key = controller.wildcard(leaf);
        let mut prepared = Vec::new();
        for action in ResourceAction::DEFAULTS {
            if (api && matches!(action, ResourceAction::Create | ResourceAction::Edit))
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
            let shallow = controller.shallow && action.member();
            let base = if shallow { leaf } else { &collection };
            let path = match action {
                ResourceAction::Create => format!("{base}/create"),
                ResourceAction::Edit => format!("{base}/{member_key}/edit"),
                action if action.member() => format!("{base}/{member_key}"),
                _ => base.to_owned(),
            };
            let uri = format!("{prefix}/{path}");
            let mut route =
                self.prepare_route(&uri, Arc::new(ControllerDispatcher::new(*handler)))?;
            let route_name = controller.names.get(&action).cloned().unwrap_or_else(|| {
                format!("{}.{}", if shallow { leaf } else { name }, action.name())
            });
            route.definition = definition;
            route.name(&route_name);
            route.setAction(&format!("{}@{}", type_name::<C>(), action.name()))?;
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
