use aws_lc_rs::{digest, rand};
use jiff::{SignedDuration, Timestamp, civil::DateTime as CivilDateTime};
use zeroize::Zeroizing;

use crate::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    database::{Connection, DatabaseError, DatabaseResult, Value, query::Record},
};

use super::token_model::{EloquentTokenStore, ModelStore, ModelToken, PersonalAccessTokenModel};

pub(super) const TIMESTAMP: &str = "%Y-%m-%d %H:%M:%S";

/// A persisted token. Its digest is kept in storage and its secret is only returned at issuance.
#[derive(Clone)]
pub struct PersonalAccessToken {
    pub id: AuthIdentifier,
    pub tokenable_type: String,
    pub tokenable_id: AuthIdentifier,
    pub name: String,
    pub abilities: Vec<String>,
    pub created_at: Option<Timestamp>,
    pub updated_at: Option<Timestamp>,
    pub last_used_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub(super) serialized_model: super::token_model::SerializedModel,
}

impl serde::Serialize for PersonalAccessToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let model = self
            .serialized_model
            .as_ref()
            .map_err(serde::ser::Error::custom)?;
        serde::Serialize::serialize(model, serializer)
    }
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

    pub fn isExpired(&self, now: Timestamp, expiration: Option<SignedDuration>) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at < now)
            || expiration.is_some_and(|lifetime| {
                self.created_at
                    .is_none_or(|created_at| now.duration_since(created_at) >= lifetime)
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
    pub fn toArray(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self.issuance_payload())
    }

    pub fn toJson(&self) -> serde_json::Result<Zeroizing<String>> {
        serde_json::to_string(&self.issuance_payload()).map(Zeroizing::new)
    }

    fn issuance_payload(&self) -> IssuancePayload<'_> {
        IssuancePayload {
            access_token: &self.access_token,
            plain_text_token: self.plain_text_token.as_str(),
        }
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct IssuancePayload<'a> {
    access_token: &'a PersonalAccessToken,
    plain_text_token: &'a str,
}

/// Persists and retrieves personal access tokens on the selected application connection.
pub struct PersonalAccessTokens {
    connection: Connection,
    prefix: String,
    model: Box<dyn ModelStore>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Sanctum token API")]
impl PersonalAccessTokens {
    pub fn new(connection: Connection, prefix: impl Into<String>) -> Self {
        Self::for_model::<super::DefaultPersonalAccessToken>(connection, prefix.into())
    }

    pub(super) fn connection(&self) -> &Connection {
        &self.connection
    }

    pub(super) fn for_model<M: PersonalAccessTokenModel>(
        connection: Connection,
        prefix: String,
    ) -> Self {
        let model = Box::new(EloquentTokenStore::<M>::new(connection.clone()));
        Self {
            connection,
            prefix,
            model,
        }
    }

    pub async fn createToken(
        &self,
        tokenable_type: &str,
        tokenable_id: impl Into<AuthIdentifier>,
        name: &str,
        abilities: &[&str],
        expires_at: Option<Timestamp>,
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
        expires_at: Option<Timestamp>,
        secret: Zeroizing<String>,
    ) -> AuthResult<NewAccessToken> {
        let access_token = self
            .model
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
                    expires_at.map_or(Value::Null, |date| timestamp_text(date).into()),
                ),
            ]))
            .await?;
        Ok(NewAccessToken {
            plain_text_token: Zeroizing::new(format!("{}|{}", access_token.id, secret.as_str())),
            access_token,
        })
    }

    pub async fn findToken(&self, credential: &str) -> AuthResult<Option<PersonalAccessToken>> {
        Ok(self
            .find_for_authentication(credential)
            .await?
            .map(|(token, _)| token))
    }

    pub(super) fn accepts_bearer(&self, credential: &str) -> bool {
        if credential == "0" {
            return false;
        }
        if !self.model.integer_key() {
            return true;
        }
        credential.split_once('|').is_none_or(|(id, secret)| {
            !id.is_empty()
                && id.bytes().all(|byte| byte.is_ascii_digit())
                && !secret.is_empty()
                && secret != "0"
        })
    }

    pub(super) async fn find_for_authentication(
        &self,
        credential: &str,
    ) -> AuthResult<Option<(PersonalAccessToken, Box<dyn ModelToken>)>> {
        self.model
            .find(credential)
            .await
            .map_err(AuthError::backend)
    }

    /// Revokes only a token belonging to the supplied owner.
    pub async fn revokeToken(
        &self,
        tokenable_type: &str,
        tokenable_id: impl Into<AuthIdentifier>,
        id: impl Into<AuthIdentifier>,
    ) -> AuthResult<bool> {
        self.model
            .revoke(tokenable_type, tokenable_id.into(), id.into())
            .await
            .map_err(AuthError::backend)
    }

    pub(super) async fn record_use(
        &self,
        token: &mut PersonalAccessToken,
        mut model: Box<dyn ModelToken>,
    ) -> AuthResult<bool> {
        if !model
            .record_use(seconds_precision(Timestamp::now()))
            .await?
        {
            return Ok(false);
        }
        token.last_used_at = model.last_used_at()?;
        token.updated_at = model.updated_at()?;
        token.serialized_model = model.serialized_model();
        Ok(true)
    }

    pub(super) async fn related(
        &self,
        owner: &str,
        owner_id: AuthIdentifier,
    ) -> AuthResult<Vec<PersonalAccessToken>> {
        self.model
            .related(owner, owner_id)
            .await
            .map_err(AuthError::backend)
    }

    pub(super) async fn delete_related(
        &self,
        owner: &str,
        owner_id: AuthIdentifier,
        id: Option<AuthIdentifier>,
    ) -> AuthResult<u64> {
        self.model
            .delete_related(owner, owner_id, id)
            .await
            .map_err(AuthError::backend)
    }

    pub(super) async fn prune(&self, column: &'static str, cutoff: Timestamp) -> AuthResult<()> {
        self.model
            .prune(column, cutoff)
            .await
            .map_err(AuthError::backend)
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

fn seconds_precision(value: Timestamp) -> Timestamp {
    Timestamp::from_second(crate::support::timestamp_seconds(value)).expect("valid whole second")
}

pub(super) fn parse_timestamp(value: &str) -> DatabaseResult<Timestamp> {
    value
        .parse::<Timestamp>()
        .or_else(|_| {
            CivilDateTime::strptime(TIMESTAMP, value)
                .and_then(|time| time.to_zoned(jiff::tz::TimeZone::UTC))
                .map(|time| time.timestamp())
        })
        .map_err(|_| invalid_row())
}

pub(super) fn timestamp_text(value: Timestamp) -> String {
    let nanoseconds = value.subsec_nanosecond().unsigned_abs();
    // Token attribute text retains whole milliseconds, microseconds or nanoseconds.
    let format = if nanoseconds == 0 {
        "%Y-%m-%dT%H:%M:%S%:z"
    } else if nanoseconds.is_multiple_of(1_000_000) {
        "%Y-%m-%dT%H:%M:%S%.3f%:z"
    } else if nanoseconds.is_multiple_of(1_000) {
        "%Y-%m-%dT%H:%M:%S%.6f%:z"
    } else {
        "%Y-%m-%dT%H:%M:%S%.9f%:z"
    };
    value.strftime(format).to_string()
}

pub(super) fn invalid_row() -> DatabaseError {
    DatabaseError::InvalidQuery("personal access token row")
}

#[cfg(test)]
#[path = "../../tests/unit/sanctum_timestamps.rs"]
mod tests;
