use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model},
};

pub async fn force_change_password(
    record: &mut impl Model,
    password: &str,
) -> DatabaseResult<bool> {
    record
        .force_fill_async(Attributes::from([("password".into(), password.into())]))
        .await?;
    record.save().await
}
