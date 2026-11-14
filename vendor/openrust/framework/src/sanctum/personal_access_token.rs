use std::time::SystemTime;

use aws_lc_rs::{digest, rand};
use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

use crate::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    database::{Connection, DatabaseError, DatabaseResult, Row, Value, query::Record},
};

use super::token_model::{EloquentTokenStore, ModelStore, ModelToken, PersonalAccessTokenModel};

const TABLE: &str = "personal_access_tokens";
pub(super) const TIMESTAMP: &str = "%Y-%m-%d %H:%M:%S";

/// A persisted token. Its digest is kept in storage and its secret is only returned at issuance.
#[derive(Clone, serde::Serialize)]
pub struct PersonalAccessToken {
    pub id: AuthIdentifier,
    pub tokenable_type: String,
    pub tokenable_id: AuthIdentifier,
    pub name: String,
    pub abilities: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Sanctum token API")]
impl PersonalAccessToken {
    pub fn can(&self, ability: &str) -> bool {
        self.abilities
            .iter()
            .any(|value| value == "*" || value == ability)
    }

    pub fn cant(&self, ability: &str) -> bool {
        !self.can(ability)
    }

    pub fn isExpired(&self, now: DateTime<Utc>, expiration: Option<TimeDelta>) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
            || expiration.is_some_and(|lifetime| {
                self.created_at
                    .is_none_or(|created_at| now - created_at >= lifetime)
            })
    }
}

/// A token and the credential to return to the client once.
pub struct NewAccessToken {
    pub access_token: PersonalAccessToken,
    pub plain_text_token: Zeroizing<String>,
}

#[allow(
    non_snake_case,
    reason = "Matches Sanctum's explicit serialization API"
)]
impl NewAccessToken {
    /// Includes the plaintext credential for the issuance response.
    pub fn toArray(&self) -> serde_json::Value {
        serde_json::json!({
            "accessToken": self.access_token,
            "plainTextToken": self.plain_text_token.as_str(),
        })
    }

    pub fn toJson(&self) -> serde_json::Result<Zeroizing<String>> {
        serde_json::to_string(&self.toArray()).map(Zeroizing::new)
    }
}

/// Persists and retrieves personal access tokens on the selected application connection.
pub struct PersonalAccessTokens {
    connection: Connection,
    prefix: String,
    model: Option<Box<dyn ModelStore>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Sanctum token API")]
impl PersonalAccessTokens {
    pub fn new(connection: Connection, prefix: impl Into<String>) -> Self {
        Self {
            connection,
            prefix: prefix.into(),
            model: None,
        }
    }

    pub(super) fn for_model<M: PersonalAccessTokenModel>(
        connection: Connection,
        prefix: String,
    ) -> Self {
        let model = Box::new(EloquentTokenStore::<M>::new(connection.clone()));
        Self {
            connection,
            prefix,
            model: Some(model),
        }
    }

    pub async fn createToken(
        &self,
        tokenable_type: &str,
        tokenable_id: impl Into<AuthIdentifier>,
        name: &str,
        abilities: &[&str],
        expires_at: Option<DateTime<Utc>>,
    ) -> AuthResult<NewAccessToken> {
        self.create_with_secret(
            tokenable_type,
            tokenable_id.into(),
            name,
            abilities,
            expires_at,
            generate_secret(&self.prefix)?,
        )
        .await
    }

    pub(super) async fn create_with_secret(
        &self,
        tokenable_type: &str,
        tokenable_id: AuthIdentifier,
        name: &str,
        abilities: &[&str],
        expires_at: Option<DateTime<Utc>>,
        secret: Zeroizing<String>,
    ) -> AuthResult<NewAccessToken> {
        if let Some(model) = &self.model {
            let access_token = model
                .create(Record::from([
                    ("tokenable_type".into(), tokenable_type.into()),
                    ("tokenable_id".into(), tokenable_id.clone().into()),
                    ("name".into(), name.into()),
                    ("token".into(), token_hash(&secret).into()),
                    (
                        "abilities".into(),
                        Value::Json(serde_json::json!(abilities)),
                    ),
                    (
                        "expires_at".into(),
                        expires_at.map_or(Value::Null, |date| date.to_rfc3339().into()),
                    ),
                ]))
                .await
                .map_err(AuthError::backend)?;
            return Ok(NewAccessToken {
                plain_text_token: Zeroizing::new(format!(
                    "{}|{}",
                    access_token.id,
                    secret.as_str()
                )),
                access_token,
            });
        }
        let AuthIdentifier::UnsignedInteger(owner_id) = tokenable_id else {
            return Err(AuthError::InvalidIdentity);
        };
        let created_at = seconds_precision(DateTime::<Utc>::from(SystemTime::now()));
        let expires_at = expires_at.map(seconds_precision);
        let created = created_at.format(TIMESTAMP).to_string();
        let row = Record::from([
            ("tokenable_type".into(), tokenable_type.into()),
            ("tokenable_id".into(), Value::Unsigned(owner_id)),
            ("name".into(), name.into()),
            ("token".into(), token_hash(&secret).into()),
            (
                "abilities".into(),
                serde_json::to_string(abilities)
                    .map_err(AuthError::backend)?
                    .into(),
            ),
            (
                "expires_at".into(),
                expires_at.map_or(Value::Null, |value| {
                    value.format(TIMESTAMP).to_string().into()
                }),
            ),
            ("created_at".into(), created.as_str().into()),
            ("updated_at".into(), created.as_str().into()),
        ]);
        let id = self
            .connection
            .table(TABLE)
            .insertGetId(row)
            .await
            .map_err(AuthError::backend)?;
        let id = integer(&id).map_err(AuthError::backend)?;
        Ok(NewAccessToken {
            access_token: PersonalAccessToken {
                id: id.into(),
                tokenable_type: tokenable_type.into(),
                tokenable_id,
                name: name.into(),
                abilities: abilities.iter().map(|ability| (*ability).into()).collect(),
                created_at: Some(created_at),
                last_used_at: None,
                expires_at,
            },
            plain_text_token: Zeroizing::new(format!("{id}|{}", secret.as_str())),
        })
    }

    pub async fn findToken(&self, credential: &str) -> AuthResult<Option<PersonalAccessToken>> {
        Ok(self
            .find_for_authentication(credential)
            .await?
            .map(|(token, _)| token))
    }

    pub(super) async fn find_for_authentication(
        &self,
        credential: &str,
    ) -> AuthResult<Option<(PersonalAccessToken, Option<Box<dyn ModelToken>>)>> {
        if let Some(model) = &self.model {
            return model
                .find(credential)
                .await
                .map(|found| found.map(|(token, model)| (token, Some(model))))
                .map_err(AuthError::backend);
        }
        let (id, secret) = if let Some((id, secret)) = credential.split_once('|') {
            if id.is_empty() || secret.is_empty() {
                return Ok(None);
            }
            (Some(id), secret)
        } else if credential.is_empty() {
            return Ok(None);
        } else {
            (None, credential)
        };
        let hash = token_hash(secret);
        let query = self.connection.table(TABLE).useWritePdo();
        let query = match id {
            Some(id) => {
                let Some(id) = integer_key(id, &self.connection) else {
                    return Ok(None);
                };
                query.r#where("id", "=", id)
            }
            None => query.r#where("token", "=", hash.as_str()),
        };
        let Some(row) = query.first().await.map_err(AuthError::backend)? else {
            return Ok(None);
        };
        if !bool::from(text(&row, "token")?.as_bytes().ct_eq(hash.as_bytes())) {
            return Ok(None);
        }
        Ok(Some((decode(&row)?, None)))
    }

    /// Revokes only a token belonging to the supplied owner.
    pub async fn revokeToken(
        &self,
        tokenable_type: &str,
        tokenable_id: impl Into<AuthIdentifier>,
        id: impl Into<AuthIdentifier>,
    ) -> AuthResult<bool> {
        let id = id.into();
        let tokenable_id = tokenable_id.into();
        if let Some(model) = &self.model {
            return model
                .revoke(tokenable_type, tokenable_id, id)
                .await
                .map_err(AuthError::backend);
        }
        let (AuthIdentifier::UnsignedInteger(owner_id), AuthIdentifier::UnsignedInteger(id)) =
            (tokenable_id, id)
        else {
            return Ok(false);
        };
        self.connection
            .table(TABLE)
            .r#where("id", "=", Value::Unsigned(id))
            .r#where("tokenable_type", "=", tokenable_type)
            .r#where("tokenable_id", "=", Value::Unsigned(owner_id))
            .delete()
            .await
            .map(|count| count != 0)
            .map_err(AuthError::backend)
    }

    pub(super) async fn record_use(
        &self,
        token: &mut PersonalAccessToken,
        credential: &str,
        model: Option<Box<dyn ModelToken>>,
    ) -> AuthResult<bool> {
        let now = seconds_precision(Utc::now());
        if let Some(mut model) = model {
            if !model.record_use(now).await.map_err(AuthError::backend)? {
                return Ok(false);
            }
            token.last_used_at = model.last_used_at().map_err(AuthError::backend)?;
            return Ok(true);
        }
        let timestamp = now.format(TIMESTAMP).to_string();
        let secret = credential
            .split_once('|')
            .map_or(credential, |(_, secret)| secret);
        let modified = self.connection.hasModifiedRecords();
        let result = self
            .connection
            .table(TABLE)
            .r#where("id", "=", Value::from(token.id.clone()))
            .r#where("token", "=", token_hash(secret))
            .update(Record::from([
                ("last_used_at".into(), timestamp.as_str().into()),
                ("updated_at".into(), timestamp.as_str().into()),
            ]))
            .await;
        self.connection.setRecordModificationState(modified);
        let count = result.map_err(AuthError::backend)?;
        if count == 0 && self.findToken(credential).await?.is_none() {
            return Ok(false);
        }
        token.last_used_at = Some(now);
        Ok(true)
    }

    pub(super) async fn related(
        &self,
        owner: &str,
        owner_id: AuthIdentifier,
    ) -> AuthResult<Vec<PersonalAccessToken>> {
        if let Some(model) = &self.model {
            return model
                .related(owner, owner_id)
                .await
                .map_err(AuthError::backend);
        }
        let AuthIdentifier::UnsignedInteger(owner_id) = owner_id else {
            return Err(AuthError::InvalidIdentity);
        };
        self.connection
            .table(TABLE)
            .useWritePdo()
            .r#where("tokenable_type", "=", owner)
            .r#where("tokenable_id", "=", Value::Unsigned(owner_id))
            .get()
            .await
            .map_err(AuthError::backend)?
            .iter()
            .map(decode)
            .collect()
    }

    pub(super) async fn delete_related(
        &self,
        owner: &str,
        owner_id: AuthIdentifier,
        id: Option<AuthIdentifier>,
    ) -> AuthResult<u64> {
        if let Some(model) = &self.model {
            return model
                .delete_related(owner, owner_id, id)
                .await
                .map_err(AuthError::backend);
        }
        let AuthIdentifier::UnsignedInteger(owner_id) = owner_id else {
            return Err(AuthError::InvalidIdentity);
        };
        let mut query = self
            .connection
            .table(TABLE)
            .r#where("tokenable_type", "=", owner)
            .r#where("tokenable_id", "=", Value::Unsigned(owner_id));
        if let Some(id) = id {
            let Some(key) = integer_key(&id.to_string(), &self.connection) else {
                return Ok(0);
            };
            query = query.r#where("id", "=", key);
        }
        query.delete().await.map_err(AuthError::backend)
    }

    pub(super) async fn prune(
        &self,
        column: &'static str,
        cutoff: DateTime<Utc>,
    ) -> AuthResult<()> {
        if let Some(model) = &self.model {
            return model
                .prune(column, cutoff)
                .await
                .map_err(AuthError::backend);
        }
        self.connection
            .table(TABLE)
            .r#where(column, "<", cutoff.format(TIMESTAMP).to_string())
            .delete()
            .await
            .map_err(AuthError::backend)?;
        Ok(())
    }
}

pub(super) fn generate_secret(prefix: &str) -> AuthResult<Zeroizing<String>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut entropy = Zeroizing::new(String::with_capacity(40));
    let mut random = Zeroizing::new([0_u8; 64]);
    while entropy.len() < 40 {
        rand::fill(random.as_mut()).map_err(AuthError::backend)?;
        for &byte in random.iter().filter(|&&byte| byte < 248) {
            entropy.push(char::from(ALPHABET[usize::from(byte % 62)]));
            if entropy.len() == 40 {
                break;
            }
        }
    }
    let checksum = crc32fast::hash(entropy.as_bytes());
    Ok(Zeroizing::new(format!(
        "{}{}{:08x}",
        prefix,
        entropy.as_str(),
        checksum
    )))
}
pub(super) fn token_hash(token: &str) -> String {
    let hash = digest::digest(&digest::SHA256, token.as_bytes());
    let mut output = String::with_capacity(64);
    const HEX: &[u8] = b"0123456789abcdef";
    for &byte in hash.as_ref() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

pub(super) fn integer_key(value: &str, connection: &Connection) -> Option<Value> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let id = value.parse::<u64>().ok()?;
    if id == 0 || (connection.is_sqlite() && i64::try_from(id).is_err()) {
        return None;
    }
    Some(Value::Unsigned(id))
}

fn decode(row: &Row) -> AuthResult<PersonalAccessToken> {
    let id =
        integer(row.get("id").ok_or(AuthError::InvalidIdentity)?).map_err(AuthError::backend)?;
    let tokenable_id = integer(row.get("tokenable_id").ok_or(AuthError::InvalidIdentity)?)
        .map_err(AuthError::backend)?;
    let abilities = match row.get("abilities") {
        Some(Value::Null) => Vec::new(),
        Some(Value::Text(abilities)) => {
            serde_json::from_str(abilities).map_err(AuthError::backend)?
        }
        _ => return Err(AuthError::backend(invalid_row())),
    };
    Ok(PersonalAccessToken {
        id: id.into(),
        tokenable_type: text(row, "tokenable_type")?.into(),
        tokenable_id: tokenable_id.into(),
        name: text(row, "name")?.into(),
        abilities,
        created_at: date(row, "created_at")?,
        last_used_at: date(row, "last_used_at")?,
        expires_at: date(row, "expires_at")?,
    })
}

pub(super) fn integer(value: &Value) -> DatabaseResult<u64> {
    match value {
        Value::Unsigned(value) => Ok(*value),
        Value::Integer(value) => u64::try_from(*value).map_err(|_| invalid_row()),
        _ => Err(invalid_row()),
    }
}

fn text<'a>(row: &'a Row, name: &str) -> AuthResult<&'a str> {
    match row.get(name) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(AuthError::backend(invalid_row())),
    }
}

fn date(row: &Row, name: &str) -> AuthResult<Option<DateTime<Utc>>> {
    match row.get(name) {
        Some(Value::Text(value)) => parse_timestamp(value).map(Some).map_err(AuthError::backend),
        Some(Value::Null) => Ok(None),
        _ => Err(AuthError::backend(invalid_row())),
    }
}

fn seconds_precision(value: DateTime<Utc>) -> DateTime<Utc> {
    use chrono::Timelike as _;
    value
        .with_nanosecond(0)
        .expect("zero nanoseconds are valid")
}

pub(super) fn parse_timestamp(value: &str) -> DatabaseResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.to_utc())
        .or_else(|_| NaiveDateTime::parse_from_str(value, TIMESTAMP).map(|time| time.and_utc()))
        .map_err(|_| invalid_row())
}

pub(super) fn invalid_row() -> DatabaseError {
    DatabaseError::InvalidQuery("personal access token row")
}
