//! Application-owned type erasure for explicitly registered Eloquent models.
use crate::{
    Fortify, FortifyError, FortifyResult, TwoFactorAuthenticatable,
    actions::*,
    resolver::{Resolver, internal},
};
use openrust::{
    contracts::auth::Authenticatable,
    database::Value,
    http::{HttpError, HttpFuture},
    support::facades::Facade as _,
};
use std::{
    any::{Any, TypeId},
    marker::PhantomData,
    sync::Arc,
};

pub(crate) trait RegisteredModel: Send + Sync {
    fn account(&self, user: &dyn Authenticatable) -> Option<Box<dyn Account>>;
    fn find(&self, key: Value) -> HttpFuture<'_, Result<Option<Box<dyn Account>>, HttpError>>;
}
pub(crate) struct ModelAdapter<T>(PhantomData<fn() -> T>);
impl<T> ModelAdapter<T> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}
impl<T: TwoFactorAuthenticatable + Clone> RegisteredModel for ModelAdapter<T> {
    fn account(&self, user: &dyn Authenticatable) -> Option<Box<dyn Account>> {
        (user as &dyn Any)
            .downcast_ref::<T>()
            .map(|user| Box::new(TypedAccount(user.clone())) as Box<dyn Account>)
    }
    fn find(&self, key: Value) -> HttpFuture<'_, Result<Option<Box<dyn Account>>, HttpError>> {
        Box::pin(async move {
            Ok(T::find(key)
                .await
                .map_err(internal)?
                .map(|user| Box::new(TypedAccount(user)) as Box<dyn Account>))
        })
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Mutation<'a> {
    Enable(bool),
    Confirm(&'a str),
    Disable,
    Generate,
    Consume(&'a str),
}
pub(crate) enum AccountEvent {
    Challenged,
    Failed,
    Valid,
}
pub(crate) trait Account: Send + Sync {
    fn into_user(self: Box<Self>) -> Box<dyn Authenticatable>;
    fn key(&self) -> FortifyResult<serde_json::Value>;
    fn attribute(&self, name: &str) -> FortifyResult<Option<Value>>;
    fn recovery_codes(&self, fortify: &Fortify) -> FortifyResult<Vec<String>>;
    fn qr_url(
        &self,
        fortify: &Fortify,
        provider: &dyn crate::contracts::TwoFactorAuthenticationProvider,
    ) -> FortifyResult<String>;
    fn qr_svg(
        &self,
        fortify: &Fortify,
        provider: &dyn crate::contracts::TwoFactorAuthenticationProvider,
    ) -> FortifyResult<String>;
    fn mutate<'a>(&'a mut self, mutation: Mutation<'a>) -> HttpFuture<'a, Result<(), HttpError>>;
    fn emit(&self, event: AccountEvent) -> HttpFuture<'_, Result<(), HttpError>>;
}
struct TypedAccount<T>(T);
impl<T: TwoFactorAuthenticatable> Account for TypedAccount<T> {
    fn into_user(self: Box<Self>) -> Box<dyn Authenticatable> {
        Box::new(self.0)
    }
    fn key(&self) -> FortifyResult<serde_json::Value> {
        match self.0.get_key() {
            Some(Value::Text(v)) => Ok(v.clone().into()),
            Some(Value::Integer(v)) => Ok((*v).into()),
            Some(Value::Unsigned(v)) => Ok((*v).into()),
            _ => Err(FortifyError::InvalidAttribute("primary key")),
        }
    }
    fn attribute(&self, name: &str) -> FortifyResult<Option<Value>> {
        Ok(self.0.get_attribute_value(name)?)
    }
    fn recovery_codes(&self, fortify: &Fortify) -> FortifyResult<Vec<String>> {
        self.0.recovery_codes(fortify)
    }
    fn qr_url(
        &self,
        fortify: &Fortify,
        provider: &dyn crate::contracts::TwoFactorAuthenticationProvider,
    ) -> FortifyResult<String> {
        self.0.two_factor_qr_code_url(fortify, provider)
    }
    fn qr_svg(
        &self,
        fortify: &Fortify,
        provider: &dyn crate::contracts::TwoFactorAuthenticationProvider,
    ) -> FortifyResult<String> {
        self.0.two_factor_qr_code_svg(fortify, provider)
    }
    fn mutate<'a>(&'a mut self, mutation: Mutation<'a>) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async move {
            match mutation {
                Mutation::Enable(force) => {
                    Resolver::<EnableTwoFactorAuthentication>::get_facade_root()
                        .map_err(internal)?
                        .call(&mut self.0, force)
                        .await
                }
                Mutation::Confirm(code) => {
                    Resolver::<ConfirmTwoFactorAuthentication>::get_facade_root()
                        .map_err(internal)?
                        .call(&mut self.0, code)
                        .await
                }
                Mutation::Disable => {
                    Resolver::<DisableTwoFactorAuthentication>::get_facade_root()
                        .map_err(internal)?
                        .call(&mut self.0)
                        .await
                }
                Mutation::Generate => {
                    Resolver::<GenerateNewRecoveryCodes>::get_facade_root()
                        .map_err(internal)?
                        .call(&mut self.0)
                        .await
                }
                Mutation::Consume(code) => {
                    let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
                    crate::actions::consume_recovery_code(&mut self.0, code, &fortify).await
                }
            }
        })
    }
    fn emit(&self, event: AccountEvent) -> HttpFuture<'_, Result<(), HttpError>> {
        Box::pin(async move {
            use crate::events::*;
            let user = self.0.get_auth_identifier();
            match event {
                AccountEvent::Challenged => {
                    crate::actions::emit(TwoFactorAuthenticationChallenged { user }, &[&self.0])
                        .await
                }
                AccountEvent::Failed => {
                    crate::actions::emit(TwoFactorAuthenticationFailed { user }, &[&self.0]).await
                }
                AccountEvent::Valid => {
                    crate::actions::emit(
                        ValidTwoFactorAuthenticationCodeProvided { user },
                        &[&self.0],
                    )
                    .await
                }
            }
        })
    }
}
impl Fortify {
    pub(crate) fn registered_model(&self, id: TypeId) -> FortifyResult<Arc<dyn RegisteredModel>> {
        self.two_factor_models
            .read()
            .expect("Fortify model registry lock")
            .get(&id)
            .cloned()
            .ok_or(FortifyError::UnregisteredTwoFactorModel)
    }
    pub(crate) fn account(&self, user: &dyn Authenticatable) -> Option<Box<dyn Account>> {
        self.registered_model(user.type_id()).ok()?.account(user)
    }
}
pub(crate) fn present(value: Option<Value>) -> bool {
    !matches!(value, None | Some(Value::Null))
}
pub(crate) fn truthy(value: Option<Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Text(v)) => !v.is_empty() && v != "0",
        Some(Value::Bool(v)) => v,
        Some(Value::Integer(v)) => v != 0,
        Some(Value::Unsigned(v)) => v != 0,
        Some(Value::Real(v)) => v != 0.0,
        _ => true,
    }
}
pub(crate) fn text(value: Option<Value>) -> FortifyResult<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(v)) => Ok(Some(v)),
        _ => Err(FortifyError::InvalidAttribute("two-factor field")),
    }
}
