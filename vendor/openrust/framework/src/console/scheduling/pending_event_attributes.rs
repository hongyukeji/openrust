use std::sync::Arc;

use super::Event;

type ConfigureEvent = dyn Fn(&mut Event) + Send + Sync;

#[derive(Clone, Default)]
pub(super) struct PendingEventAttributes {
    configure: Vec<Arc<ConfigureEvent>>,
}

impl PendingEventAttributes {
    pub(super) fn push(&mut self, configure: impl Fn(&mut Event) + Send + Sync + 'static) {
        self.configure.push(Arc::new(configure));
    }

    pub(super) fn apply(&self, event: &mut Event) {
        for configure in &self.configure {
            configure(event);
        }
    }
}
