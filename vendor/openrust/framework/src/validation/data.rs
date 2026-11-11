use serde_json::Value;

use super::{ValidationError, ValidationResult, path};

/// Structured request data supplied to a validator.
#[derive(Clone, Debug)]
pub struct Data(Value);

impl Data {
    /// Builds validation input from a JSON object or array.
    pub fn new(input: Value) -> ValidationResult<Self> {
        if !(input.is_object() || input.is_array()) {
            return Err(ValidationError::InvalidPath);
        }
        path::inspect(&input)?;
        Ok(Self(input))
    }

    pub fn replace(&mut self, input: Value) -> ValidationResult<()> {
        *self = Self::new(input)?;
        Ok(())
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// Resolves a dot path or wildcard against this input.
    ///
    /// An absent child beneath a matched parent is returned with no value.
    /// An explicit JSON null remains a supplied value.
    pub fn attributes(&self, attribute: &str) -> ValidationResult<Vec<(String, Option<&Value>)>> {
        self.expand(attribute, &mut path::Budget::default())
    }

    pub(super) fn expand(
        &self,
        attribute: &str,
        budget: &mut path::Budget,
    ) -> ValidationResult<Vec<(String, Option<&Value>)>> {
        path::expand(&self.0, &path::parse(attribute)?, budget)
    }
}
