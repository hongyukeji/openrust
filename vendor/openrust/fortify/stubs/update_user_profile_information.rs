use openrust::{
    contracts::auth::{Authenticatable, MustVerifyEmail},
    database::{Value, eloquent::Model},
    http::{HttpError, HttpFuture, RequestValue},
    validation::{Rule, RuleSet, rules::Unique},
};
use openrust_fortify::contracts::{UpdatesUserProfileInformation, User};

pub(crate) struct UpdateUserProfileInformation;
impl UpdatesUserProfileInformation for UpdateUserProfileInformation {
    fn update<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        input: &'a RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn User>, HttpError>> {
        Box::pin(async move {
            let mut user = super::user(user)?;
            let rules = RuleSet::new()
                .field("name", [Rule::required(), Rule::string(), Rule::max(255)])?
                .field(
                    "email",
                    [
                        Rule::required(),
                        Rule::string(),
                        Rule::email(),
                        Rule::max(255),
                        Unique::new("users", None).ignoreModel(&user, None).into(),
                    ],
                )?;
            super::validator(input, rules, Default::default())?
                .validateWithBag("updateProfileInformation")
                .await?;
            let email = super::text(input, "email")?;
            let changed = user.getAttribute("email") != Some(&Value::Text(email.into()));
            let mut attributes = std::collections::BTreeMap::from([
                ("name".into(), super::text(input, "name")?.into()),
                ("email".into(), email.into()),
            ]);
            if changed {
                attributes.insert("email_verified_at".into(), Value::Null);
            }
            user.forceFill(attributes)
                .map_err(super::internal)?
                .save()
                .await
                .map_err(super::internal)?;
            if changed {
                user.sendEmailVerificationNotification()
                    .await
                    .map_err(super::internal)?;
            }
            Ok(Box::new(user) as Box<dyn User>)
        })
    }
}
