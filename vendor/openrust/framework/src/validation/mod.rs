//! Validation rules, results, and error reporting.

mod factory;
mod rule;
pub mod rules;
mod validated_input;
mod validation_exception;
mod validation_service_provider;
mod validator;

use std::{error::Error, future::Future, pin::Pin};

pub use factory::Factory;
pub use rule::Rule;
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
    Execution {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Structured input including files prepared for validation.
pub struct Data {
    _private: (),
}

/// A validation input value. Missing input is represented separately.
pub struct Value {
    _private: (),
}

/// Field rules in their configured evaluation order.
pub struct RuleSet {
    _private: (),
}

/// The input and scoped services available to a validation rule.
pub struct RuleContext {
    _private: (),
}

/// A rule failure with a message key or literal and replacement parameters.
pub struct RuleFailure {
    _private: (),
}
