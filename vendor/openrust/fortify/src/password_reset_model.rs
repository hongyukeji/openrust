#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::resolver::internal;
use openrust::{
    contracts::auth::CanResetPassword,
    database::eloquent::Model,
    http::{HttpError, HttpFuture},
    support::Str,
};
use std::{any::Any, marker::PhantomData};

pub(crate) trait RegisteredModel: Send + Sync {
    fn complete<'a>(
        &'a self,
        user: &'a mut dyn CanResetPassword,
    ) -> HttpFuture<'a, Result<(), HttpError>>;
}
pub(crate) struct ModelAdapter<T>(PhantomData<fn() -> T>);
impl<T> ModelAdapter<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}
impl<T: Model + CanResetPassword> RegisteredModel for ModelAdapter<T> {
    fn complete<'a>(
        &'a self,
        user: &'a mut dyn CanResetPassword,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            let user = (user as &mut dyn Any)
                .downcast_mut::<T>()
                .ok_or_else(|| internal(std::io::Error::other("Password reset model mismatch")))?;
            user.set_remember_token(Some(Str::random(60)));
            user.save().await.map_err(internal)?;
            crate::actions::emit(
                openrust::auth::events::PasswordReset {
                    user: user.get_auth_identifier(),
                },
                &[],
            )
            .await
        })
    }
}
