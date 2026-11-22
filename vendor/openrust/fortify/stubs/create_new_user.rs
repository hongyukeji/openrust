use crate::app::models::User;
use openrust::contracts::validation::Validator as _;
use openrust::{
    database::eloquent::{Model, ModelState},
    hashing::{Hash, HashOptions},
    http::{HttpError, HttpFuture, RequestValue},
    validation::{Rule, RuleSet, rules::Unique},
};
use openrust_fortify::contracts::{CreatesNewUsers, User as Account};

pub(crate) struct CreateNewUser;
impl CreatesNewUsers for CreateNewUser {
    fn create<'a>(
        &'a self,
        input: &'a RequestValue,
    ) -> HttpFuture<'a, Result<Box<dyn Account>, HttpError>> {
        Box::pin(async move {
            let model = User::from_state(ModelState::default());
            let rules = RuleSet::new()
                .field("name", [Rule::required(), Rule::string(), Rule::max(255)])?
                .field(
                    "email",
                    [
                        Rule::required(),
                        Rule::string(),
                        Rule::email(),
                        Rule::max(255),
                        Unique::fromModel(&model, None).into(),
                    ],
                )?
                .field(
                    "password",
                    super::password_validation_rules::password_rules(),
                )?;
            super::validator(input, rules, Default::default())?
                .validate()
                .await?;
            let password = Hash::make(super::text(input, "password")?, &HashOptions::new()).await?;
            let user = User::create(
                [
                    ("name".into(), super::text(input, "name")?.into()),
                    ("email".into(), super::text(input, "email")?.into()),
                    ("password".into(), password.into()),
                ]
                .into(),
            )
            .await?;
            Ok(Box::new(user) as Box<dyn Account>)
        })
    }
}
