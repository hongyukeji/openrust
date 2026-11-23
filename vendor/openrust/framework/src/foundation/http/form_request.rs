use crate::{
    auth::access::AuthorizationError,
    contracts::container::Container as _,
    http::{HttpError, HttpFuture, Request},
    support::facades::current_container,
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
    fn validate_resolved<'a>(
        &'a mut self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<ValidatedInput, HttpError>> {
        Box::pin(async move {
            let mut data = Data::from_request(request).await?;
            self.prepare_for_validation(&mut data)?;
            self.authorize(request, &data)
                .await
                .map_err(HttpError::from)?;
            let rules = self.validation_rules(request)?;
            let rules = if request.is_precognitive() {
                request.filter_precognitive_rules(rules)?
            } else {
                rules
            };
            let mut validator = crate::support::facades::Validator::make(data, rules)?;
            validator.stop_on_first_failure(self.stop_on_first_failure());
            self.with_validator(&mut validator)?;
            for callback in self.after() {
                validator.after(callback);
            }
            let validated = validator
                .validate_request(request)
                .await
                .map_err(|mut error| {
                    if let ValidationError::Invalid(exception) = &mut error {
                        exception.error_bag(self.error_bag());
                        if let Some(path) = self.redirect_to() {
                            exception.redirect_to(path);
                        }
                    }
                    error
                })?;
            if request.is_precognitive() {
                (crate::foundation::Precognition::afterValidationHook(request))(&validator)?;
            }
            self.passed_validation(request, &validated)?;
            Ok(validated)
        })
    }

    fn stop_on_first_failure(&self) -> bool {
        false
    }

    fn with_validator(
        &mut self,
        _validator: &mut crate::validation::Validator,
    ) -> ValidationResult<()> {
        Ok(())
    }

    fn after(&self) -> Vec<crate::validation::AfterValidation> {
        Vec::new()
    }

    fn prepare_for_validation(&mut self, _input: &mut Data) -> ValidationResult<()> {
        Ok(())
    }

    fn authorize<'a>(
        &'a self,
        _request: &'a mut Request,
        _input: &'a Data,
    ) -> ValidationFuture<'a, Result<(), AuthorizationError>> {
        Box::pin(async { Ok(()) })
    }

    fn error_bag(&self) -> &str {
        "default"
    }

    fn redirect_to(&self) -> Option<&str> {
        None
    }

    fn rules(&self) -> ValidationResult<RuleSet>;

    fn validation_rules(&self, _request: &Request) -> ValidationResult<RuleSet> {
        self.rules()
    }

    fn passed_validation(
        &mut self,
        _request: &mut Request,
        _input: &ValidatedInput,
    ) -> ValidationResult<()> {
        Ok(())
    }
}

/// A form request and its validated input, injected before the action runs.
pub struct ValidatedFormRequest<F> {
    form: F,
    input: ValidatedInput,
}

impl<F> ValidatedFormRequest<F> {
    pub fn form(&self) -> &F {
        &self.form
    }

    pub fn validated(&self) -> &ValidatedInput {
        &self.input
    }

    pub fn into_parts(self) -> (F, ValidatedInput) {
        (self.form, self.input)
    }
}

impl<F: FormRequest + Default + 'static> ValidatedFormRequest<F> {
    pub(crate) async fn resolve(request: &mut Request) -> Result<Self, HttpError> {
        let mut form = if let Some(container) = current_container().filter(|app| app.bound::<F>()) {
            let form = request.with_container(container.make_async::<F>()).await?;
            std::sync::Arc::into_inner(form).ok_or_else(|| HttpError::Internal {
                source: std::io::Error::other(
                    "A form request binding must return an owned instance for each resolution.",
                )
                .into(),
            })?
        } else {
            F::default()
        };
        let input = form.validate_resolved(request).await?;
        Ok(Self { form, input })
    }
}
