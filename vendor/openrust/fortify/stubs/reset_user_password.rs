use crate::app::models::User;
use openrust::contracts::validation::Validator as _;
use openrust::{
    contracts::auth::CanResetPassword,
    http::{HttpError, HttpFuture, RequestValue},
    validation::RuleSet,
};
use openrust_fortify::contracts::ResetsUserPasswords;

pub(crate) struct ResetUserPassword;
impl ResetsUserPasswords for ResetUserPassword {
    fn reset<'a>(
        &'a self,
        user: &'a mut dyn CanResetPassword,
        input: &'a RequestValue,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            let rules = RuleSet::new().field(
                "password",
                super::password_validation_rules::password_rules(),
            )?;
            super::validator(input, rules, Default::default())?
                .validate()
                .await?;
            let user = (user as &mut dyn std::any::Any)
                .downcast_mut::<User>()
                .ok_or_else(|| {
                    super::internal(std::io::Error::other(
                        "Fortify reset requires app::models::User",
                    ))
                })?;
            super::save_password(user, input).await
        })
    }
}
