use super::Model;

/// A collection of hydrated models.
pub struct Collection<M: Model> {
    _models: Vec<M>,
}
