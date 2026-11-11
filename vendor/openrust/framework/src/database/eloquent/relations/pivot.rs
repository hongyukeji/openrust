use crate::database::{
    DatabaseResult, Value,
    eloquent::{Attributes, serialization},
};

/// Attributes for one intermediate-table row.
pub struct Pivot {
    attributes: Attributes,
}

#[allow(non_snake_case, reason = "Matches Laravel's model attribute API")]
impl Pivot {
    pub(super) fn serialize(&self) -> DatabaseResult<Option<serde_json::Value>> {
        self.attributes
            .iter()
            .map(|(name, value)| Ok((name.clone(), serialization::json_value(value.clone())?)))
            .collect::<DatabaseResult<serde_json::Map<_, _>>>()
            .map(|values| Some(serde_json::Value::Object(values)))
    }

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
