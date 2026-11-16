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
    pub(crate) fn matches(&self, request: &Request) -> bool {
        with_context(|context| {
            Arc::ptr_eq(&context.attributes.0, &request.attributes().0)
                && context
                    .identity
                    .get()
                    .is_some_and(|identity| self.0.as_ptr() == Arc::as_ptr(identity))
        })
        .unwrap_or(false)
    }
}

impl RequestAttributes {
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

struct RequestContext {
    container: Arc<Container>,
    uri: crate::http::Uri,
    host: Option<::http::HeaderValue>,
    attributes: RequestAttributes,
    identity: OnceLock<Arc<()>>,
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

pub(crate) fn borrow_for_resolution(
    container: &Container,
) -> Option<tokio::sync::OwnedMutexGuard<Request>> {
    read_context(container, |context| {
        let lease = RESOLVING_REQUEST
            .try_with(Arc::clone)
            .ok()?
            .try_lock_owned()
            .ok()?;
        Arc::ptr_eq(&context.attributes.0, &lease.attributes().0).then_some(lease)
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
    attributes: RequestAttributes,
    future: F,
) -> F::Output {
    CURRENT
        .scope(
            RequestContext {
                container,
                uri,
                host,
                attributes,
                identity: OnceLock::new(),
                route: Mutex::default(),
            },
            future,
        )
        .await
}

pub(crate) fn with_uri<T>(get: impl FnOnce(&crate::http::Uri) -> T) -> Option<T> {
    with_context(|context| get(&context.uri))
}

pub(crate) fn with_attributes<T>(get: impl FnOnce(&RequestAttributes) -> T) -> Option<T> {
    with_context(|context| get(&context.attributes))
}

pub(crate) fn with_url_uri() -> Option<crate::http::Uri> {
    with_context(|context| crate::routing::url_uri(&context.uri, context.host.as_ref()))
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
    read_context(container, |context| context.attributes.get(name))
}

pub(crate) fn route_parameter(container: &Container, name: &str) -> Option<ParameterValue> {
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
        if Arc::ptr_eq(&context.attributes.0, &attributes.0) {
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
