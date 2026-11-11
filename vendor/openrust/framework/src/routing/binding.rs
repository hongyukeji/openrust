use std::{any::Any, collections::HashMap, sync::Arc};

use crate::{
    container::ParameterValue,
    contracts::routing::{Handler, UrlRoutable},
    http::{HttpError, HttpFuture, RequestParts},
};

pub(crate) type RouteModels = HashMap<String, ParameterValue>;
pub(super) type Resolver = Arc<
    dyn for<'a> Fn(BindingContext<'a>) -> HttpFuture<'a, Result<Option<ParameterValue>, HttpError>>
        + Send
        + Sync,
>;

/// The request metadata and lookup key available to a model resolver.
pub struct BindingContext<'a> {
    pub(crate) request: &'a RequestParts,
    pub(crate) value: &'a str,
    pub(crate) field: Option<&'a str>,
    pub(crate) parent: Option<&'a str>,
    pub(crate) with_trashed: bool,
    pub(crate) models: &'a RouteModels,
}

impl<'a> BindingContext<'a> {
    pub fn request(&self) -> &'a RequestParts {
        self.request
    }

    pub fn value(&self) -> &'a str {
        self.value
    }

    pub fn field(&self) -> Option<&'a str> {
        self.field
    }

    #[allow(non_snake_case, reason = "Matches Laravel's route binding API")]
    pub fn allowsTrashedBindings(&self) -> bool {
        self.with_trashed
    }

    pub fn parent<T: Any>(&self) -> Option<&'a T> {
        self.models.get(self.parent?)?.downcast_ref()
    }
}

pub(crate) struct Binding {
    pub(crate) name: String,
    pub(crate) field: Option<String>,
    pub(crate) parent: Option<String>,
    pub(crate) with_trashed: bool,
    pub(crate) resolver: Resolver,
}

#[derive(Default)]
pub(crate) struct BindingPlan {
    pub(crate) bindings: Vec<Binding>,
    pub(crate) missing: Option<Arc<dyn Handler>>,
}

pub(super) fn resolver<T, F>(resolve: F) -> Resolver
where
    T: Any + Send + Sync,
    F: for<'a> Fn(BindingContext<'a>) -> HttpFuture<'a, Result<Option<T>, HttpError>>
        + Send
        + Sync
        + 'static,
{
    Arc::new(move |binding| {
        let lookup = resolve(binding);
        Box::pin(async move { Ok(lookup.await?.map(ParameterValue::new)) })
    })
}

pub(super) fn model<T: UrlRoutable>() -> Resolver {
    resolver(|mut binding| {
        binding.field = binding.field.or_else(|| Some(T::get_route_key_name()));
        if binding.parent.is_some() {
            T::resolve_child_route_binding(binding)
        } else {
            T::resolve_route_binding(binding)
        }
    })
}
