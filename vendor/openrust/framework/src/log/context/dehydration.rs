use std::sync::{Arc, RwLock};

use crate::log::LogContext;

use super::Repository;

/// The public and hidden context transferred to a queued job.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DehydratedContext {
    public: LogContext,
    hidden: LogContext,
}

impl DehydratedContext {
    pub(crate) fn from_repository(context: &Repository) -> Self {
        Self {
            public: context.all(),
            hidden: context.allHidden(),
        }
    }

    pub(crate) fn fill(&self, context: &Repository) {
        context.addMany(self.public.clone());
        context.addManyHidden(self.hidden.clone());
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.public.is_empty() && self.hidden.is_empty()
    }
}

type Hook = Arc<dyn Fn(&Repository) + Send + Sync>;

/// Application-scoped callbacks around context transport.
#[derive(Default)]
pub(crate) struct Hooks {
    dehydrating: RwLock<Vec<Hook>>,
    hydrated: RwLock<Vec<Hook>>,
}

impl Hooks {
    pub(crate) fn on_dehydrating(&self, hook: Hook) {
        self.dehydrating
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(hook);
    }

    pub(crate) fn on_hydrated(&self, hook: Hook) {
        self.hydrated
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(hook);
    }

    pub(crate) fn dehydrate(&self, context: &Repository) -> DehydratedContext {
        let copy = Repository::default();
        DehydratedContext::from_repository(context).fill(&copy);
        run(&self.dehydrating, &copy);
        DehydratedContext::from_repository(&copy)
    }

    pub(crate) fn hydrate(&self, data: &DehydratedContext, context: &Repository) {
        data.fill(context);
        run(&self.hydrated, context);
    }
}

fn run(hooks: &RwLock<Vec<Hook>>, context: &Repository) {
    let hooks = hooks
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    for hook in hooks {
        hook(context);
    }
}
