use crate::{
    support::MessageBag,
    validation::{ValidatedInput, ValidationFuture, ValidationResult},
};

/// Evaluates rules and exposes their errors and validated data.
pub trait Validator: Send {
    fn validate(&mut self) -> ValidationFuture<'_, ValidationResult<ValidatedInput>>;
    fn fails(&mut self) -> ValidationFuture<'_, ValidationResult<bool>>;
    fn validated(&self) -> ValidationResult<ValidatedInput>;
    fn errors(&self) -> &MessageBag;
}
