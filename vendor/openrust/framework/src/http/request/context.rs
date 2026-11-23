use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use crate::{
    container::{Container, ParameterValue},
    routing::binding::RouteModels,
    support::facades::current_container,
};

use super::Request;

/// Exact, server-set request attributes. Clones share the same attribute bag.
#[derive(Clone, Default)]
pub struct RequestAttributes(Arc<Mutex<HashMap<String, ParameterValue>>>);

pub(crate) struct RequestIdentity(Weak<()>);

impl RequestIdentity {
    pub(crate) fn same_scope(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
    pub(crate) fn matches(&self, request: &Request) -> bool {
        with_context(|context| {
            Arc::ptr_eq(
                &context
                    .metadata
                    .lock()
                    .expect("请求元数据锁已损坏")
                    .attributes
                    .0,
                &request.attributes().0,
            ) && context
                .identity
                .get()
                .is_some_and(|identity| self.0.as_ptr() == Arc::as_ptr(identity))
        })
        .unwrap_or(false)
    }
}

impl RequestAttributes {
    pub(crate) fn snapshot(&self) -> HashMap<String, ParameterValue> {
        self.0
            .lock()
            .expect("request attributes lock poisoned")
            .clone()
    }

    pub fn get(&self, name: &str) -> Option<ParameterValue> {
        self.0
            .lock()
            .expect("request attributes lock poisoned")
            .get(name)
            .cloned()
    }

    pub fn insert(&self, name: impl Into<String>, value: ParameterValue) -> Option<ParameterValue> {
        let name = name.into();
        self.0
            .lock()
            .expect("request attributes lock poisoned")
            .insert(name, value)
    }

    pub fn remove(&self, name: &str) -> Option<ParameterValue> {
        self.0
            .lock()
            .expect("request attributes lock poisoned")
            .remove(name)
    }
}

pub(crate) type RouteParameters = Arc<[(String, String)]>;

#[derive(Default)]
struct RouteValues {
    parameters: RouteParameters,
    models: RouteModels,
}

struct Metadata {
    uri: crate::http::Uri,
    host: Option<::http::HeaderValue>,
    base_url: String,
    client_ips: Vec<std::net::IpAddr>,
    attributes: RequestAttributes,
}
struct RequestContext {
    container: Arc<Container>,
    metadata: Mutex<Metadata>,
    identity: OnceLock<Arc<()>>,
    session_cache: Mutex<Option<crate::session::CacheAttributes>>,
    route: Mutex<RouteValues>,
}

tokio::task_local! {
    static CURRENT: RequestContext;
    static RESOLVING_REQUEST: Arc<tokio::sync::Mutex<Request>>;
}

/// Keeps the request available to asynchronous dependency resolution and restores
/// the original slot even if the resolution future is cancelled.
pub(super) async fn resolving<T>(request: &mut Request, future: impl Future<Output = T>) -> T {
    struct Restore<'a> {
        destination: &'a mut Request,
        source: Arc<tokio::sync::Mutex<Request>>,
    }

    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let mut request = self
                .source
                .try_lock()
                .expect("request lease escaped resolution");
            refresh_metadata(&request);
            std::mem::swap(self.destination, &mut request);
        }
    }

    let original = std::mem::replace(
        request,
        Request::from_parts(
            ::http::Request::new(()).into_parts().0,
            crate::http::Body::empty(),
        ),
    );
    let slot = Arc::new(tokio::sync::Mutex::new(original));
    let restore = Restore {
        destination: request,
        source: slot.clone(),
    };
    let value = RESOLVING_REQUEST.scope(slot, future).await;
    drop(restore);
    value
}

// 请求借用只在所属任务和应用中有效；不把活动请求存入全局容器。
pub(crate) fn current_request() -> Option<Arc<tokio::sync::Mutex<Request>>> {
    with_context(|_| RESOLVING_REQUEST.try_with(Arc::clone).ok()).flatten()
}

pub(crate) fn borrow_for_resolution(
    container: &Container,
) -> Option<tokio::sync::OwnedMutexGuard<Request>> {
    read_context(container, |context| {
        let lease = RESOLVING_REQUEST
            .try_with(Arc::clone)
            .ok()?
            .try_lock_owned()
            .ok()?;
        Arc::ptr_eq(
            &context
                .metadata
                .lock()
                .expect("请求元数据锁已损坏")
                .attributes
                .0,
            &lease.attributes().0,
        )
        .then_some(lease)
    })
}

pub(crate) fn identity(container: &Container) -> Option<RequestIdentity> {
    read_context(container, |context| {
        Some(RequestIdentity(Arc::downgrade(
            context.identity.get_or_init(|| Arc::new(())),
        )))
    })
}

pub(crate) async fn scope<F: Future>(
    container: Arc<Container>,
    uri: crate::http::Uri,
    host: Option<::http::HeaderValue>,
    base_url: String,
    attributes: RequestAttributes,
    client_ips: Vec<std::net::IpAddr>,
    future: F,
) -> F::Output {
    CURRENT
        .scope(
            RequestContext {
                container,
                metadata: Mutex::new(Metadata {
                    uri,
                    host,
                    base_url,
                    client_ips,
                    attributes,
                }),
                identity: OnceLock::new(),
                session_cache: Mutex::default(),
                route: Mutex::default(),
            },
            future,
        )
        .await
}

pub(crate) fn with_client_ips<T>(get: impl FnOnce(&[std::net::IpAddr]) -> T) -> Option<T> {
    with_metadata(|metadata| get(&metadata.client_ips))
}

pub(crate) fn with_uri<T>(get: impl FnOnce(&crate::http::Uri) -> T) -> Option<T> {
    if let Some(request) = current_request().and_then(|request| request.try_lock_owned().ok()) {
        return Some(get(request.uri()));
    }
    with_metadata(|metadata| get(&metadata.uri))
}

pub(crate) fn with_attributes<T>(get: impl FnOnce(&RequestAttributes) -> T) -> Option<T> {
    if let Some(request) = current_request().and_then(|request| request.try_lock_owned().ok()) {
        return Some(get(request.attributes()));
    }
    with_metadata(|metadata| get(&metadata.attributes))
}

pub(crate) fn with_url_uri() -> Option<crate::http::Uri> {
    if let Some(request) = current_request().and_then(|request| request.try_lock_owned().ok()) {
        return Some(crate::routing::url_uri(
            request.uri(),
            request.headers().get(http::header::HOST),
        ));
    }
    with_metadata(|metadata| crate::routing::url_uri(&metadata.uri, metadata.host.as_ref()))
}

pub(crate) fn base_url() -> Option<String> {
    if let Some(request) = current_request().and_then(|request| request.try_lock_owned().ok()) {
        return Some(request.get_base_url().to_owned());
    }
    with_metadata(|metadata| metadata.base_url.clone())
}

fn with_metadata<T>(get: impl FnOnce(&Metadata) -> T) -> Option<T> {
    with_context(|context| get(&context.metadata.lock().expect("请求元数据锁已损坏")))
}

/// 请求替换及一次独占借用结束后，同步现有消费者使用的快照。
pub(crate) fn refresh_metadata(request: &Request) {
    let retired = with_context(|context| {
        let previous = std::mem::replace(
            &mut *context.metadata.lock().expect("请求元数据锁已损坏"),
            Metadata {
                uri: request.uri().clone(),
                host: request.headers().get(http::header::HOST).cloned(),
                base_url: request.get_base_url().to_owned(),
                client_ips: request.ips().to_vec(),
                attributes: request.attributes().clone(),
            },
        );
        let values = if let Some(route) = request.custom_resolved_route() {
            RouteValues {
                parameters: route
                    .parameters
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<Vec<_>>()
                    .into(),
                models: route
                    .models
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
            }
        } else {
            RouteValues {
                parameters: request.route_parameters.clone(),
                models: request.route_models.clone(),
            }
        };
        let route = std::mem::replace(
            &mut *context.route.lock().expect("request route lock poisoned"),
            values,
        );
        *context
            .session_cache
            .lock()
            .expect("request session cache poisoned") = request
            .session()
            .ok()
            .map(|session| session.cache_attributes().clone());
        (previous, route)
    });
    drop(retired);
}

fn with_context<T>(get: impl FnOnce(&RequestContext) -> T) -> Option<T> {
    CURRENT
        .try_with(|context| {
            current_container()
                .filter(|active| Arc::ptr_eq(active, &context.container))
                .map(|_| get(context))
        })
        .ok()
        .flatten()
}

fn read_context<T>(
    container: &Container,
    get: impl FnOnce(&RequestContext) -> Option<T>,
) -> Option<T> {
    CURRENT
        .try_with(|context| {
            if std::ptr::eq(container, context.container.as_ref())
                && current_container()
                    .is_some_and(|active| Arc::ptr_eq(&active, &context.container))
            {
                get(context)
            } else {
                None
            }
        })
        .ok()
        .flatten()
}

pub(crate) fn attribute(container: &Container, name: &str) -> Option<ParameterValue> {
    read_context(container, |context| {
        context
            .metadata
            .lock()
            .expect("请求元数据锁已损坏")
            .attributes
            .get(name)
    })
}

pub(crate) fn set_session_cache(request: &Request, cache: Option<crate::session::CacheAttributes>) {
    with_context(|context| {
        if Arc::ptr_eq(
            &context
                .metadata
                .lock()
                .expect("请求元数据锁已损坏")
                .attributes
                .0,
            &request.attributes().0,
        ) {
            *context
                .session_cache
                .lock()
                .expect("request session cache poisoned") = cache;
        }
    });
}

pub(crate) fn session_cache(container: &Container) -> Option<crate::session::CacheAttributes> {
    read_context(container, |context| {
        context
            .session_cache
            .lock()
            .expect("request session cache poisoned")
            .clone()
    })
}

pub(crate) fn route_parameter(container: &Container, name: &str) -> Option<ParameterValue> {
    if let Some(request) = borrow_for_resolution(container)
        && let Some(route) = request.custom_resolved_route()
    {
        return route.models.get(name).cloned().or_else(|| {
            route
                .parameters
                .get(name)
                .map(|value| ParameterValue::new(value.clone()))
        });
    }
    read_context(container, |context| {
        let route = context.route.lock().expect("request route lock poisoned");
        route.models.get(name).cloned().or_else(|| {
            route
                .parameters
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| ParameterValue::new(value.clone()))
        })
    })
}

fn update(attributes: &RequestAttributes, set: impl FnOnce(&mut RouteValues)) {
    let _ = CURRENT.try_with(|context| {
        if Arc::ptr_eq(
            &context
                .metadata
                .lock()
                .expect("请求元数据锁已损坏")
                .attributes
                .0,
            &attributes.0,
        ) {
            set(&mut context.route.lock().expect("request route lock poisoned"));
        }
    });
}

pub(crate) fn set_parameters(attributes: &RequestAttributes, parameters: &RouteParameters) {
    let mut retired = None;
    update(attributes, |route| {
        retired = Some(std::mem::replace(
            route,
            RouteValues {
                parameters: parameters.clone(),
                models: RouteModels::new(),
            },
        ));
    });
}

pub(crate) fn set_model(attributes: &RequestAttributes, name: &str, model: &ParameterValue) {
    let mut retired = None;
    update(attributes, |route| {
        retired = route.models.insert(name.to_owned(), model.clone());
    });
}
