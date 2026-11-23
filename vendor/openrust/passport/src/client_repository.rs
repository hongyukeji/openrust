//! Lifecycle-scoped client lookup using the selected Eloquent model.
use crate::{ClientModel, ClientRecord};
use openrust::{
    contracts::config::Repository,
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        eloquent::{Attributes, Model},
    },
};
use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tokio::sync::OnceCell;

/// A typed user's morph identity, retained without serializing the user model.
#[derive(Clone, Debug)]
pub struct ClientOwner {
    id: Value,
    morph_class: &'static str,
}
#[allow(non_snake_case)]
impl ClientOwner {
    pub fn forUser<U: Model + openrust::contracts::auth::Authenticatable>(
        user: &U,
    ) -> DatabaseResult<Self> {
        Ok(Self {
            id: user.getKey().cloned().unwrap_or(Value::Null),
            morph_class: user.getMorphClass()?,
        })
    }
}

#[derive(Debug)]
pub enum ClientRepositoryError {
    Database(DatabaseError),
    PersonalAccessClientNotFound { provider: String },
}
impl std::fmt::Display for ClientRepositoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => error.fmt(f),
            Self::PersonalAccessClientNotFound { provider } => write!(
                f,
                "Personal access client not found for '{provider}' user provider. Please create one."
            ),
        }
    }
}
impl std::error::Error for ClientRepositoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}
impl From<DatabaseError> for ClientRepositoryError {
    fn from(error: DatabaseError) -> Self {
        Self::Database(error)
    }
}

pub(crate) struct NewClient {
    name: String,
    grants: Vec<&'static str>,
    redirects: Vec<String>,
    provider: Option<String>,
    confidential: bool,
    owner: Option<ClientOwner>,
}

type CachedClient = Arc<OnceCell<Option<ClientRecord>>>;
/// Resolve through the container inside a request or Artisan lifecycle.
/// Concurrent lookups of the same identifier retain the same model object.
pub struct ClientRepository {
    store: Arc<dyn ClientStore>,
    tokens: Arc<crate::token_store::TokenModels>,
    cache: Mutex<HashMap<String, CachedClient>>,
}
#[allow(non_snake_case)]
impl ClientRepository {
    pub(crate) fn new(
        store: Arc<dyn ClientStore>,
        tokens: Arc<crate::token_store::TokenModels>,
    ) -> Self {
        Self {
            store,
            tokens,
            cache: Mutex::default(),
        }
    }
    pub async fn createPersonalAccessGrantClient(
        &self,
        name: &str,
        provider: Option<&str>,
    ) -> DatabaseResult<ClientRecord> {
        self.create(name, vec!["personal_access"], &[], provider, true, None)
            .await
    }
    pub async fn createPasswordGrantClient(
        &self,
        name: &str,
        provider: Option<&str>,
        confidential: bool,
    ) -> DatabaseResult<ClientRecord> {
        self.create(
            name,
            vec!["password", "refresh_token"],
            &[],
            provider,
            confidential,
            None,
        )
        .await
    }
    pub async fn createClientCredentialsGrantClient(
        &self,
        name: &str,
    ) -> DatabaseResult<ClientRecord> {
        self.create(name, vec!["client_credentials"], &[], None, true, None)
            .await
    }
    pub async fn createImplicitGrantClient(
        &self,
        name: &str,
        redirectUris: &[String],
    ) -> DatabaseResult<ClientRecord> {
        self.create(name, vec!["implicit"], redirectUris, None, false, None)
            .await
    }
    pub async fn createDeviceAuthorizationGrantClient(
        &self,
        name: &str,
        confidential: bool,
        user: Option<&ClientOwner>,
    ) -> DatabaseResult<ClientRecord> {
        self.create(
            name,
            vec![
                "urn:ietf:params:oauth:grant-type:device_code",
                "refresh_token",
            ],
            &[],
            None,
            confidential,
            user,
        )
        .await
    }
    pub async fn createAuthorizationCodeGrantClient(
        &self,
        name: &str,
        redirectUris: &[String],
        confidential: bool,
        user: Option<&ClientOwner>,
        enableDeviceFlow: bool,
    ) -> DatabaseResult<ClientRecord> {
        let mut grants = vec!["authorization_code", "refresh_token"];
        if enableDeviceFlow {
            grants.push("urn:ietf:params:oauth:grant-type:device_code");
        }
        self.create(name, grants, redirectUris, None, confidential, user)
            .await
    }
    async fn create(
        &self,
        name: &str,
        grants: Vec<&'static str>,
        redirects: &[String],
        provider: Option<&str>,
        confidential: bool,
        owner: Option<&ClientOwner>,
    ) -> DatabaseResult<ClientRecord> {
        self.store
            .create(NewClient {
                name: name.into(),
                grants,
                redirects: redirects.to_vec(),
                provider: provider.map(str::to_owned),
                confidential,
                owner: owner.cloned(),
            })
            .await
    }
    pub async fn update(
        &self,
        client: &ClientRecord,
        name: &str,
        redirectUris: &[String],
    ) -> DatabaseResult<bool> {
        let columns = client.columns().await?;
        let redirects = if columns.iter().any(|column| column == "redirect_uris") {
            (
                "redirect_uris",
                Value::Json(serde_json::json!(redirectUris)),
            )
        } else {
            ("redirect", Value::Text(redirectUris.join(",")))
        };
        client
            .assign_and_save(Attributes::from([
                ("name".into(), name.into()),
                (redirects.0.into(), redirects.1),
            ]))
            .await
    }
    pub async fn delete(&self, client: &ClientRecord) -> DatabaseResult<()> {
        self.tokens.revoke(client).await?;
        client
            .assign_and_save(Attributes::from([("revoked".into(), true.into())]))
            .await?;
        Ok(())
    }
    pub async fn regenerateSecret(&self, client: &ClientRecord) -> DatabaseResult<bool> {
        client
            .assign_and_save(Attributes::from([(
                "secret".into(),
                openrust::support::Str::random(40).into(),
            )]))
            .await
    }
    pub async fn findForUser(
        &self,
        clientId: impl Into<Value>,
        user: &ClientOwner,
    ) -> DatabaseResult<Option<ClientRecord>> {
        Ok(self
            .store
            .for_user(user.clone(), Some(clientId.into()))
            .await?
            .into_iter()
            .next())
    }
    pub async fn forUser(
        &self,
        user: &ClientOwner,
    ) -> DatabaseResult<openrust::support::Collection<ClientRecord>> {
        self.store
            .for_user(user.clone(), None)
            .await
            .map(|models| models.into_iter().collect())
    }
    pub async fn personalAccessClient(
        &self,
        provider: &str,
    ) -> Result<ClientRecord, ClientRepositoryError> {
        self.store.personal_access(provider).await?.ok_or_else(|| {
            ClientRepositoryError::PersonalAccessClientNotFound {
                provider: provider.into(),
            }
        })
    }
    pub async fn find(&self, id: impl Into<Value>) -> DatabaseResult<Option<ClientRecord>> {
        let id = id.into();
        if !self.store.valid_id(&id) {
            return Ok(None);
        }
        let cell = self
            .cache
            .lock()
            .expect("Passport client lookup cache poisoned")
            .entry(format!("{id:?}"))
            .or_default()
            .clone();
        cell.get_or_try_init(|| self.store.find(id)).await.cloned()
    }
    pub async fn findActive(&self, id: impl Into<Value>) -> DatabaseResult<Option<ClientRecord>> {
        let Some(client) = self.find(id).await? else {
            return Ok(None);
        };
        if crate::client::truthy(client.getAttributeValue("revoked").await?.as_ref()) {
            Ok(None)
        } else {
            Ok(Some(client))
        }
    }
}
pub(crate) trait ClientStore: Send + Sync {
    fn for_user(
        &self,
        user: ClientOwner,
        id: Option<Value>,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<ClientRecord>>>;
    fn personal_access<'a>(
        &'a self,
        provider: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<ClientRecord>>>;
    fn create(&self, attributes: NewClient) -> DatabaseFuture<'_, DatabaseResult<ClientRecord>>;
    fn valid_id(&self, id: &Value) -> bool;
    fn find(&self, id: Value) -> DatabaseFuture<'_, DatabaseResult<Option<ClientRecord>>>;
}
pub(crate) struct EloquentClientStore<M> {
    database: Arc<DatabaseManager>,
    config: Arc<dyn Repository>,
    model: PhantomData<M>,
}
impl<M: ClientModel> EloquentClientStore<M> {
    pub(crate) fn new(database: Arc<DatabaseManager>, config: Arc<dyn Repository>) -> Self {
        Self {
            database,
            config,
            model: PhantomData,
        }
    }
    fn connection(&self) -> DatabaseResult<Connection> {
        let prototype = M::from_state(Default::default());
        let name = if let Some(name) = prototype.getConnectionName() {
            Some(name.to_owned())
        } else {
            crate::client::configured_connection(self.config.get("passport.connection"))?
        };
        let connection = self.database.connection(
            name.as_deref()
                .filter(|name| !name.is_empty() && *name != "0"),
        )?;
        Ok(connection)
    }
}
impl<M: ClientModel> ClientStore for EloquentClientStore<M> {
    fn for_user(
        &self,
        user: ClientOwner,
        id: Option<Value>,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<ClientRecord>>> {
        Box::pin(async move {
            let connection = self.connection()?;
            let prototype = M::from_state(Default::default());
            let columns = connection
                .getSchemaBuilder()
                .getColumnListing(prototype.getTable())
                .await?;
            let legacy = columns.iter().any(|column| column == "user_id");
            let owner_key = if legacy { "user_id" } else { "owner_id" };
            let mut query = M::newQuery(&connection);
            query
                .r#where("revoked", "=", false)
                .r#where(owner_key, "=", user.id)
                .whereNested(|conditions| conditions.whereNotNull(owner_key));
            if !legacy {
                query.r#where("owner_type", "=", user.morph_class);
            }
            if let Some(id) = id {
                return query.find(id).await.map(|model| {
                    model
                        .into_iter()
                        .map(|model| ClientRecord::new(model, connection.clone()))
                        .collect()
                });
            }
            query.orderBy("name", "asc").get().await.map(|models| {
                models
                    .into_iter()
                    .map(|model| ClientRecord::new(model, connection.clone()))
                    .collect()
            })
        })
    }
    fn personal_access<'a>(
        &'a self,
        provider: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<ClientRecord>>> {
        Box::pin(async move {
            let connection = self.connection()?;
            let default_provider = self
                .config
                .get("auth.guards.api.provider")
                .as_ref()
                .and_then(|value| value.as_str())
                == Some(provider);
            let prototype = M::from_state(Default::default());
            let created = prototype
                .getCreatedAtColumn()
                .unwrap_or("created_at")
                .to_owned();
            let mut query = M::newQuery(&connection);
            query
                .r#where("revoked", "=", false)
                .whereNested(|conditions| {
                    let conditions = if default_provider {
                        conditions.orWhereNull("provider")
                    } else {
                        conditions
                    };
                    conditions.orWhere("provider", "=", provider)
                });
            for model in query.orderBy(&created, "desc").get().await? {
                if model.hasGrantType("personal_access")? {
                    return Ok(Some(ClientRecord::new(model, connection)));
                }
            }
            Ok(None)
        })
    }

    fn create(&self, client: NewClient) -> DatabaseFuture<'_, DatabaseResult<ClientRecord>> {
        Box::pin(async move {
            let connection = self.connection()?;
            let mut model = M::newQuery(&connection)
                .make_async(Attributes::new())
                .await?;
            let columns = connection
                .getSchemaBuilder()
                .getColumnListing(model.getTable())
                .await?;
            let has = |column: &str| columns.iter().any(|name| name == column);
            let mut attributes = Attributes::from([
                ("name".into(), client.name.into()),
                (
                    "secret".into(),
                    if client.confidential {
                        openrust::support::Str::random(40).into()
                    } else {
                        Value::Null
                    },
                ),
                (
                    "provider".into(),
                    client.provider.map_or(Value::Null, Value::Text),
                ),
                ("revoked".into(), false.into()),
            ]);
            if has("redirect_uris") {
                attributes.insert(
                    "redirect_uris".into(),
                    Value::Json(serde_json::json!(client.redirects)),
                );
            } else {
                attributes.insert("redirect".into(), client.redirects.join(",").into());
            }
            if has("grant_types") {
                attributes.insert(
                    "grant_types".into(),
                    Value::Json(serde_json::json!(client.grants)),
                );
            } else {
                attributes.insert(
                    "personal_access_client".into(),
                    client.grants.contains(&"personal_access").into(),
                );
                attributes.insert(
                    "password_client".into(),
                    client.grants.contains(&"password").into(),
                );
            }
            if let Some(owner) = client.owner {
                if has("user_id") {
                    attributes.insert("user_id".into(), owner.id);
                } else {
                    attributes.insert("owner_id".into(), owner.id);
                    attributes.insert("owner_type".into(), owner.morph_class.into());
                }
            }
            model.forceFillAsync(attributes).await?;
            model.save().await?;
            Ok(ClientRecord::new(model, connection))
        })
    }
    fn valid_id(&self, id: &Value) -> bool {
        let prototype = M::from_state(Default::default());
        !M::uniqueIds().contains(&prototype.getKeyName())
            || matches!(id, Value::Text(id) if M::isValidUniqueId(id))
    }
    fn find(&self, id: Value) -> DatabaseFuture<'_, DatabaseResult<Option<ClientRecord>>> {
        Box::pin(async move {
            let connection = self.connection()?;
            M::newQuery(&connection)
                .find(id)
                .await
                .map(|model| model.map(|model| ClientRecord::new(model, connection)))
        })
    }
}
