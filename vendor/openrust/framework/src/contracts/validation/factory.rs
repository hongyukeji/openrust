use std::collections::BTreeMap;

use crate::validation::{Data, RuleSet, ValidationResult, Validator};

/// Creates validators with custom messages and attribute names.
pub trait Factory: Send + Sync {
    fn make(
        &self,
        data: Data,
        rules: RuleSet,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> ValidationResult<Validator>;
}
