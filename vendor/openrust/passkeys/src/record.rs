//! Retains the actual selected Eloquent model across actions, responses and events.
use crate::{AuthenticationOptions, PasskeyModel, StoredCredential, WebAuthn, internal};
use openrust::{
    database::{Connection, DatabaseFuture, DatabaseResult, Transaction, Value},
    http::{HttpError, HttpFuture},
};
use std::{any::Any, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct PasskeyRecord(Arc<dyn LoadedPasskey>);
impl PasskeyRecord {
    pub fn new<M: PasskeyModel>(model: M) -> Self {
        Self(Arc::new(Loaded(Arc::new(Mutex::new(model)))))
    }
    pub fn model<M: PasskeyModel>(&self) -> Option<Arc<Mutex<M>>> {
        self.0
            .as_any()
            .downcast_ref::<Loaded<M>>()
            .map(|loaded| loaded.0.clone())
    }
    pub fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub async fn get_key(&self) -> Option<Value> {
        self.0.key().await
    }
    pub async fn get_attribute_value(&self, key: &str) -> DatabaseResult<Option<Value>> {
        self.0.attribute(key).await
    }
    pub async fn to_array(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        self.0.to_array().await
    }
    pub async fn credential(&self) -> crate::PasskeyResult<StoredCredential> {
        self.0.credential().await
    }
    pub(crate) async fn connection(&self) -> DatabaseResult<Connection> {
        self.0.connection().await
    }
    pub(crate) async fn delete(&self) -> DatabaseResult<()> {
        self.0.delete().await
    }
    pub(crate) async fn snapshot(&self) -> Box<dyn Any + Send + Sync> {
        self.0.snapshot().await
    }
    pub(crate) async fn verify(
        &self,
        value: serde_json::Value,
        options: &AuthenticationOptions,
        web: &WebAuthn,
        expected: Option<&Value>,
        transaction: &mut Transaction,
    ) -> Result<Value, HttpError> {
        self.0
            .verify(value, options, web, expected, transaction)
            .await
    }
}
trait LoadedPasskey: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn key(&self) -> DatabaseFuture<'_, Option<Value>>;
    fn attribute<'a>(&'a self, key: &'a str) -> DatabaseFuture<'a, DatabaseResult<Option<Value>>>;
    fn to_array(
        &self,
    ) -> DatabaseFuture<'_, DatabaseResult<serde_json::Map<String, serde_json::Value>>>;
    fn credential(&self) -> DatabaseFuture<'_, crate::PasskeyResult<StoredCredential>>;
    fn connection(&self) -> DatabaseFuture<'_, DatabaseResult<Connection>>;
    fn delete(&self) -> DatabaseFuture<'_, DatabaseResult<()>>;
    fn snapshot(&self) -> DatabaseFuture<'_, Box<dyn Any + Send + Sync>>;
    fn verify<'a>(
        &'a self,
        value: serde_json::Value,
        options: &'a AuthenticationOptions,
        web: &'a WebAuthn,
        expected: Option<&'a Value>,
        transaction: &'a mut Transaction,
    ) -> HttpFuture<'a, Result<Value, HttpError>>;
}
struct Loaded<M>(Arc<Mutex<M>>);
impl<M: PasskeyModel> LoadedPasskey for Loaded<M> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn key(&self) -> DatabaseFuture<'_, Option<Value>> {
        Box::pin(async move { self.0.lock().await.get_key().cloned() })
    }
    fn attribute<'a>(&'a self, key: &'a str) -> DatabaseFuture<'a, DatabaseResult<Option<Value>>> {
        Box::pin(async move { self.0.lock().await.get_attribute_value(key) })
    }
    fn to_array(
        &self,
    ) -> DatabaseFuture<'_, DatabaseResult<serde_json::Map<String, serde_json::Value>>> {
        Box::pin(async move { self.0.lock().await.to_array() })
    }
    fn credential(&self) -> DatabaseFuture<'_, crate::PasskeyResult<StoredCredential>> {
        Box::pin(async move { self.0.lock().await.credential() })
    }
    fn connection(&self) -> DatabaseFuture<'_, DatabaseResult<Connection>> {
        Box::pin(async move { self.0.lock().await.get_connection() })
    }
    fn delete(&self) -> DatabaseFuture<'_, DatabaseResult<()>> {
        Box::pin(async move { self.0.lock().await.delete().await.map(|_| ()) })
    }
    fn snapshot(&self) -> DatabaseFuture<'_, Box<dyn Any + Send + Sync>> {
        Box::pin(async move { Box::new(self.0.lock().await.clone()) as Box<dyn Any + Send + Sync> })
    }
    fn verify<'a>(
        &'a self,
        value: serde_json::Value,
        options: &'a AuthenticationOptions,
        web: &'a WebAuthn,
        expected: Option<&'a Value>,
        transaction: &'a mut Transaction,
    ) -> HttpFuture<'a, Result<Value, HttpError>> {
        Box::pin(async move {
            let mut model = self.0.lock().await;
            model
                .refresh_for_update(transaction)
                .await
                .map_err(internal)?;
            let (key, attributes) = crate::actions::verified_credential_attributes(
                &*model, value, options, web, expected,
            )?;
            model.force_fill(attributes).map_err(internal)?;
            model
                .save_in_transaction(transaction)
                .await
                .map_err(internal)?;
            Ok(key)
        })
    }
}
