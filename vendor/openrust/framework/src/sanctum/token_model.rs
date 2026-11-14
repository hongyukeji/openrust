use chrono::{DateTime, Utc};
use subtle::ConstantTimeEq as _;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model, casts::AttributeCast},
};

use super::PersonalAccessToken;
use super::personal_access_token::{integer, invalid_row, parse_timestamp};

type FoundModelToken = Option<(PersonalAccessToken, Box<dyn ModelToken>)>;

pub(super) trait ModelStore: Send + Sync {
    fn create(
        &self,
        attributes: Attributes,
    ) -> DatabaseFuture<'_, DatabaseResult<PersonalAccessToken>>;
    fn find<'a>(
        &'a self,
        id: Option<u64>,
        hash: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<FoundModelToken>>;
    fn revoke<'a>(
        &'a self,
        owner: &'a str,
        owner_id: u64,
        id: u64,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;
    fn prune(
        &self,
        column: &'static str,
        cutoff: DateTime<Utc>,
    ) -> DatabaseFuture<'_, DatabaseResult<()>>;
}

pub(super) trait ModelToken: Send + Sync {
    fn record_use(&mut self, now: DateTime<Utc>) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn last_used_at(&self) -> DatabaseResult<Option<DateTime<Utc>>>;
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

impl<M: Model> ModelStore for EloquentTokenStore<M> {
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
        id: Option<u64>,
        hash: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<FoundModelToken>> {
        Box::pin(async move {
            let mut query = self.query();
            if let Some(id) = id {
                query.whereKey(Value::Unsigned(id));
            } else {
                query.r#where("token", "=", hash);
            }
            let Some(model) = query.first().await? else {
                return Ok(None);
            };
            if !bool::from(text(&model, "token")?.as_bytes().ct_eq(hash.as_bytes())) {
                return Ok(None);
            }
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
        owner_id: u64,
        id: u64,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move {
            let mut query = self.query();
            query
                .whereKey(Value::Unsigned(id))
                .r#where("tokenable_type", "=", owner)
                .r#where("tokenable_id", "=", Value::Unsigned(owner_id));
            let Some(mut model) = query.first().await? else {
                return Ok(false);
            };
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

    fn last_used_at(&self) -> DatabaseResult<Option<DateTime<Utc>>> {
        date(&self.model, "last_used_at")
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
        id: integer(model.getKey().ok_or_else(invalid_row)?)?,
        tokenable_type: text(model, "tokenable_type")?,
        tokenable_id: integer(
            &model
                .getAttributeValue("tokenable_id")?
                .ok_or_else(invalid_row)?,
        )?,
        name: text(model, "name")?,
        abilities,
        created_at: date(model, "created_at")?,
        last_used_at: date(model, "last_used_at")?,
        expires_at: date(model, "expires_at")?,
    })
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
