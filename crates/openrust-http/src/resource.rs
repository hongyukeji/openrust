//! Conventional resource routes backed by explicitly registered controller actions.

use axum::handler::Handler;
use axum::routing::{self, MethodRouter};
use openrust_core::AppState;

use crate::routes::Routes;

/// A conventional resource action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// List resources at the collection path.
    Index,
    /// Show one resource by ID.
    Show,
    /// Create a resource.
    Store,
    /// Replace or partially update a resource.
    Update,
    /// Delete a resource.
    Destroy,
}

/// Collects handlers that a resource controller actually implements.
///
/// `only` and `except` filter registered handlers, independent of registration order.
#[derive(Default)]
pub struct ResourceActions {
    index: Option<MethodRouter<AppState>>,
    show: Option<MethodRouter<AppState>>,
    store: Option<MethodRouter<AppState>>,
    update: Option<MethodRouter<AppState>>,
    destroy: Option<MethodRouter<AppState>>,
    only: Option<Vec<Action>>,
    excluded: Vec<Action>,
}

impl ResourceActions {
    /// Starts with no actions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `GET {path}`.
    #[must_use]
    pub fn index<H, T>(mut self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.index = Some(routing::get(handler));
        self
    }

    /// Registers `GET {path}/{id}`.
    #[must_use]
    pub fn show<H, T>(mut self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.show = Some(routing::get(handler));
        self
    }

    /// Registers `POST {path}`.
    #[must_use]
    pub fn store<H, T>(mut self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.store = Some(routing::post(handler));
        self
    }

    /// Registers both `PUT` and `PATCH {path}/{id}` with the same handler.
    #[must_use]
    pub fn update<H, T>(mut self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.update = Some(routing::put(handler.clone()).patch(handler));
        self
    }

    /// Registers `DELETE {path}/{id}`.
    #[must_use]
    pub fn destroy<H, T>(mut self, handler: H) -> Self
    where
        H: Handler<T, AppState>,
        T: 'static,
    {
        self.destroy = Some(routing::delete(handler));
        self
    }

    /// Retains only the listed registered actions. Repeated calls intersect their lists.
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

    /// Removes the listed registered actions.
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
        if !keep(Action::Index) {
            self.index = None;
        }
        if !keep(Action::Show) {
            self.show = None;
        }
        if !keep(Action::Store) {
            self.store = None;
        }
        if !keep(Action::Update) {
            self.update = None;
        }
        if !keep(Action::Destroy) {
            self.destroy = None;
        }
    }

    pub(crate) fn mount(mut self, mut routes: Routes, path: &str) -> Routes {
        self.apply_filters();
        let base = path.trim_end_matches('/');
        let collection_path = if base.is_empty() { "/" } else { base };
        let collection = [self.index, self.store]
            .into_iter()
            .flatten()
            .reduce(MethodRouter::merge);
        let member = [self.show, self.update, self.destroy]
            .into_iter()
            .flatten()
            .reduce(MethodRouter::merge);
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

/// A controller that explicitly registers its available resource actions.
///
/// ```
/// use openrust_http::{Resource, ResourceActions, Routes};
///
/// struct Posts;
/// async fn index() -> &'static str { "posts" }
///
/// impl Resource for Posts {
///     fn actions(actions: ResourceActions) -> ResourceActions {
///         actions.index(index)
///     }
/// }
///
/// let routes = Routes::new().resource::<Posts>("/posts");
/// ```
pub trait Resource {
    /// Adds implemented actions to the supplied builder.
    fn actions(actions: ResourceActions) -> ResourceActions;
}
