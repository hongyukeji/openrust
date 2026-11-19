//! Shared identity for the actual, selected Eloquent client model.
use crate::{ClientModel, PhpArray};
use openrust::database::{Connection, DatabaseFuture, DatabaseResult, Value, eloquent::Attributes};
use std::{any::Any, sync::Arc};
use tokio::sync::Mutex;

/// A shared client model, rather than a second attributes snapshot.
#[derive(Clone)]
pub struct ClientRecord(Arc<dyn LoadedClient>);
impl std::fmt::Debug for ClientRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClientRecord([MODEL])")
    }
}
#[allow(non_snake_case)]
impl ClientRecord {
    pub(crate) fn new<M: ClientModel>(model: M, connection: Connection) -> Self {
        Self(Arc::new(Loaded {
            model: Arc::new(Mutex::new(model)),
            connection,
        }))
    }
    /// Borrows the very same typed model for application-specific methods.
    pub fn model<M: ClientModel>(&self) -> Option<Arc<Mutex<M>>> {
        self.0
            .as_any()
            .downcast_ref::<Loaded<M>>()
            .map(|loaded| loaded.model.clone())
    }
    pub(crate) async fn connection(&self) -> DatabaseResult<Connection> {
        self.0.connection().await
    }
    pub(crate) async fn columns(&self) -> DatabaseResult<Vec<String>> {
        self.0.columns().await
    }
    pub(crate) async fn assign_and_save(&self, attributes: Attributes) -> DatabaseResult<bool> {
        self.0.assign_and_save(attributes).await
    }
    pub fn sameInstance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub async fn getAttributeValue(&self, key: &str) -> DatabaseResult<Option<Value>> {
        self.0.attribute(key).await
    }
    pub async fn getKey(&self) -> Option<Value> {
        self.0.key().await
    }
    pub async fn toArray(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        self.0.to_array().await
    }
    pub async fn confidential(&self) -> bool {
        self.0.confidential().await
    }
    pub async fn firstParty(&self) -> DatabaseResult<bool> {
        self.0.first_party().await
    }
    pub async fn hasGrantType(&self, grantType: &str) -> DatabaseResult<bool> {
        self.0.has_grant_type(grantType).await
    }
    pub async fn hasScope(&self, scope: &str) -> DatabaseResult<bool> {
        self.0.has_scope(scope).await
    }
    pub async fn redirectUris(&self) -> DatabaseResult<PhpArray> {
        self.0.redirect_uris().await
    }
}
trait LoadedClient: Send + Sync {
    fn connection(&self) -> DatabaseFuture<'_, DatabaseResult<Connection>>;
    fn has_scope<'a>(&'a self, scope: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>>;
    fn columns(&self) -> DatabaseFuture<'_, DatabaseResult<Vec<String>>>;
    fn assign_and_save(&self, attributes: Attributes) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn as_any(&self) -> &dyn Any;
    fn attribute<'a>(&'a self, key: &'a str) -> DatabaseFuture<'a, DatabaseResult<Option<Value>>>;
    fn key(&self) -> DatabaseFuture<'_, Option<Value>>;
    fn to_array(
        &self,
    ) -> DatabaseFuture<'_, DatabaseResult<serde_json::Map<String, serde_json::Value>>>;
    fn confidential(&self) -> DatabaseFuture<'_, bool>;
    fn first_party(&self) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn has_grant_type<'a>(&'a self, grant: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>>;
    fn redirect_uris(&self) -> DatabaseFuture<'_, DatabaseResult<PhpArray>>;
}
struct Loaded<M> {
    model: Arc<Mutex<M>>,
    connection: Connection,
}
impl<M: ClientModel> LoadedClient for Loaded<M> {
    fn connection(&self) -> DatabaseFuture<'_, DatabaseResult<Connection>> {
        Box::pin(async move { self.model.lock().await.getConnection() })
    }
    fn has_scope<'a>(&'a self, scope: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move { self.model.lock().await.hasScope(scope) })
    }
    fn columns(&self) -> DatabaseFuture<'_, DatabaseResult<Vec<String>>> {
        Box::pin(async move {
            let table = self.model.lock().await.getTable();
            self.connection
                .getSchemaBuilder()
                .getColumnListing(table)
                .await
        })
    }
    fn assign_and_save(&self, attributes: Attributes) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(async move {
            let mut model = self.model.lock().await;
            model.forceFillAsync(attributes).await?;
            model.save().await
        })
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn attribute<'a>(&'a self, key: &'a str) -> DatabaseFuture<'a, DatabaseResult<Option<Value>>> {
        Box::pin(async move { self.model.lock().await.getAttributeValue(key) })
    }
    fn key(&self) -> DatabaseFuture<'_, Option<Value>> {
        Box::pin(async move { self.model.lock().await.getKey().cloned() })
    }
    fn to_array(
        &self,
    ) -> DatabaseFuture<'_, DatabaseResult<serde_json::Map<String, serde_json::Value>>> {
        Box::pin(async move { self.model.lock().await.toArray() })
    }
    fn confidential(&self) -> DatabaseFuture<'_, bool> {
        Box::pin(async move { self.model.lock().await.confidential() })
    }
    fn first_party(&self) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(async move { self.model.lock().await.firstParty() })
    }
    fn has_grant_type<'a>(&'a self, grant: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move { self.model.lock().await.hasGrantType(grant) })
    }
    fn redirect_uris(&self) -> DatabaseFuture<'_, DatabaseResult<PhpArray>> {
        Box::pin(async move { self.model.lock().await.redirectUris() })
    }
}
