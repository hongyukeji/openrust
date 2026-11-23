use crate::{container::ServiceKey, contracts::validation::Factory as _, support::facades::Facade};

use super::{Data, Factory, RuleSet, ValidationError, ValidationResult};

/// Creates validators in the current application.
pub struct Validator;

impl Facade for Validator {
    type Service = Factory;

    fn get_facade_accessor() -> ServiceKey {
        "validator".into()
    }
}

impl Validator {
    pub fn make(data: Data, rules: RuleSet) -> ValidationResult<super::Validator> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .make(data, rules, Default::default(), Default::default())
    }
}

impl Validator {
    pub fn extend(
        rule: &str,
        extension: impl Fn(
            &str,
            Option<&super::Value>,
            &[String],
            &super::Validator,
        ) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
        message: Option<&str>,
    ) -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .extend(rule, extension, message);
        Ok(())
    }
    pub fn extend_implicit(
        rule: &str,
        extension: impl Fn(
            &str,
            Option<&super::Value>,
            &[String],
            &super::Validator,
        ) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
        message: Option<&str>,
    ) -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .extend_implicit(rule, extension, message);
        Ok(())
    }
    pub fn extend_dependent(
        rule: &str,
        extension: impl Fn(
            &str,
            Option<&super::Value>,
            &[String],
            &super::Validator,
        ) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
        message: Option<&str>,
    ) -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .extend_dependent(rule, extension, message);
        Ok(())
    }
    pub fn replacer(
        rule: &str,
        replacer: impl Fn(&str, &str, &str, &[String], &super::Validator) -> String
        + Send
        + Sync
        + 'static,
    ) -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .replacer(rule, replacer);
        Ok(())
    }
    pub fn resolver(
        resolver: impl Fn(super::Validator) -> ValidationResult<super::Validator>
        + Send
        + Sync
        + 'static,
    ) -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .resolver(resolver);
        Ok(())
    }
}

impl Validator {
    pub fn include_unvalidated_array_keys() -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .include_unvalidated_array_keys();
        Ok(())
    }
    pub fn exclude_unvalidated_array_keys() -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .exclude_unvalidated_array_keys();
        Ok(())
    }
}
