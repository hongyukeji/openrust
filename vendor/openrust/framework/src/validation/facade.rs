use crate::{container::ServiceKey, contracts::validation::Factory as _, support::facades::Facade};

use super::{Data, Factory, RuleSet, ValidationError, ValidationResult};

/// Creates validators in the current application.
pub struct Validator;

impl Facade for Validator {
    type Service = Factory;

    fn getFacadeAccessor() -> ServiceKey {
        "validator".into()
    }
}

impl Validator {
    pub fn make(data: Data, rules: RuleSet) -> ValidationResult<super::Validator> {
        Self::getFacadeRoot()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .make(data, rules, Default::default(), Default::default())
    }
}
