#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
//! Explicit compiled models preserve the primary key independently of the auth identifier.
use crate::resolver::internal;
use openrust::{
    contracts::auth::{Authenticatable, MustVerifyEmail},
    database::{Value, eloquent::Model},
    http::{HttpError, HttpFuture},
};
use std::{any::Any, marker::PhantomData};

pub(crate) trait RegisteredModel: Send + Sync {
    fn account(&self, user: &dyn Authenticatable) -> Option<Box<dyn Account>>;
}
pub(crate) struct ModelAdapter<T>(PhantomData<fn() -> T>);
impl<T> ModelAdapter<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}
impl<T: Model + MustVerifyEmail + Clone> RegisteredModel for ModelAdapter<T> {
    fn account(&self, user: &dyn Authenticatable) -> Option<Box<dyn Account>> {
        (user as &dyn Any)
            .downcast_ref::<T>()
            .map(|user| Box::new(TypedAccount(user.clone())) as Box<dyn Account>)
    }
}
pub(crate) trait Account: Send + Sync {
    fn key(&self) -> Result<String, HttpError>;
    fn email(&self) -> &str;
    fn verify(&mut self) -> HttpFuture<'_, Result<(), HttpError>>;
    fn into_user(self: Box<Self>) -> Box<dyn Authenticatable>;
}
struct TypedAccount<T>(T);
impl<T: Model + MustVerifyEmail> Account for TypedAccount<T> {
    fn key(&self) -> Result<String, HttpError> {
        match self.0.getKey() {
            Some(Value::Text(v)) => Ok(v.clone()),
            Some(Value::Integer(v)) => Ok(v.to_string()),
            Some(Value::Unsigned(v)) => Ok(v.to_string()),
            _ => Err(internal(std::io::Error::other(
                "Invalid verification model key",
            ))),
        }
    }
    fn email(&self) -> &str {
        self.0.getEmailForVerification()
    }
    fn verify(&mut self) -> HttpFuture<'_, Result<(), HttpError>> {
        Box::pin(async move {
            if !self.0.hasVerifiedEmail() && self.0.markEmailAsVerified().await? {
                crate::actions::emit(
                    openrust::auth::events::Verified {
                        user: self.0.getAuthIdentifier(),
                    },
                    &[],
                )
                .await?;
            }
            Ok(())
        })
    }
    fn into_user(self: Box<Self>) -> Box<dyn Authenticatable> {
        Box::new(self.0)
    }
}
pub(crate) fn account(user: &dyn Authenticatable) -> Result<Box<dyn Account>, HttpError> {
    use openrust::support::facades::Facade as _;
    let fortify =
        crate::resolver::Resolver::<crate::Fortify>::get_facade_root().map_err(internal)?;
    fortify
        .email_verification_models
        .read()
        .expect("Fortify model registry lock")
        .get(&(user as &dyn Any).type_id())
        .and_then(|model| model.account(user))
        .ok_or_else(|| {
            internal(std::io::Error::other(
                "Email verification model has not been registered",
            ))
        })
}
pub(crate) fn email_hash(email: &str) -> String {
    aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA1_FOR_LEGACY_USE_ONLY,
        email.as_bytes(),
    )
    .as_ref()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}
