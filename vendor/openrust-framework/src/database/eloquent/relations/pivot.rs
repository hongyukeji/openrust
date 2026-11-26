use crate::database::{
    DatabaseResult, Value,
    eloquent::{Attributes, serialization},
};

/// Attributes for one intermediate-table row.
pub struct Pivot {
    table: &'static str,
    attributes: Attributes,
}

impl Pivot {
    pub(in crate::database::eloquent) fn serialize(
        &self,
    ) -> DatabaseResult<Option<serde_json::Value>> {
        self.attributes
            .iter()
            .map(|(name, value)| Ok((name.clone(), serialization::json_value(value.clone())?)))
            .collect::<DatabaseResult<serde_json::Map<_, _>>>()
            .map(|values| Some(serde_json::Value::Object(values)))
    }

    pub(super) fn new(table: &'static str, attributes: Attributes) -> Self {
        Self { table, attributes }
    }

    pub fn get_table(&self) -> &'static str {
        self.table
    }

    pub fn get_attribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name)
    }

    pub fn get_attributes(&self) -> &Attributes {
        &self.attributes
    }
}
