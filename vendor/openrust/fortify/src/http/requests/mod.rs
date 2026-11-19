use crate::{Fortify, resolver::Resolver};
use openrust::{
    foundation::http::FormRequest,
    support::facades::Facade as _,
    validation::{RuleSet, ValidationError, ValidationResult},
};
#[derive(Default)]
pub struct LoginRequest;
impl FormRequest for LoginRequest {
    fn rules(&self) -> ValidationResult<RuleSet> {
        let fortify =
            Resolver::<Fortify>::getFacadeRoot().map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?;
        RuleSet::parse([
            (fortify.username().as_str(), "required|string"),
            ("password", "required|string"),
            ("remember", "sometimes"),
        ])
    }
}
