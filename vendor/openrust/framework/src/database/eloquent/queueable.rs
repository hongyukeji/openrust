use std::{cell::RefCell, collections::HashSet};

use super::{Model, ModelState};

thread_local! {
    static VISITING: RefCell<HashSet<usize>> = RefCell::default();
}

struct RelationGuard(usize);

impl Drop for RelationGuard {
    fn drop(&mut self) {
        VISITING.with(|models| models.borrow_mut().remove(&self.0));
    }
}

pub(super) fn model_relations(state: &ModelState) -> Vec<String> {
    let identity = std::ptr::from_ref(state).addr();
    if !VISITING.with(|models| models.borrow_mut().insert(identity)) {
        return Vec::new();
    }
    let _guard = RelationGuard(identity);
    match &state.detached_autoload {
        Some(link) => link.visible_relations(state, |_| true).queueable_paths(),
        None => state.relations.queueable_paths(),
    }
}

pub(super) fn common_relations<'a, M: Model + 'a>(
    mut models: impl Iterator<Item = &'a M>,
) -> Vec<String> {
    let Some(first) = models.next() else {
        return Vec::new();
    };
    let mut paths = first.getQueueableRelations();
    for model in models {
        if paths.is_empty() {
            break;
        }
        let loaded: HashSet<_> = model.getQueueableRelations().into_iter().collect();
        paths.retain(|path| loaded.contains(path));
    }
    paths
}
