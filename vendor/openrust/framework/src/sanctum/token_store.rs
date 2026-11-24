//! Sanctum 的 Eloquent 存储适配、事务锁及认证快照。

use super::personal_access_token::{integer_key, invalid_row, parse_timestamp};
use super::{PersonalAccessToken, PersonalAccessTokenModel};
use crate::auth::AuthIdentifier;
use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{Attributes, Builder, Model, casts::AttributeCast},
};
use jiff::Timestamp;

pub(super) type SerializedModel = Result<serde_json::Map<String, serde_json::Value>, String>;
type FoundModelToken = Option<(PersonalAccessToken, Box<dyn ModelToken>)>;

pub(super) fn credential_key<M: Model>(id: &str, connection: &Connection) -> Option<Value> {
    if M::get_incrementing() {
        integer_key(id, connection)
    } else {
        let key = M::from_state(Default::default()).get_key_name();
        M::route_binding_value(key, id)
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
        cutoff: Timestamp,
    ) -> DatabaseFuture<'_, DatabaseResult<()>>;
}

pub(super) trait ModelToken: Send + Sync {
    fn record_use(&mut self, now: Timestamp) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn serialized_model(&self) -> SerializedModel;
    fn last_used_at(&self) -> DatabaseResult<Option<Timestamp>>;
    fn updated_at(&self) -> DatabaseResult<Option<Timestamp>>;
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
        if !matches!(self.connection.get_driver_name(), "mysql" | "mariadb") {
            query
        } else {
            query.require_relation_conditions(|conditions| {
                conditions.where_raw(
                    "BINARY CAST(tokenable_id AS CHAR) = BINARY ?",
                    vec![owner_id.to_string().into()],
                )
            })
        }
    }

    fn query(&self) -> Builder<'static, M> {
        M::query_on(&self.connection).use_write_pdo().with_casts(
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
        M::get_incrementing()
    }

    fn create(
        &self,
        attributes: Attributes,
    ) -> DatabaseFuture<'_, DatabaseResult<PersonalAccessToken>> {
        Box::pin(async move {
            let mut model = self.query().make_async(Attributes::new()).await?;
            for (column, value) in attributes {
                model.set_attribute_value_async(&column, value).await?;
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
            let Some(model) = M::find_token(self.query(), &self.connection, credential).await?
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
                .where_key(key)
                .r#where("tokenable_type", "=", owner)
                .r#where("tokenable_id", "=", Value::from(owner_id.clone()));
            let Some(mut model) = query.first().await? else {
                return Ok(false);
            };
            let stored_owner = model
                .get_raw_original("tokenable_id")
                .ok_or_else(invalid_row)?;
            if identifier(stored_owner)? != identifier(&owner_id.into())?
                || model.get_raw_original("tokenable_type") != Some(&Value::from(owner))
            {
                return Ok(false);
            }
            let mut transaction = self.connection.begin_transaction().await?;
            let deleted = lock_token(&mut transaction, &model).await?
                && model.delete_in_transaction(&mut transaction).await?;
            if deleted {
                transaction.commit().await?;
            } else {
                transaction.roll_back(None).await?;
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
                query.where_key(key);
            }
            query.delete().await
        })
    }

    fn prune(
        &self,
        column: &'static str,
        cutoff: Timestamp,
    ) -> DatabaseFuture<'_, DatabaseResult<()>> {
        Box::pin(async move {
            let mut query = self.query();
            query.r#where(
                column,
                "<",
                cutoff
                    .to_zoned(self.connection.model_timezone())
                    .strftime(super::personal_access_token::TIMESTAMP)
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
    fn record_use(&mut self, now: Timestamp) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(async move {
            let modified = self.connection.has_modified_records();
            let result = async {
                let mut transaction = self.connection.begin_transaction().await?;
                if !lock_token(&mut transaction, &self.model).await? {
                    transaction.roll_back(None).await?;
                    return Ok(false);
                }
                self.model
                    .set_attribute_value_async(
                        "last_used_at",
                        super::personal_access_token::timestamp_text(now),
                    )
                    .await?;
                if self.model.save_in_transaction(&mut transaction).await? {
                    transaction.commit().await?;
                } else {
                    // 模型观察器可取消时间戳更新，但不会因此撤销令牌。
                    transaction.roll_back(None).await?;
                }
                Ok(true)
            }
            .await;
            self.connection.set_record_modification_state(modified);
            result
        })
    }

    fn serialized_model(&self) -> SerializedModel {
        serialized_model(&self.model)
    }

    fn last_used_at(&self) -> DatabaseResult<Option<Timestamp>> {
        date(&self.model, "last_used_at")
    }

    fn updated_at(&self) -> DatabaseResult<Option<Timestamp>> {
        date(&self.model, "updated_at")
    }
}

async fn lock_token<M: Model>(transaction: &mut Transaction, model: &M) -> DatabaseResult<bool> {
    let key = model.get_key_name();
    let mut query = transaction.table(model.get_table());
    for column in [key, "tokenable_type", "tokenable_id", "token"] {
        let value = model.get_raw_original(column).ok_or_else(invalid_row)?;
        query = query.r#where(column, "=", value.clone());
    }
    Ok(query
        .select([key])
        .lock_for_update()
        .first()
        .await?
        .is_some())
}

fn snapshot<M: Model>(model: &M) -> DatabaseResult<PersonalAccessToken> {
    let abilities = match model.get_attribute_value("abilities")? {
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
        id: token_key(model.get_key().ok_or_else(invalid_row)?)?,
        tokenable_type: text(model, "tokenable_type")?,
        tokenable_id: identifier(
            &model
                .get_attribute_value("tokenable_id")?
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
    let mut values = model.to_array().map_err(|error| error.to_string())?;
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

pub(super) fn text<M: Model>(model: &M, column: &str) -> DatabaseResult<String> {
    match model.get_attribute_value(column)? {
        Some(Value::Text(text)) => Ok(text),
        _ => Err(invalid_row()),
    }
}

fn date<M: Model>(model: &M, column: &str) -> DatabaseResult<Option<Timestamp>> {
    match model.get_attribute_value(column)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => parse_timestamp(&value).map(Some),
        Some(Value::Integer(value)) => Timestamp::from_second(value)
            .ok()
            .map(Some)
            .ok_or_else(invalid_row),
        _ => Err(invalid_row()),
    }
}
