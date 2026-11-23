use crate::database::{
    DatabaseResult, Value,
    eloquent::{Attributes, serialization},
};

/// Attributes for one intermediate-table row.
pub struct Pivot {
    table: &'static str,
    attributes: Attributes,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
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

    pub fn getTable(&self) -> &'static str {
        self.table
    }

    pub fn getAttribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name)
    }

    pub fn getAttributes(&self) -> &Attributes {
        &self.attributes
    }
}
