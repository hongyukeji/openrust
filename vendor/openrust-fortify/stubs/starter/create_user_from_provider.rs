use crate::app::models::User;
use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    database::{Transaction, Value, eloquent::Model},
    http::{HttpError, HttpFuture},
    support::facades::{Date, Facade as _, Lang, Validator},
    validation::{Data, RuleSet},
};
use openrust_fortify::starter::socialite::{CreatesUsersFromProvider, ProviderProfile};

pub(crate) struct CreateUserFromProvider;
impl CreatesUsersFromProvider for CreateUserFromProvider {
    fn create<'a>(
        &'a self,
        profile: &'a ProviderProfile,
        transaction: &'a mut Transaction,
    ) -> HttpFuture<'a, Result<Box<dyn openrust_fortify::contracts::User>, HttpError>> {
        Box::pin(async move {
            let Some(email) = profile.verified_email.as_deref() else {
                return Err(invalid("user.provider_email_required")?);
            };
            let data = serde_json::json!({"name":profile.name,"email":email});
            Validator::get_facade_root()?
                .make(
                    Data::new(data)?,
                    RuleSet::parse([
                        ("name", "required|string|max:255"),
                        ("email", "required|string|email|max:255"),
                    ])?,
                    Default::default(),
                    Default::default(),
                )?
                .validate()
                .await?;
            let mut user = User::from_state(Default::default());
            if transaction
                .table(user.get_table())
                .r#where("email", "=", email)
                .exists()
                .await?
            {
                return Err(invalid("user.provider_email_exists")?);
            }
            let now = Date::now(None)
                .map_err(|e| HttpError::Internal {
                    source: Box::new(e),
                })?
                .format("Y-m-d H:i:s")
                .map_err(|e| HttpError::Internal {
                    source: Box::new(e),
                })?;
            user.force_fill(
                [
                    ("name".into(), profile.name.clone().into()),
                    ("email".into(), email.into()),
                    ("password".into(), Value::Null),
                    ("email_verified_at".into(), now.into()),
                    ("credential_version".into(), 0_i64.into()),
                ]
                .into(),
            )?;
            if !user.save_in_transaction(transaction).await? {
                return Err(HttpError::from(std::io::Error::other(
                    "用户模型取消第三方账户创建",
                )));
            }
            Ok(Box::new(user) as Box<dyn openrust_fortify::contracts::User>)
        })
    }
}

fn invalid(key: &str) -> Result<HttpError, HttpError> {
    let message = Lang::get(key, &Default::default(), None)?;
    Ok(HttpError::Validation(Box::new(
        openrust::validation::ValidationException::new(openrust::support::MessageBag::new([(
            "provider",
            vec![message],
        )])),
    )))
}
