mod substitute_bindings;

pub use substitute_bindings::SubstituteBindings;

use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    sync::Arc,
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
                    } else if let Some(instance) = self.aliases.get(name) {
                        stack.push(instance.clone());
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
        groups: &HashMap<String, MiddlewareStack>,
    ) -> Result<MiddlewareStack, RoutingError> {
        let mut stack = self.expand(entries, groups)?;
        sort_by_priority(&mut stack, &self.priority);
        let mut seen = HashSet::new();
        stack.retain(|instance| seen.insert(Arc::as_ptr(instance).cast::<()>()));
        Ok(stack)
    }
}

fn sort_by_priority(stack: &mut [Arc<dyn Middleware>], priority: &[TypeId]) {
    loop {
        let mut previous = None;
        let inversion = stack.iter().enumerate().find_map(|(index, middleware)| {
            let id = middleware.as_ref().type_id();
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
