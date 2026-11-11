use crate::database::{Value, eloquent::Attributes};

/// Attributes for one intermediate-table row.
pub struct Pivot {
    attributes: Attributes,
}

#[allow(non_snake_case, reason = "Matches Laravel's model attribute API")]
impl Pivot {
    pub(super) fn new(attributes: Attributes) -> Self {
        Self { attributes }
    }

    pub fn getAttribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name)
    }

    pub fn getAttributes(&self) -> &Attributes {
        &self.attributes
    }
}
