use openrust::{
    contracts::auth::Authenticatable,
    http::{HttpError, HttpFuture, Request, RequestValue},
    validation::{Rule, RuleSet, ValidationError},
};
use openrust_fortify::contracts::{UpdatesUserPasswords, User};

pub(crate) struct UpdateUserPassword;
impl UpdatesUserPasswords for UpdateUserPassword {
    fn update<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, HttpError>> {
        Box::pin(async {
            Err(HttpError::from(std::io::Error::other(
                "UpdateUserPassword requires updateFromRequest to resolve current_password:web",
            )))
        })
    }
    #[allow(non_snake_case, reason = "Request-aware Fortify action adapter")]
    fn updateFromRequest<'a>(
        &'a self,
        request: &'a mut Request,
        user: &'a dyn Authenticatable,
        input: &'a RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, HttpError>> {
        Box::pin(async move {
            let rules = RuleSet::new()
                .field(
                    "current_password",
                    [
                        Rule::required(),
                        Rule::string(),
                        Rule::currentPassword(Some("web")),
                    ],
                )?
                .field(
                    "password",
                    super::password_validation_rules::password_rules(),
                )?;
            let messages = [(
                "current_password.current_password".into(),
                openrust::support::facades::Lang::get(
                    "The provided password does not match your current password.",
                    &Default::default(),
                    None,
                )?,
            )]
            .into();
            super::validator(input, rules, messages)?
                .validateRequest(request)
                .await
                .map_err(|mut error| {
                    if let ValidationError::Invalid(exception) = &mut error {
                        exception.errorBag("updatePassword");
                    }
                    error
                })?;
            let mut user = super::user(user)?;
            super::save_password(&mut user, input).await?;
            Ok(Box::new(user) as Box<dyn User>)
        })
    }
}
