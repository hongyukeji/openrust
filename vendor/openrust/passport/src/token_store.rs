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
        let created = crate::events::AccessTokenCreated {
            token_id: event_id(&access, "id")?,
            user_id: access
                .get("user_id")
                .filter(|v| !matches!(v, Value::Null))
                .map(|v| {
                    openrust::auth::AuthIdentifier::try_from(v)
                        .map(|v| v.to_string())
                        .map_err(|_| openrust::database::DatabaseError::InvalidScalar)
                })
                .transpose()?,
            client_id: event_id(&access, "client_id")?,
        };
        tokens
            .persist(
                parent,
                transaction,
                access,
                &self.database,
                self.config.as_ref(),
            )
            .await?;
        openrust::support::facades::Event::dispatch_in_transaction(created, transaction)
            .await
            .map_err(event_error)?;
        if let Some(refresh) = refresh {
            let created = crate::events::RefreshTokenCreated {
                refresh_token_id: event_id(&refresh, "id")?,
                access_token_id: event_id(&refresh, "access_token_id")?,
            };
            refreshes
                .persist(
                    parent,
                    transaction,
                    refresh,
                    &self.database,
                    self.config.as_ref(),
                )
                .await?;
            openrust::support::facades::Event::dispatch_in_transaction(created, transaction)
                .await
                .map_err(event_error)?;
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
    pub(crate) fn parent(&self) -> DatabaseResult<Connection> {
        let name = crate::client::configured_connection(self.config.get("passport.connection"))?;
        self.database.connection(name.as_deref())
    }
    pub(crate) async fn is_access_revoked(&self, id: &str) -> DatabaseResult<bool> {
        let store = (self.token.read().expect("token model lock").factory)();
        store
            .is_revoked(&self.parent()?, &self.database, self.config.as_ref(), id)
            .await
    }
    pub(crate) async fn revoke_access(&self, id: &str) -> DatabaseResult<bool> {
        let store = (self.token.read().expect("token model lock").factory)();
        store
            .revoke_id(
                &self.parent()?,
                None,
                &self.database,
                self.config.as_ref(),
                id,
            )
            .await
    }
    pub(crate) async fn consume_refresh(
        &self,
        parent: &Connection,
        transaction: &mut openrust::database::Transaction,
        refresh_id: &str,
        access_id: &str,
    ) -> DatabaseResult<bool> {
        let refresh = (self.refresh.read().expect("refresh model lock").factory)();
        let tokens = (self.token.read().expect("token model lock").factory)();
        if !refresh
            .consume(
                parent,
                transaction,
                &self.database,
                self.config.as_ref(),
                refresh_id,
                access_id,
            )
            .await?
        {
            return Ok(false);
        }
        tokens
            .revoke_id(
                parent,
                Some(transaction),
                &self.database,
                self.config.as_ref(),
                access_id,
            )
            .await?;
        Ok(true)
    }
    pub(crate) async fn purge(
        &self,
        grant_models: &crate::grant_store::GrantModels,
        revoked: bool,
        cutoff: Option<String>,
        now: String,
        device: bool,
    ) -> DatabaseResult<()> {
        let parent = self.parent()?;
        let tokens = (self.token.read().expect("token model lock").factory)();
        let refresh = (self.refresh.read().expect("refresh model lock").factory)();
        tokens
            .purge(
                &parent,
                &self.database,
                self.config.as_ref(),
                refresh.clone(),
                PurgeRetention {
                    revoked,
                    cutoff: cutoff.clone(),
                    now,
                },
            )
            .await?;
        grant_models
            .auth()
            .purge(&parent, revoked, cutoff.clone())
            .await?;
        refresh
            .purge(
                &parent,
                &self.database,
                self.config.as_ref(),
                revoked,
                cutoff.clone(),
            )
            .await?;
        if device {
            grant_models
                .device()
                .purge(&parent, revoked, cutoff)
                .await?;
        }
        Ok(())
    }
    pub async fn revoke(&self, client: &ClientRecord) -> DatabaseResult<()> {
        let Some(id) = client.get_key().await else {
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
pub(crate) fn connection<M: Model>(
    parent: &Connection,
    database: &DatabaseManager,
    config: &dyn Repository,
) -> DatabaseResult<Connection> {
    let prototype = M::from_state(Default::default());
    let name = prototype
        .get_connection_name()
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
    fn is_revoked<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;
    fn revoke_id<'a>(
        &'a self,
        parent: &'a Connection,
        transaction: Option<&'a mut openrust::database::Transaction>,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;
    #[allow(
        clippy::too_many_arguments,
        reason = "Type-erased model dispatch carries its connection context and purge predicate"
    )]
    fn purge<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        refresh: Arc<dyn RefreshStore>,
        retention: PurgeRetention,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
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
    fn consume<'a>(
        &'a self,
        parent: &'a Connection,
        transaction: &'a mut openrust::database::Transaction,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: &'a str,
        access: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;

    fn active_relation(
        &self,
        connection: &Connection,
        parent_table: &str,
        now: String,
    ) -> DatabaseResult<openrust::database::query::Builder<'static>>;
    fn purge<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        revoked: bool,
        cutoff: Option<String>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
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
    fn is_revoked<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let mut query = M::query_on(&connection);
            query.where_key(id).r#where("revoked", "=", false);
            Ok(query.first().await?.is_none())
        })
    }
    fn revoke_id<'a>(
        &'a self,
        parent: &'a Connection,
        mut transaction: Option<&'a mut openrust::database::Transaction>,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let Some(mut model) = M::query_on(&connection).find(id).await? else {
                return Ok(false);
            };
            let changed = if let Some(transaction) = transaction.as_deref_mut() {
                model.refresh_for_update(transaction).await?;
                model
                    .force_fill_async([("revoked".into(), true.into())].into())
                    .await?;
                if !model.save_in_transaction(transaction).await? {
                    return Err(openrust::database::DatabaseError::InvalidQuery(
                        "Passport revocation cancelled",
                    ));
                }
                true
            } else {
                let mut query = M::query_on(&connection);
                query.where_key(id);
                query
                    .update([("revoked".into(), true.into())].into())
                    .await?
                    > 0
            };
            if changed && let Some(transaction) = transaction {
                openrust::support::facades::Event::dispatch_in_transaction(
                    crate::events::AccessTokenRevoked {
                        token_id: id.into(),
                    },
                    transaction,
                )
                .await
                .map_err(event_error)?;
            }
            Ok(changed)
        })
    }
    fn purge<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        refresh: Arc<dyn RefreshStore>,
        retention: PurgeRetention,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let PurgeRetention {
                revoked,
                cutoff,
                now,
            } = retention;
            let relation = ActiveRefresh::<M> {
                refresh,
                now,
                marker: PhantomData,
            };
            constrained::<M>(&connection, revoked, cutoff)
                .where_doesnt_have(&relation)?
                .delete()
                .await?;
            Ok(())
        })
    }
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
            let mut model = M::new_instance_on(&connection, Default::default())?;
            model.force_fill_async(attributes).await?;
            if !model.save_in_transaction(transaction).await? {
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
            let mut query = M::query_on(&connection);
            query.r#where("client_id", "=", client);
            query.r#where("user_id", "=", user);
            query.r#where("revoked", "=", false);
            query.r#where("expires_at", ">", after);
            let mut scopes = Vec::new();
            for token in query.get().await? {
                scopes.push(match token.get_attribute_value("scopes")? {
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
            let key = M::from_state(Default::default()).get_key_name();
            let mut query = M::query_on(&connection);
            query.r#where("client_id", "=", id);
            query
                .order_by(key, "asc")
                .chunk(1000, |tokens, _| {
                    let connection = connection.clone();
                    async move {
                        let ids = tokens
                            .all()
                            .iter()
                            .filter_map(|m| m.get_key().cloned())
                            .collect();
                        let mut refreshes =
                            refresh.load(&connection, database, config, ids).await?;
                        for mut token in tokens {
                            if let Some(index) = refreshes
                                .iter()
                                .position(|(id, _)| Some(id) == token.get_key())
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
    fn consume<'a>(
        &'a self,
        parent: &'a Connection,
        transaction: &'a mut openrust::database::Transaction,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        id: &'a str,
        access: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            let Some(mut model) = M::query_on(&connection).find(id).await? else {
                return Ok(false);
            };
            model.refresh_for_update(transaction).await?;
            if model.get_attribute_value("revoked")? != Some(Value::Bool(false))
                || model.get_attribute_value("access_token_id")? != Some(access.into())
            {
                return Ok(false);
            }
            model
                .force_fill_async([("revoked".into(), true.into())].into())
                .await?;
            if !model.save_in_transaction(transaction).await? {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "Passport refresh consumption cancelled",
                ));
            }
            Ok(true)
        })
    }
    fn active_relation(
        &self,
        connection: &Connection,
        parent_table: &str,
        now: String,
    ) -> DatabaseResult<openrust::database::query::Builder<'static>> {
        use openrust::database::eloquent::relations::ExistenceRelation as _;
        let relation = openrust::database::eloquent::relations::HasOne::<crate::Token, M>::new(
            "id",
            "access_token_id",
        )
        .constrain(move |mut query| {
            query.r#where("revoked", "=", false).where_nested(|q| {
                q.where_null("expires_at")
                    .or_where("expires_at", ">", now.clone())
            });
            query
        });
        relation.get_relation_existence_query(connection, parent_table)
    }
    fn purge<'a>(
        &'a self,
        parent: &'a Connection,
        database: &'a DatabaseManager,
        config: &'a dyn Repository,
        revoked: bool,
        cutoff: Option<String>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let connection = connection::<M>(parent, database, config)?;
            constrained::<M>(&connection, revoked, cutoff)
                .delete()
                .await?;
            Ok(())
        })
    }
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
            let mut model = M::new_instance_on(&connection, Default::default())?;
            model.force_fill_async(attributes).await?;
            if !model.save_in_transaction(transaction).await? {
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
            let mut query = M::query_on(&connection);
            query.where_in("access_token_id", ids);
            let mut found = LoadedRefreshes::new();
            for model in query.get().await? {
                if let Some(id) = model.get_attribute_value("access_token_id")?
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

pub(crate) fn constrained<M: Model>(
    connection: &Connection,
    revoked: bool,
    cutoff: Option<String>,
) -> openrust::database::eloquent::Builder<'static, M> {
    let mut query = M::query_on(connection);
    query.where_nested(|mut predicate| {
        if revoked {
            predicate = predicate.or_where("revoked", "=", true);
        }
        if let Some(cutoff) = cutoff {
            predicate = predicate.or_where("expires_at", "<", cutoff);
        }
        predicate
    });
    query
}
struct ActiveRefresh<M> {
    refresh: Arc<dyn RefreshStore>,
    now: String,
    marker: PhantomData<M>,
}
impl<M: Model> openrust::database::eloquent::relations::Relation for ActiveRefresh<M> {
    type Parent = M;
    type Output = ();
    fn match_results<'a>(&'a self, _: &'a [&'a M]) -> DatabaseFuture<'a, DatabaseResult<Vec<()>>> {
        Box::pin(async {
            Err(openrust::database::DatabaseError::InvalidQuery(
                "purge relation supports existence queries only",
            ))
        })
    }
}
impl<M: Model> openrust::database::eloquent::relations::ExistenceRelation for ActiveRefresh<M> {
    fn get_relation_existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<openrust::database::query::Builder<'static>> {
        self.refresh
            .active_relation(connection, parent_table, self.now.clone())
    }
}

struct PurgeRetention {
    revoked: bool,
    cutoff: Option<String>,
    now: String,
}

fn event_id(
    attributes: &openrust::database::eloquent::Attributes,
    key: &str,
) -> DatabaseResult<String> {
    attributes
        .get(key)
        .ok_or(openrust::database::DatabaseError::InvalidScalar)
        .and_then(|value| {
            openrust::auth::AuthIdentifier::try_from(value)
                .map(|v| v.to_string())
                .map_err(|_| openrust::database::DatabaseError::InvalidScalar)
        })
}
fn event_error(error: openrust::events::EventError) -> openrust::database::DatabaseError {
    openrust::database::DatabaseError::ModelEvent(Box::new(error))
}
