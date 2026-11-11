use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
};

use crate::{
    container::{Container, ParameterValue},
    routing::binding::RouteModels,
    support::facades::current_container,
};

/// Exact, server-set request attributes. Clones share the same attribute bag.
#[derive(Clone, Default)]
pub struct RequestAttributes(Arc<Mutex<HashMap<String, ParameterValue>>>);

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
    attributes: RequestAttributes,
    route: Mutex<RouteValues>,
}

tokio::task_local! {
    static CURRENT: RequestContext;
}

pub(crate) async fn scope<F: Future>(
    container: Arc<Container>,
    attributes: RequestAttributes,
    future: F,
) -> F::Output {
    CURRENT
        .scope(
            RequestContext {
                container,
                attributes,
                route: Mutex::default(),
            },
            future,
        )
        .await
}

fn read(
    container: &Container,
    get: impl FnOnce(&RequestContext) -> Option<ParameterValue>,
) -> Option<ParameterValue> {
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
    read(container, |context| context.attributes.get(name))
}

pub(crate) fn route_parameter(container: &Container, name: &str) -> Option<ParameterValue> {
    read(container, |context| {
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
