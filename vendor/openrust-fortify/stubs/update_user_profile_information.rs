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
                        Unique::new("users", None).ignore_model(&user, None).into(),
                    ],
                )?;
            super::validator(input, rules, Default::default())?
                .validate_with_bag("updateProfileInformation")
                .await?;
            let email = super::text(input, "email")?;
            let changed = user.get_attribute("email") != Some(&Value::Text(email.into()));
            let mut attributes = std::collections::BTreeMap::from([
                ("name".into(), super::text(input, "name")?.into()),
                ("email".into(), email.into()),
            ]);
            if changed {
                attributes.insert("email_verified_at".into(), Value::Null);
            }
            user.force_fill(attributes)?.save().await?;
            if changed {
                user.send_email_verification_notification()
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
            }
            Ok(Box::new(user) as Box<dyn User>)
        })
    }
    // 套件先验证，再在共享用户锁的同一事务中保存。
    fn validate_for_transaction<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        input: &'a RequestValue,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            let user = super::user(user)?;
            let rules = RuleSet::new()
                .field("name", [Rule::required(), Rule::string(), Rule::max(255)])?
                .field(
                    "email",
                    [
                        Rule::required(),
                        Rule::string(),
                        Rule::email(),
                        Rule::max(255),
                        Unique::from_model(&user, None)
                            .ignore_model(&user, None)
                            .into(),
                    ],
                )?;
            super::validator(input, rules, Default::default())?
                .validate_with_bag("updateProfileInformation")
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
            let mut user = super::user(user)?;
            let email = super::text(input, "email")?;
            let changed = user.get_attribute("email") != Some(&Value::Text(email.into()));
            let mut attributes = std::collections::BTreeMap::from([
                ("name".into(), super::text(input, "name")?.into()),
                ("email".into(), email.into()),
            ]);
            if changed {
                attributes.insert("email_verified_at".into(), Value::Null);
            }
            user.force_fill(attributes)?;
            if !user.save_in_transaction(transaction).await? {
                return Err(HttpError::from(std::io::Error::other(
                    "用户模型取消资料保存",
                )));
            }
            Ok(Box::new(user) as Box<dyn User>)
        })
    }
}
