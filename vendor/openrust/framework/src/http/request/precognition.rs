use crate::validation::{RuleSet, ValidationResult};

use super::Request;

#[allow(non_snake_case, reason = "Matches Laravel's CanBePrecognitive concern")]
impl Request {
    pub fn isAttemptingPrecognition(&self) -> bool {
        self.headers()
            .get("precognition")
            .is_some_and(|value| value == "true")
    }

    pub fn isPrecognitive(&self) -> bool {
        self.attributes()
            .get("precognitive")
            .and_then(|value| value.downcast::<bool>().ok())
            .is_some_and(|value| *value)
    }

    /// Selects concrete attributes after the validator expands wildcard rules.
    pub fn filterPrecognitiveRules(&self, rules: RuleSet) -> ValidationResult<RuleSet> {
        match self.headers().get("precognition-validate-only") {
            Some(value) => rules.precognitive(value.as_bytes()),
            None => Ok(rules),
        }
    }
}
