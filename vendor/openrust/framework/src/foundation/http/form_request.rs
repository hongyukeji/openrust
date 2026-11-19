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
#[allow(non_snake_case, reason = "Matches Laravel's FormRequest lifecycle")]
pub trait FormRequest: Send + Sync {
    fn validateResolved<'a>(
        &'a mut self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<ValidatedInput, HttpError>> {
        Box::pin(async move {
            let mut data = Data::fromRequest(request).await?;
            self.prepareForValidation(&mut data)?;
            self.authorize(request, &data)
                .await
                .map_err(HttpError::from)?;
            let rules = self.validationRules(request)?;
            let rules = if request.isPrecognitive() {
                request.filterPrecognitiveRules(rules)?
            } else {
                rules
            };
            let mut validator = crate::support::facades::Validator::make(data, rules)?;
            let validated = validator
                .validateRequest(request)
                .await
                .map_err(|mut error| {
                    if let ValidationError::Invalid(exception) = &mut error {
                        exception.errorBag(self.errorBag());
                        if let Some(path) = self.redirectTo() {
                            exception.redirectTo(path);
                        }
                    }
                    error
                })?;
            if request.isPrecognitive() {
                (crate::foundation::Precognition::afterValidationHook(request))(&validator)?;
            }
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

    fn validationRules(&self, _request: &Request) -> ValidationResult<RuleSet> {
        self.rules()
    }

    fn passedValidation(
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
            let form = request
                .withContainer(container.makeAsync::<F>())
                .await
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
            std::sync::Arc::into_inner(form).ok_or_else(|| HttpError::Internal {
                source: std::io::Error::other(
                    "A form request binding must return an owned instance for each resolution.",
                )
                .into(),
            })?
        } else {
            F::default()
        };
        let input = form.validateResolved(request).await?;
        Ok(Self { form, input })
    }
}
