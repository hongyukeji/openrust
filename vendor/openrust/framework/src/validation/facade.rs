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

#[allow(non_snake_case)]
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
    pub fn extendImplicit(
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
            .extendImplicit(rule, extension, message);
        Ok(())
    }
    pub fn extendDependent(
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
            .extendDependent(rule, extension, message);
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

#[allow(non_snake_case)]
impl Validator {
    pub fn includeUnvalidatedArrayKeys() -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .includeUnvalidatedArrayKeys();
        Ok(())
    }
    pub fn excludeUnvalidatedArrayKeys() -> ValidationResult<()> {
        Self::get_facade_root()
            .map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?
            .excludeUnvalidatedArrayKeys();
        Ok(())
    }
}
