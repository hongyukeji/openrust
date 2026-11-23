use crate::validation::{RuleSet, ValidationResult};

use super::Request;

impl Request {
    pub fn is_attempting_precognition(&self) -> bool {
        self.headers()
            .get("precognition")
            .is_some_and(|value| value == "true")
    }

    pub fn is_precognitive(&self) -> bool {
        self.attributes()
            .get("precognitive")
            .and_then(|value| value.downcast::<bool>().ok())
            .is_some_and(|value| *value)
    }

    /// Selects concrete attributes after the validator expands wildcard rules.
    pub fn filter_precognitive_rules(&self, rules: RuleSet) -> ValidationResult<RuleSet> {
        match self.headers().get("precognition-validate-only") {
            Some(value) => rules.precognitive(value.as_bytes()),
            None => Ok(rules),
        }
    }
}
