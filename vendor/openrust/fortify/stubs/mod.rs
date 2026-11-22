mod create_new_user;
mod password_validation_rules;
mod reset_user_password;
mod update_user_password;
mod update_user_profile_information;

pub(crate) use create_new_user::CreateNewUser;
pub(crate) use reset_user_password::ResetUserPassword;
pub(crate) use update_user_password::UpdateUserPassword;
pub(crate) use update_user_profile_information::UpdateUserProfileInformation;

use crate::app::models::User;
use openrust::{
    contracts::{auth::Authenticatable, validation::Factory as _},
    database::eloquent::Model,
    hashing::{Hash, HashOptions},
    http::{HttpError, RequestValue},
    support::facades::{Facade as _, Validator},
    validation::{Data, RuleSet, Validator as NativeValidator},
};

fn user(user: &dyn Authenticatable) -> Result<User, HttpError> {
    (user as &dyn std::any::Any)
        .downcast_ref::<User>()
        .cloned()
        .ok_or_else(|| {
            HttpError::from(std::io::Error::other(
                "Fortify actions require app::models::User",
            ))
        })
}
fn text<'a>(input: &'a RequestValue, key: &str) -> Result<&'a str, HttpError> {
    match input.get(key) {
        Some(RequestValue::Value(serde_json::Value::String(text))) => Ok(text),
        _ => Err(HttpError::InvalidRequest),
    }
}
fn validator(
    input: &RequestValue,
    rules: RuleSet,
    messages: std::collections::BTreeMap<String, String>,
) -> Result<NativeValidator, HttpError> {
    Ok(Validator::getFacadeRoot()?.make(
        Data::fromRequestValue(input)?,
        rules,
        messages,
        Default::default(),
    )?)
}
async fn save_password(user: &mut User, input: &RequestValue) -> Result<(), HttpError> {
    let password = Hash::make(text(input, "password")?, &HashOptions::new()).await?;
    user.forceFill([("password".into(), password.into())].into())?
        .save()
        .await?;
    Ok(())
}
