mod substitute_bindings;
mod throttle_requests;
mod throttle_requests_with_redis;

pub use substitute_bindings::SubstituteBindings;
pub use throttle_requests::ThrottleRequests;
pub use throttle_requests_with_redis::ThrottleRequestsWithRedis;

use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use crate::contracts::http::Middleware;

use super::RoutingError;

/// A concrete middleware instance or a reference to a registered group or alias.
#[derive(Clone)]
pub enum MiddlewareEntry {
    Instance(Arc<dyn Middleware>),
    Named(String),
}

impl MiddlewareEntry {
    /// Registers a concrete middleware with its post-response hook.
    pub fn terminable<M: crate::contracts::http::TerminableMiddleware>(middleware: Arc<M>) -> Self {
        Self::Instance(crate::foundation::http::terminable(middleware))
    }

    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }
}

pub(crate) type MiddlewareStack = Vec<Arc<dyn Middleware>>;

#[derive(Default)]
pub(crate) struct MiddlewareRegistry {
    pub(crate) groups: HashMap<String, Vec<MiddlewareEntry>>,
    pub(crate) aliases: HashMap<String, Arc<dyn Middleware>>,
    pub(crate) priority: Vec<TypeId>,
    pub(crate) throttle_with_redis: bool,
    throttle_aliases: Mutex<HashMap<String, Arc<dyn Middleware>>>,
}

impl MiddlewareRegistry {
    pub(crate) fn resolve_groups(&self) -> Result<HashMap<String, MiddlewareStack>, RoutingError> {
        let mut resolved = HashMap::new();
        let mut visiting = HashSet::new();
        for name in self.groups.keys() {
            self.resolve_group(name, &mut visiting, &mut resolved)?;
        }
        Ok(resolved)
    }

    fn resolve_group<'a>(
        &'a self,
        name: &'a str,
        visiting: &mut HashSet<&'a str>,
        resolved: &mut HashMap<String, MiddlewareStack>,
    ) -> Result<(), RoutingError> {
        if resolved.contains_key(name) {
            return Ok(());
        }
        if !visiting.insert(name) {
            return Err(RoutingError::MiddlewareGroupCycle {
                name: name.to_owned(),
            });
        }
        let entries = &self.groups[name];
        for entry in entries {
            if let MiddlewareEntry::Named(child) = entry
                && self.groups.contains_key(child)
            {
                self.resolve_group(child, visiting, resolved)?;
            }
        }
        resolved.insert(name.to_owned(), self.expand(entries, resolved)?);
        visiting.remove(name);
        Ok(())
    }

    fn alias(&self, name: &str) -> Option<Arc<dyn Middleware>> {
        if let Some(alias) = self.aliases.get(name) {
            return Some(alias.clone());
        }
        let limiter = name
            .strip_prefix("throttle:")
            .filter(|name| !name.is_empty())?;
        let mut aliases = self
            .throttle_aliases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(
            aliases
                .entry(limiter.into())
                .or_insert_with(|| {
                    if self.throttle_with_redis {
                        Arc::new(ThrottleRequestsWithRedis::new(limiter))
                    } else {
                        Arc::new(ThrottleRequests::new(limiter))
                    }
                })
                .clone(),
        )
    }

    fn expand(
        &self,
        entries: &[MiddlewareEntry],
        groups: &HashMap<String, MiddlewareStack>,
    ) -> Result<MiddlewareStack, RoutingError> {
        let mut stack = Vec::new();
        for entry in entries {
            match entry {
                MiddlewareEntry::Instance(instance) => stack.push(instance.clone()),
                MiddlewareEntry::Named(name) => {
                    if let Some(group) = groups.get(name) {
                        stack.extend(group.iter().cloned());
                    } else if let Some(instance) = self.alias(name) {
                        stack.push(instance);
                    } else {
                        return Err(RoutingError::UnknownMiddleware { name: name.clone() });
                    }
                }
            }
        }
        Ok(stack)
    }

    pub(crate) fn resolve_stack(
        &self,
        entries: &[MiddlewareEntry],
        excluded: &[String],
        groups: &HashMap<String, MiddlewareStack>,
    ) -> Result<MiddlewareStack, RoutingError> {
        let mut stack = self.expand(entries, groups)?;
        if !excluded.is_empty() {
            let mut instances = HashSet::new();
            for name in excluded {
                if let Some(group) = groups.get(name) {
                    instances.extend(group.iter().map(middleware_identity));
                } else if let Some(alias) = self.alias(name) {
                    instances.insert(middleware_identity(&alias));
                }
            }
            stack.retain(|item| !instances.contains(&middleware_identity(item)));
        }
        sort_by_priority(&mut stack, &self.priority);
        let mut seen = HashSet::new();
        stack.retain(|instance| seen.insert(middleware_identity(instance)));
        Ok(stack)
    }
}

fn middleware_identity(middleware: &Arc<dyn Middleware>) -> *const () {
    middleware.terminable().map_or_else(
        || Arc::as_ptr(middleware).cast(),
        |terminable| std::ptr::from_ref(terminable).cast(),
    )
}

fn sort_by_priority(stack: &mut [Arc<dyn Middleware>], priority: &[TypeId]) {
    loop {
        let mut previous = None;
        let inversion = stack.iter().enumerate().find_map(|(index, middleware)| {
            let id = middleware.middleware_type_id();
            let rank = priority.iter().position(|candidate| *candidate == id)?;
            let preceding = previous.replace((index, rank));
            preceding
                .filter(|&(_, prior_rank)| prior_rank > rank)
                .map(|(from, _)| from..=index)
        });
        let Some(inversion) = inversion else {
            return;
        };
        // Move the earlier-priority item just before the preceding ranked item.
        // Intervening unranked middleware keeps its relative order.
        stack[inversion].rotate_right(1);
    }
}
