//! Compiled authorization and device models retain their real Eloquent lifecycle.
use openrust::{
    contracts::config::Repository,
    database::{
        Connection, DatabaseFuture, DatabaseManager, DatabaseResult, Transaction, Value,
        eloquent::{Attributes, Model},
    },
};
use std::{
    marker::PhantomData,
    sync::{Arc, RwLock},
};

pub(crate) trait GrantRecord: Send + Sync {
    fn get_key(&self) -> Option<&Value>;
    fn attribute(&self, name: &str) -> Option<&Value>;
    fn value(&self, name: &str) -> DatabaseResult<Option<Value>>;
    fn fill(&mut self, attributes: Attributes) -> DatabaseResult<()>;
    fn fill_async(&mut self, attributes: Attributes) -> DatabaseFuture<'_, DatabaseResult<()>>;
    fn save(&mut self) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn save_in<'a>(
        &'a mut self,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;
    fn lock<'a>(
        &'a mut self,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}
struct Record<M>(M);
impl<M: Model> GrantRecord for Record<M> {
    fn get_key(&self) -> Option<&Value> {
        self.0.getKey()
    }
    fn attribute(&self, name: &str) -> Option<&Value> {
        self.0.getAttribute(name)
    }
    fn value(&self, name: &str) -> DatabaseResult<Option<Value>> {
        self.0.getAttributeValue(name)
    }
    fn fill(&mut self, attributes: Attributes) -> DatabaseResult<()> {
        self.0.forceFill(attributes).map(|_| ())
    }
    fn fill_async(&mut self, attributes: Attributes) -> DatabaseFuture<'_, DatabaseResult<()>> {
        Box::pin(async move { self.0.forceFillAsync(attributes).await.map(|_| ()) })
    }
    fn save(&mut self) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(self.0.save())
    }
    fn save_in<'a>(
        &'a mut self,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(self.0.saveInTransaction(transaction))
    }
    fn lock<'a>(
        &'a mut self,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move { self.0.refreshForUpdate(transaction).await.map(|_| ()) })
    }
}
pub(crate) trait GrantStore: Send + Sync {
    fn new_record(&self, parent: &Connection) -> DatabaseResult<Box<dyn GrantRecord>>;
    fn find<'a>(
        &'a self,
        parent: &'a Connection,
        id: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Box<dyn GrantRecord>>>>;
    fn pending<'a>(
        &'a self,
        parent: &'a Connection,
        code: String,
        after: String,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Box<dyn GrantRecord>>>>;
    fn purge<'a>(
        &'a self,
        parent: &'a Connection,
        revoked: bool,
        cutoff: Option<String>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}
struct Store<M> {
    database: Arc<DatabaseManager>,
    config: Arc<dyn Repository>,
    marker: PhantomData<M>,
}
impl<M: Model> Store<M> {
    fn connection(&self, parent: &Connection) -> DatabaseResult<Connection> {
        crate::token_store::connection::<M>(parent, &self.database, self.config.as_ref())
    }
}
fn erased<M: Model>(model: M) -> Box<dyn GrantRecord> {
    Box::new(Record(model))
}
impl<M: Model> GrantStore for Store<M> {
    fn new_record(&self, parent: &Connection) -> DatabaseResult<Box<dyn GrantRecord>> {
        M::newInstanceOn(&self.connection(parent)?, Attributes::new()).map(erased)
    }
    fn find<'a>(
        &'a self,
        parent: &'a Connection,
        id: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Box<dyn GrantRecord>>>> {
        Box::pin(async move {
            Ok(M::queryOn(&self.connection(parent)?)
                .find(id)
                .await?
                .map(erased))
        })
    }
    fn pending<'a>(
        &'a self,
        parent: &'a Connection,
        code: String,
        after: String,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Box<dyn GrantRecord>>>> {
        Box::pin(async move {
            let mut query = M::queryOn(&self.connection(parent)?);
            query
                .r#where("user_code", "=", code)
                .where_null("user_id")
                .r#where("revoked", "=", false)
                .r#where("expires_at", ">", after);
            Ok(query.first().await?.map(erased))
        })
    }
    fn purge<'a>(
        &'a self,
        parent: &'a Connection,
        revoked: bool,
        cutoff: Option<String>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            crate::token_store::constrained::<M>(&self.connection(parent)?, revoked, cutoff)
                .delete()
                .await?;
            Ok(())
        })
    }
}
type Factory = fn(Arc<DatabaseManager>, Arc<dyn Repository>) -> Arc<dyn GrantStore>;
fn store<M: Model>(
    database: Arc<DatabaseManager>,
    config: Arc<dyn Repository>,
) -> Arc<dyn GrantStore> {
    Arc::new(Store::<M> {
        database,
        config,
        marker: PhantomData,
    })
}
struct Registration {
    name: &'static str,
    factory: Factory,
}
pub(crate) struct GrantModels {
    auth: RwLock<Registration>,
    device: RwLock<Registration>,
    database: Arc<DatabaseManager>,
    config: Arc<dyn Repository>,
}
impl GrantModels {
    pub fn new(database: Arc<DatabaseManager>, config: Arc<dyn Repository>) -> Self {
        Self {
            auth: RwLock::new(registration::<crate::AuthCode>()),
            device: RwLock::new(registration::<crate::DeviceCode>()),
            database,
            config,
        }
    }
    pub fn use_auth<M: Model>(&self) {
        *self.auth.write().expect("auth code model lock") = registration::<M>();
    }
    pub fn use_device<M: Model>(&self) {
        *self.device.write().expect("device code model lock") = registration::<M>();
    }
    pub fn auth_name(&self) -> &'static str {
        self.auth.read().expect("auth code model lock").name
    }
    pub fn device_name(&self) -> &'static str {
        self.device.read().expect("device code model lock").name
    }
    pub fn auth(&self) -> Arc<dyn GrantStore> {
        (self.auth.read().expect("auth code model lock").factory)(
            self.database.clone(),
            self.config.clone(),
        )
    }
    pub fn device(&self) -> Arc<dyn GrantStore> {
        (self.device.read().expect("device code model lock").factory)(
            self.database.clone(),
            self.config.clone(),
        )
    }
}
fn registration<M: Model>() -> Registration {
    Registration {
        name: std::any::type_name::<M>(),
        factory: store::<M>,
    }
}
