use std::collections::BTreeMap;

use crate::validation::{
    Data, RuleSet, ValidationExtension, ValidationReplacer, ValidationResult, Validator,
};

/// Creates validators with custom messages and attribute names.
#[allow(non_snake_case)]
pub trait Factory: Send + Sync {
    fn make(
        &self,
        data: Data,
        rules: RuleSet,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> ValidationResult<Validator>;
    fn extend(&self, rule: &str, extension: ValidationExtension, message: Option<&str>);
    fn extendImplicit(&self, rule: &str, extension: ValidationExtension, message: Option<&str>);
    fn replacer(&self, rule: &str, replacer: ValidationReplacer);
}
