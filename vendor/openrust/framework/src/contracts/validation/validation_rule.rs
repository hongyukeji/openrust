use crate::validation::{RuleContext, RuleFailure, ValidationFuture, ValidationResult, Value};

/// Checks an attribute and returns its violations, if any.
pub trait ValidationRule: Send + Sync {
    fn validate<'a>(
        &'a self,
        context: &'a RuleContext,
        attribute: &'a str,
        value: Option<&'a Value>,
    ) -> ValidationFuture<'a, ValidationResult<Vec<RuleFailure>>>;
}
