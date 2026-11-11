use crate::{
    auth::access::AuthorizationError,
    contracts::validation::Validator as _,
    http::{HttpError, HttpFuture, Request},
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
#[allow(non_snake_case, reason = "Matches Laravel's FormRequest lifecycle")]
pub trait FormRequest: Send + Sync {
    fn validateResolved<'a>(
        &'a mut self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<ValidatedInput, HttpError>> {
        Box::pin(async move {
            let mut data = Data::new(
                request
                    .input(None)
                    .await?
                    .unwrap_or_else(|| serde_json::json!({})),
            )?;
            self.prepareForValidation(&mut data)?;
            self.authorize(request, &data)
                .await
                .map_err(HttpError::from)?;
            let mut validator = crate::support::facades::Validator::make(data, self.rules()?)?;
            let validated = validator.validate().await.map_err(|mut error| {
                if let ValidationError::Invalid(exception) = &mut error {
                    exception.errorBag(self.errorBag());
                    if let Some(path) = self.redirectTo() {
                        exception.redirectTo(path);
                    }
                }
                error
            })?;
            self.passedValidation(request, &validated)?;
            Ok(validated)
        })
    }

    fn prepareForValidation(&mut self, _input: &mut Data) -> ValidationResult<()> {
        Ok(())
    }

    fn authorize<'a>(
        &'a self,
        _request: &'a mut Request,
        _input: &'a Data,
    ) -> ValidationFuture<'a, Result<(), AuthorizationError>> {
        Box::pin(async { Ok(()) })
    }

    fn errorBag(&self) -> &str {
        "default"
    }

    fn redirectTo(&self) -> Option<&str> {
        None
    }

    fn rules(&self) -> ValidationResult<RuleSet>;

    fn passedValidation(
        &mut self,
        _request: &mut Request,
        _input: &ValidatedInput,
    ) -> ValidationResult<()> {
        Ok(())
    }
}
