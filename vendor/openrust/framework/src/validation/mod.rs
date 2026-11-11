//! Validation rules, results, and error reporting.

mod data;
pub(crate) mod facade;
mod factory;
mod path;
mod rule;
mod rule_set;
pub mod rules;
mod validated_input;
mod validation_exception;
mod validation_service_provider;
mod validator;

use std::{error::Error, future::Future, pin::Pin};

pub use data::Data;
pub use factory::Factory;
pub use rule::Rule;
pub use rule_set::RuleSet;
pub use validated_input::ValidatedInput;
pub use validation_exception::ValidationException;
pub use validation_service_provider::ValidationServiceProvider;
pub use validator::Validator;

/// A validation operation that may perform asynchronous I/O.
pub type ValidationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a validation operation.
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Invalid user input or a failure while running validation.
#[derive(Debug)]
pub enum ValidationError {
    Invalid(Box<ValidationException>),
    InvalidPath,
    InvalidRule,
    NotValidated,
    LimitsExceeded,
    Execution {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// A validation input value. Missing input is represented separately.
pub type Value = serde_json::Value;

/// The data available to a custom validation rule.
pub struct RuleContext<'a> {
    pub data: &'a Data,
}

/// A custom rule failure with a translation key or literal message.
pub struct RuleFailure {
    pub message: String,
    pub replacements: std::collections::BTreeMap<String, String>,
}

impl RuleFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            replacements: Default::default(),
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(exception) => exception.fmt(f),
            Self::InvalidPath => f.write_str("invalid validation field path"),
            Self::InvalidRule => f.write_str("invalid validation rule"),
            Self::NotValidated => f.write_str("validation has not completed"),
            Self::LimitsExceeded => f.write_str("validation input exceeds its limits"),
            Self::Execution { .. } => f.write_str("validation could not be executed"),
        }
    }
}

impl Error for ValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invalid(source) => Some(source),
            Self::Execution { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}
