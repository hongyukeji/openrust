use crate::{PasskeyRecord, Passkeys, contracts::PasskeyUser};
use openrust::{
    database::{DatabaseError, DatabaseResult, Value, eloquent::Attributes},
    support::facades::Facade as _,
};
use std::{marker::PhantomData, sync::Arc};
/// Application-local relationship selecting the registered, actual Eloquent model.
pub struct PasskeyRelation<P>(PhantomData<fn() -> P>);
#[allow(
    non_snake_case,
    reason = "Matches native Eloquent relationship operations"
)]
impl<P: PasskeyUser> PasskeyRelation<P> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
    fn service() -> DatabaseResult<Arc<Passkeys>> {
        crate::http::Resolver::<Passkeys>::get_facade_root()
            .map_err(|_| DatabaseError::InvalidQuery("Passkeys service is not bound"))
    }
    fn key(parent: &P) -> DatabaseResult<Value> {
        parent.get_key().cloned().ok_or(DatabaseError::InvalidQuery(
            "Passkeys relationship requires a persisted user key",
        ))
    }
    pub async fn get_results(&self, parent: &P) -> DatabaseResult<Vec<PasskeyRecord>> {
        Self::service()?
            .passkey_store()
            .records("user_id", Self::key(parent)?)
            .await
    }
    pub async fn exists(&self, parent: &P) -> DatabaseResult<bool> {
        Self::service()?
            .passkey_store()
            .exists("user_id", Self::key(parent)?)
            .await
    }
    pub async fn create(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<PasskeyRecord> {
        Self::service()?
            .passkey_store()
            .create(attributes, Self::key(parent)?)
            .await
    }
}
