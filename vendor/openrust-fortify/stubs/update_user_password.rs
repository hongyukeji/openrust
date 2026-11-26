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
                "UpdateUserPassword requires update_from_request to resolve current_password:web",
            )))
        })
    }
    fn update_from_request<'a>(
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
                        Rule::current_password(Some("web")),
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
                .validate_request(request)
                .await
                .map_err(|mut error| {
                    if let ValidationError::Invalid(exception) = &mut error {
                        exception.error_bag("updatePassword");
                    }
                    error
                })?;
            let mut user = super::user(user)?;
            super::save_password(&mut user, input).await?;
            Ok(Box::new(user) as Box<dyn User>)
        })
    }
    // 纯令牌客户端使用绑定当前凭据的password用途近期证明，不伪造web Guard。
    fn validate_for_transaction<'a>(
        &'a self,
        _user: &'a dyn Authenticatable,
        input: &'a RequestValue,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            let rules = RuleSet::new().field(
                "password",
                super::password_validation_rules::password_rules(),
            )?;
            super::validator(input, rules, Default::default())?
                .validate_with_bag("updatePassword")
                .await?;
            Ok(())
        })
    }
    fn update_in_transaction<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        input: &'a RequestValue,
        transaction: &'a mut openrust::database::Transaction,
    ) -> HttpFuture<'a, Result<Box<dyn User>, HttpError>> {
        Box::pin(async move {
            #[cfg(feature = "sanctum")]
            {
                let mut user = super::user(user)?;
                openrust_fortify::starter::credentials::change_password_in_transaction(
                    &mut user,
                    transaction,
                    super::text(input, "password")?,
                )
                .await?;
                Ok(Box::new(user) as Box<dyn User>)
            }
            #[cfg(not(feature = "sanctum"))]
            {
                let _ = (user, input, transaction);
                Err(HttpError::from(std::io::Error::other(
                    "套件密码事务需要Sanctum功能",
                )))
            }
        })
    }
}
