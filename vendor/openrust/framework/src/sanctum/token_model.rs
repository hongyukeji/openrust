use chrono::{DateTime, Utc};
use subtle::ConstantTimeEq as _;

use crate::auth::AuthIdentifier;
use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model, ModelMetadata, ModelState, casts::AttributeCast},
};

use super::PersonalAccessToken;
use super::personal_access_token::{integer_key, invalid_row, parse_timestamp, token_hash};

pub(super) type SerializedModel = Result<serde_json::Map<String, serde_json::Value>, String>;
type FoundModelToken = Option<(PersonalAccessToken, Box<dyn ModelToken>)>;

/// An Eloquent token model with an overridable Sanctum credential lookup.
#[allow(non_snake_case, reason = "Matches Sanctum's token model API")]
pub trait PersonalAccessTokenModel: Model {
    fn findToken<'a>(
        query: Builder<'static, Self>,
        connection: &'a Connection,
        credential: &'a str,
    ) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send + 'a {
        Self::findTokenByDigest(query, connection, credential)
    }

    /// Standard lookup for overrides that retain Sanctum's SHA-256 credential verification.
    fn findTokenByDigest<'a>(
        mut query: Builder<'static, Self>,
        connection: &'a Connection,
        credential: &'a str,
    ) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send + 'a {
        async move {
            let digest = if let Some((id, secret)) = credential.split_once('|') {
                if id.is_empty() || secret.is_empty() {
                    return Ok(None);
                }
                let Some(key) = credential_key::<Self>(id, connection) else {
                    return Ok(None);
                };
                query.whereKey(key);
                token_hash(secret)
            } else if credential.is_empty() {
                return Ok(None);
            } else {
                let digest = token_hash(credential);
                query.r#where("token", "=", digest.as_str());
                digest
            };
            let Some(model) = query.first().await? else {
                return Ok(None);
            };
            if !bool::from(text(&model, "token")?.as_bytes().ct_eq(digest.as_bytes())) {
                return Ok(None);
            }
            Ok(Some(model))
        }
    }
}

/// The Eloquent model used for typed queries against Sanctum's default table.
pub struct DefaultPersonalAccessToken(ModelState);

impl Model for DefaultPersonalAccessToken {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata =
            ModelMetadata::new("personal_access_tokens").hidden(&["token"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("abilities", AttributeCast::Json),
            ("last_used_at", AttributeCast::DateTime),
            ("expires_at", AttributeCast::DateTime),
        ]
    }
}

impl PersonalAccessTokenModel for DefaultPersonalAccessToken {}

fn credential_key<M: Model>(id: &str, connection: &Connection) -> Option<Value> {
    if M::getIncrementing() {
        integer_key(id, connection)
    } else {
        let key = M::from_state(Default::default()).getKeyName();
        M::routeBindingValue(key, id)
    }
}

pub(super) trait ModelStore: Send + Sync {
    fn integer_key(&self) -> bool;
    fn create(
        &self,
        attributes: Attributes,
    ) -> DatabaseFuture<'_, DatabaseResult<PersonalAccessToken>>;
    fn find<'a>(
        &'a self,
        credential: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<FoundModelToken>>;
    fn revoke<'a>(
        &'a self,
        owner: &'a str,
        owner_id: AuthIdentifier,
        id: AuthIdentifier,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;
    fn related<'a>(
        &'a self,
        owner: &'a str,
        owner_id: AuthIdentifier,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<PersonalAccessToken>>>;
    fn delete_related<'a>(
        &'a self,
        owner: &'a str,
        owner_id: AuthIdentifier,
        id: Option<AuthIdentifier>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>>;
    fn prune(
        &self,
        column: &'static str,
        cutoff: DateTime<Utc>,
    ) -> DatabaseFuture<'_, DatabaseResult<()>>;
}

pub(super) trait ModelToken: Send + Sync {
    fn record_use(&mut self, now: DateTime<Utc>) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn serialized_model(&self) -> SerializedModel;
    fn last_used_at(&self) -> DatabaseResult<Option<DateTime<Utc>>>;
    fn updated_at(&self) -> DatabaseResult<Option<DateTime<Utc>>>;
}

pub(super) struct EloquentTokenStore<M> {
    connection: Connection,
    model: std::marker::PhantomData<M>,
}

impl<M: Model> EloquentTokenStore<M> {
    pub(super) fn new(connection: Connection) -> Self {
        Self {
            connection,
            model: std::marker::PhantomData,
        }
    }

    pub(super) fn owner_query(&self, owner: &str, owner_id: AuthIdentifier) -> Builder<'static, M> {
        let query = self
            .query()
            .require_relation_value("tokenable_type", owner)
            .require_mysql_morph_alias(owner)
            .require_relation_value("tokenable_id", Value::from(owner_id.clone()));
        if self.connection.is_sqlite() {
            query
        } else {
            query.require_relation_conditions(|conditions| {
                conditions.whereRaw(
                    "BINARY CAST(tokenable_id AS CHAR) = BINARY ?",
                    vec![owner_id.to_string().into()],
                )
            })
        }
    }

    fn query(&self) -> Builder<'static, M> {
        M::queryOn(&self.connection).useWritePdo().withCasts(
            [
                ("abilities", AttributeCast::Json),
                ("expires_at", AttributeCast::DateTime),
                ("last_used_at", AttributeCast::DateTime),
            ]
            .into_iter()
            .filter(|(name, _)| !M::casts().iter().any(|(column, _)| column == name)),
        )
    }
}

impl<M: PersonalAccessTokenModel> ModelStore for EloquentTokenStore<M> {
    fn integer_key(&self) -> bool {
        M::getIncrementing()
    }

    fn create(
        &self,
        attributes: Attributes,
    ) -> DatabaseFuture<'_, DatabaseResult<PersonalAccessToken>> {
        Box::pin(async move {
            let mut model = self.query().makeAsync(Attributes::new()).await?;
            for (column, value) in attributes {
                model.setAttributeValueAsync(&column, value).await?;
            }
            if !model.save().await? {
                return Err(DatabaseError::InvalidQuery(
                    "personal access token creation cancelled",
                ));
            }
            snapshot(&model)
        })
    }

    fn find<'a>(
        &'a self,
        credential: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<FoundModelToken>> {
        Box::pin(async move {
            let Some(model) = M::findToken(self.query(), &self.connection, credential).await?
            else {
                return Ok(None);
            };
            let token = snapshot(&model)?;
            Ok(Some((
                token,
                Box::new(LoadedModel {
                    model,
                    connection: self.connection.clone(),
                }) as Box<dyn ModelToken>,
            )))
        })
    }

    fn revoke<'a>(
        &'a self,
        owner: &'a str,
        owner_id: AuthIdentifier,
        id: AuthIdentifier,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move {
            let mut query = self.query();
            let Some(key) = credential_key::<M>(&id.to_string(), &self.connection) else {
                return Ok(false);
            };
            query
                .whereKey(key)
                .r#where("tokenable_type", "=", owner)
                .r#where("tokenable_id", "=", Value::from(owner_id.clone()));
            let Some(mut model) = query.first().await? else {
                return Ok(false);
            };
            let stored_owner = model
                .getRawOriginal("tokenable_id")
                .ok_or_else(invalid_row)?;
            if identifier(stored_owner)? != identifier(&owner_id.into())?
                || model.getRawOriginal("tokenable_type") != Some(&Value::from(owner))
            {
                return Ok(false);
            }
            let mut transaction = self.connection.beginTransaction().await?;
            let deleted = lock_token(&mut transaction, &model).await?
                && model.deleteInTransaction(&mut transaction).await?;
            if deleted {
                transaction.commit().await?;
            } else {
                transaction.rollBack(None).await?;
            }
            Ok(deleted)
        })
    }

    fn related<'a>(
        &'a self,
        owner: &'a str,
        owner_id: AuthIdentifier,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<PersonalAccessToken>>> {
        Box::pin(async move {
            self.owner_query(owner, owner_id)
                .get()
                .await?
                .iter()
                .map(snapshot::<M>)
                .collect()
        })
    }

    fn delete_related<'a>(
        &'a self,
        owner: &'a str,
        owner_id: AuthIdentifier,
        id: Option<AuthIdentifier>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(async move {
            let mut query = self.owner_query(owner, owner_id);
            if let Some(id) = id {
                let Some(key) = credential_key::<M>(&id.to_string(), &self.connection) else {
                    return Ok(0);
                };
                query.whereKey(key);
            }
            query.delete().await
        })
    }

    fn prune(
        &self,
        column: &'static str,
        cutoff: DateTime<Utc>,
    ) -> DatabaseFuture<'_, DatabaseResult<()>> {
        Box::pin(async move {
            let mut query = self.query();
            query.r#where(
                column,
                "<",
                cutoff
                    .with_timezone(&self.connection.model_timezone())
                    .format(super::personal_access_token::TIMESTAMP)
                    .to_string(),
            );
            query.delete().await?;
            Ok(())
        })
    }
}

struct LoadedModel<M> {
    model: M,
    connection: Connection,
}

impl<M: Model> ModelToken for LoadedModel<M> {
    fn record_use(&mut self, now: DateTime<Utc>) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(async move {
            let modified = self.connection.hasModifiedRecords();
            let result = async {
                let mut transaction = self.connection.beginTransaction().await?;
                if !lock_token(&mut transaction, &self.model).await? {
                    transaction.rollBack(None).await?;
                    return Ok(false);
                }
                self.model
                    .setAttributeValueAsync("last_used_at", now.to_rfc3339())
                    .await?;
                if self.model.saveInTransaction(&mut transaction).await? {
                    transaction.commit().await?;
                } else {
                    // A model observer may suppress the timestamp update without revoking the token.
                    transaction.rollBack(None).await?;
                }
                Ok(true)
            }
            .await;
            self.connection.setRecordModificationState(modified);
            result
        })
    }

    fn serialized_model(&self) -> SerializedModel {
        serialized_model(&self.model)
    }

    fn last_used_at(&self) -> DatabaseResult<Option<DateTime<Utc>>> {
        date(&self.model, "last_used_at")
    }

    fn updated_at(&self) -> DatabaseResult<Option<DateTime<Utc>>> {
        date(&self.model, "updated_at")
    }
}

async fn lock_token<M: Model>(transaction: &mut Transaction, model: &M) -> DatabaseResult<bool> {
    let key = model.getKeyName();
    let mut query = transaction.table(model.getTable());
    for column in [key, "tokenable_type", "tokenable_id", "token"] {
        let value = model.getRawOriginal(column).ok_or_else(invalid_row)?;
        query = query.r#where(column, "=", value.clone());
    }
    Ok(query.select([key]).lockForUpdate().first().await?.is_some())
}

fn snapshot<M: Model>(model: &M) -> DatabaseResult<PersonalAccessToken> {
    let abilities = match model.getAttributeValue("abilities")? {
        Some(Value::Json(abilities)) => {
            serde_json::from_value(abilities).map_err(|_| invalid_row())?
        }
        Some(Value::Text(abilities)) => {
            serde_json::from_str(&abilities).map_err(|_| invalid_row())?
        }
        Some(Value::Null) => Vec::new(),
        _ => return Err(invalid_row()),
    };
    Ok(PersonalAccessToken {
        id: token_key(model.getKey().ok_or_else(invalid_row)?)?,
        tokenable_type: text(model, "tokenable_type")?,
        tokenable_id: identifier(
            &model
                .getAttributeValue("tokenable_id")?
                .ok_or_else(invalid_row)?,
        )?,
        name: text(model, "name")?,
        abilities,
        created_at: date(model, "created_at")?,
        updated_at: date(model, "updated_at")?,
        last_used_at: date(model, "last_used_at")?,
        expires_at: date(model, "expires_at")?,
        serialized_model: serialized_model(model),
    })
}

fn serialized_model<M: Model>(model: &M) -> SerializedModel {
    let mut values = model.toArray().map_err(|error| error.to_string())?;
    values.remove("token");
    Ok(values)
}

fn identifier(value: &Value) -> DatabaseResult<AuthIdentifier> {
    match value {
        Value::Integer(value) if *value >= 0 => Ok(AuthIdentifier::UnsignedInteger(*value as u64)),
        _ => AuthIdentifier::try_from(value).map_err(|_| invalid_row()),
    }
}

fn token_key(value: &Value) -> DatabaseResult<AuthIdentifier> {
    match value {
        Value::Text(key) if key.is_empty() || key.contains('|') => Err(invalid_row()),
        _ => identifier(value),
    }
}

fn text<M: Model>(model: &M, column: &str) -> DatabaseResult<String> {
    match model.getAttributeValue(column)? {
        Some(Value::Text(text)) => Ok(text),
        _ => Err(invalid_row()),
    }
}

fn date<M: Model>(model: &M, column: &str) -> DatabaseResult<Option<DateTime<Utc>>> {
    match model.getAttributeValue(column)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => parse_timestamp(&value).map(Some),
        Some(Value::Integer(value)) => DateTime::from_timestamp(value, 0)
            .map(Some)
            .ok_or_else(invalid_row),
        _ => Err(invalid_row()),
    }
}
