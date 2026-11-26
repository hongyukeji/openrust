use std::{
    panic::Location,
    rc::{Rc, Weak},
};

use crate::{
    contracts::routing::{Handler, UrlRoutable},
    routing::{
        MiddlewareEntry, ResourceAction, ResourceController, ResourceKind, Route, RouteGroup,
        RoutingError,
    },
};

use super::{ROUTER, RouteRegistration};

/// 在当前注册作用域内配置一条已注册的路由。
#[derive(Debug)]
pub struct RegisteredRoute {
    scope: Weak<RouteRegistration>,
    index: usize,
}

// 路由对象由路由器持有，每次配置只在调用期间借用路由器。
impl RegisteredRoute {
    pub(super) fn new(scope: &Rc<RouteRegistration>, index: usize) -> Self {
        Self {
            scope: Rc::downgrade(scope),
            index,
        }
    }

    fn configure(
        self,
        configure: impl FnOnce(&mut Route) -> Result<(), RoutingError>,
    ) -> Result<Self, RoutingError> {
        let scope = current_scope(&self.scope)?;
        configure(scope.borrow_router()?.registered_route_mut(self.index))?;
        Ok(self)
    }
}

// 配置经现有路由对象执行，沿用参数校验、名称前缀和中间件语义。
macro_rules! route_options {
    (
        $($method:ident($($arg:ident: $arg_type:ty),*) => $target:ident;)*
        @fallible {
            $($checked:ident($($value:ident: $value_type:ty),*);)*
        }
    ) => {
        impl RegisteredRoute {
            $(
                pub fn $method(self, $($arg: $arg_type),*) -> Result<Self, RoutingError> {
                    self.configure(|route| {
                        route.$target($($arg),*);
                        Ok(())
                    })
                }
            )*
            $(
                pub fn $checked(self, $($value: $value_type),*) -> Result<Self, RoutingError> {
                    self.configure(|route| route.$checked($($value),*).map(|_| ()))
                }
            )*
        }
    };
}

route_options! {
    name(name: &str) => name;
    middleware_name(name: impl Into<String>) => middleware_name;
    without_middleware(name: impl Into<String>) => without_middleware;
    can(ability: &str, parameters: &[&str]) => can;
    defaults(name: impl Into<String>, value: impl Into<String>) => defaults;
    block(lock_seconds: u64, wait_seconds: u64) => block;
    without_blocking() => without_blocking;
    scope_bindings() => scope_bindings;
    without_scoped_bindings() => without_scoped_bindings;
    with_trashed() => with_trashed;
    missing(handler: impl Handler + 'static) => missing;
    @fallible {
        r#where(name: &str, expression: &str);
        where_alpha(name: &str);
        where_alpha_numeric(name: &str);
        where_number(name: &str);
        where_uuid(name: &str);
        where_ulid(name: &str);
        where_in(name: &str, values: &[&str]);
    }
}

impl RegisteredRoute {
    pub fn middleware(self, middleware: impl Into<MiddlewareEntry>) -> Result<Self, RoutingError> {
        self.configure(|route| {
            match middleware.into() {
                MiddlewareEntry::Named(name) => route.middleware_name(name),
                MiddlewareEntry::Instance(middleware) => route.middleware(middleware),
            };
            Ok(())
        })
    }

    pub fn model<T: UrlRoutable>(self, name: &str) -> Result<Self, RoutingError> {
        self.configure(|route| route.model::<T>(name).map(|_| ()))
    }
}

fn current_scope(scope: &Weak<RouteRegistration>) -> Result<Rc<RouteRegistration>, RoutingError> {
    ROUTER
        .try_with(|active| {
            if Weak::ptr_eq(scope, &Rc::downgrade(active)) {
                Ok(Rc::clone(active))
            } else {
                Err(RoutingError::NoRegistrationScope)
            }
        })
        .map_err(|_| RoutingError::NoRegistrationScope)?
}

/// 在资源注册前应用配置。语句结束时自动注册，也可显式调用 `register`。
pub struct PendingResourceRegistration<C: Send + Sync + 'static> {
    scope: Weak<RouteRegistration>,
    name: String,
    controller: Option<ResourceController<C>>,
    kind: ResourceKind,
    definition: &'static Location<'static>,
}

impl<C: Send + Sync + 'static> PendingResourceRegistration<C> {
    pub(super) fn new(
        scope: &Rc<RouteRegistration>,
        name: &str,
        controller: ResourceController<C>,
        kind: ResourceKind,
        definition: &'static Location<'static>,
    ) -> Self {
        Self {
            scope: Rc::downgrade(scope),
            name: name.to_owned(),
            controller: Some(controller),
            kind,
            definition,
        }
    }

    pub fn register(mut self) -> Result<(), RoutingError> {
        let controller = self.controller.take().expect("待注册的资源控制器");
        self.register_controller(controller)
    }

    fn register_controller(&self, controller: ResourceController<C>) -> Result<(), RoutingError> {
        let scope = current_scope(&self.scope)?;
        scope
            .borrow_router()?
            .register_resource(&self.name, controller, self.kind, self.definition)
    }
}

// 资源链式配置复用同一控制器定义，最终仍由 Router 批量准备并提交路由。
macro_rules! resource_options {
    ($($method:ident($($arg:ident: $arg_type:ty),*);)*) => {
        impl<C: Send + Sync + 'static> PendingResourceRegistration<C> {
            $(
                pub fn $method(mut self, $($arg: $arg_type),*) -> Self {
                    self.controller = self.controller.take().map(|controller| controller.$method($($arg),*));
                    self
                }
            )*
        }
    };
}

resource_options! {
    only(actions: &[ResourceAction]);
    except(actions: &[ResourceAction]);
    names(names: &[(ResourceAction, &str)]);
    name(action: ResourceAction, name: impl Into<String>);
    parameters(parameters: &[(&str, &str)]);
    parameter(resource: impl Into<String>, parameter: impl Into<String>);
    scoped(bindings: &[(&str, &str)]);
    shallow();
    missing(handler: impl Handler + 'static);
    with_trashed();
    with_trashed_for(actions: &[ResourceAction]);
    middleware(middleware: &[&str]);
    middleware_for(actions: &[ResourceAction], middleware: &[&str]);
    without_middleware(middleware: &[&str]);
    without_middleware_for(actions: &[ResourceAction], middleware: &[&str]);
    creatable();
    destroyable();
}

impl<C: Send + Sync + 'static> Drop for PendingResourceRegistration<C> {
    fn drop(&mut self) {
        if let Some(controller) = self.controller.take()
            && !std::thread::panicking()
            && let Err(error) = self.register_controller(controller)
            && let Some(scope) = self.scope.upgrade()
        {
            scope.pending_error.borrow_mut().get_or_insert(error);
        }
    }
}

/// 路由组退出时恢复路由器的组栈，即使回调提前返回。
struct GroupScope<'a>(&'a RouteRegistration);

impl Drop for GroupScope<'_> {
    fn drop(&mut self) {
        self.0.router.borrow_mut().pop_group();
    }
}

pub(crate) fn register_group(
    group: RouteGroup,
    register: impl FnOnce() -> Result<(), RoutingError>,
) -> Result<(), RoutingError> {
    ROUTER
        .try_with(|scope| {
            scope.borrow_router()?.push_group(group);
            let _group_scope = GroupScope(scope);
            register()
        })
        .map_err(|_| RoutingError::NoRegistrationScope)?
}
