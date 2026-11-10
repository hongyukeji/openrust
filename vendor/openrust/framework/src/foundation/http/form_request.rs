use crate::{
    auth::access::AuthorizationError,
    http::Request,
    validation::{
        Data, RuleSet, ValidatedInput, ValidationError, ValidationFuture, ValidationResult,
    },
};

/// A failure while resolving an application form request.
#[derive(Debug)]
pub enum FormRequestError {
    Authorization(AuthorizationError),
    Validation(ValidationError),
}

/// Defines the authorization and validation of a handler's input.
pub trait FormRequest: Send + Sync {
    fn prepare_for_validation(&mut self, input: &mut Data) -> ValidationResult<()>;

    fn authorize<'a>(
        &'a self,
        request: &'a mut Request,
        input: &'a Data,
    ) -> ValidationFuture<'a, Result<(), AuthorizationError>>;

    fn rules(&self) -> ValidationResult<RuleSet>;

    fn passed_validation(
        &mut self,
        request: &mut Request,
        input: &ValidatedInput,
    ) -> ValidationResult<()>;
}
