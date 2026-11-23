use crate::resolver::internal;
use openrust::{
    contracts::auth::CanResetPassword,
    database::eloquent::Model,
    http::{HttpError, HttpFuture},
};
use std::{any::Any, marker::PhantomData};

pub(crate) trait RegisteredModel: Send + Sync {
    fn persist<'a>(
        &'a self,
        user: &'a mut dyn CanResetPassword,
        remember_token: String,
    ) -> HttpFuture<'a, Result<(), HttpError>>;
}
pub(crate) struct ModelAdapter<T>(PhantomData<fn() -> T>);
impl<T> ModelAdapter<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}
impl<T: Model + CanResetPassword> RegisteredModel for ModelAdapter<T> {
    fn persist<'a>(
        &'a self,
        user: &'a mut dyn CanResetPassword,
        remember_token: String,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            let user = (user as &mut dyn Any)
                .downcast_mut::<T>()
                .ok_or_else(|| internal(std::io::Error::other("Password reset model mismatch")))?;
            user.set_remember_token(Some(remember_token));
            user.save().await.map_err(internal)?;
            Ok(())
        })
    }
}
