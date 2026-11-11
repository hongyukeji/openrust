use std::ops::Deref;

use super::Model;

/// Models returned by an Eloquent query.
pub struct Collection<M: Model> {
    models: Vec<M>,
}

impl<M: Model> Collection<M> {
    pub fn new(models: Vec<M>) -> Self {
        Self { models }
    }

    pub fn all(&self) -> &[M] {
        &self.models
    }
}

impl<M: Model> Deref for Collection<M> {
    type Target = [M];

    fn deref(&self) -> &Self::Target {
        self.all()
    }
}

impl<M: Model> IntoIterator for Collection<M> {
    type Item = M;
    type IntoIter = std::vec::IntoIter<M>;

    fn into_iter(self) -> Self::IntoIter {
        self.models.into_iter()
    }
}
