//! Type-erased registration retains each actual Eloquent token model during revocation.
use crate::{ClientRecord, RefreshToken, RefreshTokenModel, Token, TokenModel};
use openrust::{
    contracts::config::Repository,
    database::{
        Connection, DatabaseFuture, DatabaseManager, DatabaseResult, Value, eloquent::Model,
    },
};
use std::{
    marker::PhantomData,
    sync::{Arc, RwLock},
};

type TokenFactory = fn() -> Arc<dyn TokenStore>;
type RefreshFactory = fn() -> Arc<dyn RefreshStore>;
struct Registration<F> {
    name: &'static str,
    factory: F,
}
pub(crate) struct TokenModels {
    token: RwLock<Registration<TokenFactory>>,
    refresh: RwLock<Registration<RefreshFactory>>,
    database: Arc<DatabaseManager>,
    config: Arc<dyn Repository>,
}
impl TokenModels {
    pub fn new(database: Arc<DatabaseManager>, config: Arc<dyn Repository>) -> Self {
        Self {
            token: RwLock::new(Registration {
                name: std::any::type_name::<Token>(),
                factory: token_store::<Token>,
            }),
            refresh: RwLock::new(Registration {
                name: std::any::type_name::<RefreshToken>(),
                factory: refresh_store::<RefreshToken>,
            }),
            database,
            config,
        }
    }
    pub fn use_token<M: TokenModel>(&self) {
        *self.token.write().expect("token model lock") = Registration {
            name: std::any::type_name::<M>(),
            factory: token_store::<M>,
        };
    }
    pub fn use_refresh<M: RefreshTokenModel>(&self) {
        *self.refresh.write().expect("refresh model lock") = Registration {
            name: std::any::type_name::<M>(),
            factory: refresh_store::<M>,
        };
    }
    pub fn token_name(&self) -> &'static str {
        self.token.read().expect("token model lock").name
    }
    pub fn refresh_name(&self) -> &'static str {
        self.refresh.read().expect("refresh model lock").name
    }
    pub(crate) async fn persist(
        &self,
        parent: &Connection,
        transaction: &mut openrust::database::Transaction,
        access: openrust::database::eloquent::Attributes,
        refresh: Option<openrust::database::eloquent::Attributes>,
    ) -> DatabaseResult<()> {
        let tokens = (self.token.read().expect("token model lock").factory)();
        let refreshes = (self.refresh.read().expect("refresh model lock").factory)();
        tokens
            .persist(
                parent,
                transaction,
                access,
                &self.database,
                self.config.as_ref(),
            )
            .await?;
        if let Some(refresh) = refresh {
            refreshes
                .persist(
                    parent,
                    transaction,
                    refresh,
                    &self.database,
                    self.config.as_ref(),
                )
                .await?;
        }
        Ok(())
    }
    pub(crate) async fn active_scopes(
        &self,
        parent: &Connection,
        client: Value,
        user: Value,
        after: String,
    ) -> DatabaseResult<Vec<crate::PhpArray>> {
        let tokens = (self.token.read().expect("token model lock").factory)();
        tokens
            .active_scopes(
                parent,
                &self.database,
                self.config.as_ref(),
                client,
                user,
                after,
            )
            .await
    }
    pub async fn revoke(&self, client: &ClientRecord) -> DatabaseResult<()> {
        let Some(id) = client.getKey().await else {
            return Ok(());
        };
        let parent = client.connection().await?;
        let tokens = (self.token.read().expect("token model lock").factory)();
        let refresh = (self.refresh.read().expect("refresh model lock").factory)();
        tokens
            .revoke(
                &parent,
                &self.database,
                self.config.as_ref(),
                id,
                refresh.as_ref(),
            )
            .await
    }
}
fn connection<M: Model>(
    parent: &Connection,
    database: &DatabaseManager,
    config: &dyn Repository,
) -> DatabaseResult<Connection> {
    let prototype = M::from_state(Default::default());
    let name = prototype
        .getConnectionName()
        .map(str::to_owned)
        .map(Some)
        .map(Ok)
        .unwrap_or_else(|| {
            crate::client::configured_connection(config.get("passport.connection"))
        })?;
    match name {
        Some(name) => database.connection(Some(&name)),
        None => Ok(parent.clone()),
    }
}
trait TokenStore: Send + Sync {
    fn persist<'a>(
        &'a self,
        parent: &'a Connection,
        transaction: &'a mut openrust::database::Transaction,
        attributes: openrust::database::eloquent::Attributes,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;

    fn active_scopes<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        client: Value,
        user: Value,
        after: String,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<crate::PhpArray>>>;
    fn revoke<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: Value,
        refresh: &'a dyn RefreshStore,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}
type LoadedRefreshes = Vec<(Value, Box<dyn RevocableRefresh>)>;
trait RefreshStore: Send + Sync {
    fn persist<'a>(
        &'a self,
        parent: &'a Connection,
        transaction: &'a mut openrust::database::Transaction,
        attributes: openrust::database::eloquent::Attributes,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;

    fn load<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        ids: Vec<Value>,
    ) -> DatabaseFuture<'a, DatabaseResult<LoadedRefreshes>>;
}
trait RevocableRefresh: Send + Sync {
    fn revoke(&mut self) -> DatabaseFuture<'_, DatabaseResult<bool>>;
}
struct Typed<M>(PhantomData<M>);
fn token_store<M: TokenModel>() -> Arc<dyn TokenStore> {
    Arc::new(Typed::<M>(PhantomData))
}
fn refresh_store<M: RefreshTokenModel>() -> Arc<dyn RefreshStore> {
    Arc::new(Typed::<M>(PhantomData))
}
impl<M: TokenModel> TokenStore for Typed<M> {
    fn persist<'a>(
        &'a self,
        parent: &'a Connection,
        transaction: &'a mut openrust::database::Transaction,
        attributes: openrust::database::eloquent::Attributes,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let mut model = M::newInstanceOn(&connection, Default::default())?;
            model.forceFillAsync(attributes).await?;
            if !model.saveInTransaction(transaction).await? {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "Passport token save cancelled",
                ));
            }
            Ok(())
        })
    }

    fn active_scopes<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        client: Value,
        user: Value,
        after: String,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<crate::PhpArray>>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let mut query = M::queryOn(&connection);
            query.r#where("client_id", "=", client);
            query.r#where("user_id", "=", user);
            query.r#where("revoked", "=", false);
            query.r#where("expires_at", ">", after);
            let mut scopes = Vec::new();
            for token in query.get().await? {
                scopes.push(match token.getAttributeValue("scopes")? {
                    Some(Value::Json(value)) => crate::PhpArray::try_from(value)?,
                    None | Some(Value::Null) => crate::PhpArray::default(),
                    _ => {
                        return Err(openrust::database::DatabaseError::InvalidQuery(
                            "Invalid Passport token scopes",
                        ));
                    }
                });
            }
            Ok(scopes)
        })
    }
    fn revoke<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: Value,
        refresh: &'a dyn RefreshStore,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let key = M::from_state(Default::default()).getKeyName();
            let mut query = M::queryOn(&connection);
            query.r#where("client_id", "=", id);
            query
                .orderBy(key, "asc")
                .chunk(1000, |tokens, _| {
                    let connection = connection.clone();
                    async move {
                        let ids = tokens
                            .all()
                            .iter()
                            .filter_map(|m| m.getKey().cloned())
                            .collect();
                        let mut refreshes =
                            refresh.load(&connection, database, config, ids).await?;
                        for mut token in tokens {
                            if let Some(index) = refreshes
                                .iter()
                                .position(|(id, _)| Some(id) == token.getKey())
                            {
                                let (_, mut refresh) = refreshes.remove(index);
                                refresh.revoke().await?;
                            }
                            token.revoke().await?;
                        }
                        Ok(true)
                    }
                })
                .await?;
            Ok(())
        })
    }
}
impl<M: RefreshTokenModel> RefreshStore for Typed<M> {
    fn persist<'a>(
        &'a self,
        parent: &'a Connection,
        transaction: &'a mut openrust::database::Transaction,
        attributes: openrust::database::eloquent::Attributes,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let mut model = M::newInstanceOn(&connection, Default::default())?;
            model.forceFillAsync(attributes).await?;
            if !model.saveInTransaction(transaction).await? {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "Passport token save cancelled",
                ));
            }
            Ok(())
        })
    }

    fn load<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        ids: Vec<Value>,
    ) -> DatabaseFuture<'a, DatabaseResult<LoadedRefreshes>> {
        Box::pin(async move {
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let connection = connection::<M>(parent, database, config)?;
            let mut query = M::queryOn(&connection);
            query.whereIn("access_token_id", ids);
            let mut found = LoadedRefreshes::new();
            for model in query.get().await? {
                if let Some(id) = model.getAttributeValue("access_token_id")?
                    && !found.iter().any(|(key, _)| *key == id)
                {
                    found.push((id, Box::new(LoadedRefresh(model))));
                }
            }
            Ok(found)
        })
    }
}
struct LoadedRefresh<M>(M);
impl<M: RefreshTokenModel> RevocableRefresh for LoadedRefresh<M> {
    fn revoke(&mut self) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(self.0.revoke())
    }
}
