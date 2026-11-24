use aws_lc_rs::{digest, rand};
use jiff::{SignedDuration, Timestamp, civil::DateTime as CivilDateTime};
use zeroize::Zeroizing;

use crate::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    database::{Connection, DatabaseError, DatabaseResult, Value, query::Record},
};

use super::token_model::PersonalAccessTokenModel;
use super::token_store::{EloquentTokenStore, ModelStore, ModelToken};

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
    pub(super) serialized_model: super::token_store::SerializedModel,
    pub(super) attribute_values: super::token_store::AttributeValues,
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

impl PersonalAccessToken {
    /// 显式读取令牌模型的属性转换结果；隐藏属性仍不会进入序列化响应。
    pub fn get_attribute_value(&self, name: &str) -> AuthResult<Option<Value>> {
        (self.attribute_values)(name).map_err(Into::into)
    }

    pub fn can(&self, ability: &str) -> bool {
        self.abilities
            .iter()
            .any(|value| value == "*" || value == ability)
    }

    pub fn cant(&self, ability: &str) -> bool {
        !self.can(ability)
    }

    pub fn is_expired(&self, now: Timestamp, expiration: Option<SignedDuration>) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at < now)
            || expiration.is_some_and(|lifetime| {
                self.created_at
                    .is_none_or(|created_at| now.duration_since(created_at) >= lifetime)
            })
    }
}

/// 事务内签发的选项；普通 Sanctum 默认能力和有效期保持不变。
pub struct TokenOptions<'a> {
    name: &'a str,
    abilities: &'a [&'a str],
    expires_at: Option<Timestamp>,
    attributes: Record,
    secret: Option<Zeroizing<String>>,
}

impl<'a> TokenOptions<'a> {
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            abilities: &["*"],
            expires_at: None,
            attributes: Record::new(),
            secret: None,
        }
    }
    pub fn abilities(mut self, abilities: &'a [&'a str]) -> Self {
        self.abilities = abilities;
        self
    }
    pub fn expires_at(mut self, expires_at: Option<Timestamp>) -> Self {
        self.expires_at = expires_at;
        self
    }
    /// 复用模型覆写的 generate_token_string；明文仅保留到签发返回。
    pub fn secret(mut self, secret: Zeroizing<String>) -> Self {
        self.secret = Some(secret);
        self
    }
    pub fn attributes(mut self, attributes: Record) -> Self {
        self.attributes = attributes;
        self
    }
}

/// A token and the credential to return to the client once.
pub struct NewAccessToken {
    pub access_token: PersonalAccessToken,
    pub plain_text_token: Zeroizing<String>,
}

impl NewAccessToken {
    /// Includes the plaintext credential for the issuance response.
    pub fn to_array(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self.issuance_payload())
    }

    pub fn to_json(&self) -> serde_json::Result<Zeroizing<String>> {
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

impl PersonalAccessTokens {
    pub fn new(connection: Connection, prefix: impl Into<String>) -> Self {
        Self::for_model::<super::DefaultPersonalAccessToken>(connection, prefix.into())
    }

    pub fn get_table(&self) -> String {
        self.model.table_name()
    }

    pub fn get_connection_name(&self) -> &str {
        self.connection.get_name()
    }

    /// 核对能否与账户连接共同提交；不将跨连接写入冒充原子事务。
    pub fn uses_connection(&self, connection: &Connection) -> bool {
        self.connection.same_query_source(connection)
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

    pub async fn create_token(
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
        self.create_with_attributes(
            None,
            tokenable_type,
            tokenable_id,
            secret,
            TokenOptions::new(name)
                .abilities(abilities)
                .expires_at(expires_at),
        )
        .await
    }

    /// 在调用者事务内签发，供需要与账户写锁共同提交的安全操作使用。
    /// 连接不一致或扩展属性试图覆盖标准凭据字段时，在写入前报错。
    pub async fn create_token_in_transaction(
        &self,
        transaction: &mut crate::database::Transaction,
        tokenable_type: &str,
        tokenable_id: impl Into<AuthIdentifier>,
        mut options: TokenOptions<'_>,
    ) -> AuthResult<NewAccessToken> {
        if !transaction.belongs_to(&self.connection) {
            return Err(AuthError::Configuration("令牌与账户必须使用同一事务连接"));
        }
        let secret = match options.secret.take() {
            Some(secret) => secret,
            None => generate_secret(&self.prefix)?,
        };
        self.create_with_attributes(
            Some(transaction),
            tokenable_type,
            tokenable_id.into(),
            secret,
            options,
        )
        .await
    }

    async fn create_with_attributes(
        &self,
        transaction: Option<&mut crate::database::Transaction>,
        tokenable_type: &str,
        tokenable_id: AuthIdentifier,
        secret: Zeroizing<String>,
        options: TokenOptions<'_>,
    ) -> AuthResult<NewAccessToken> {
        let TokenOptions {
            name,
            abilities,
            expires_at,
            attributes,
            secret: _,
        } = options;
        let mut values = Record::from([
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
        ]);
        for (key, value) in attributes {
            let base = key.split("->").next().unwrap_or(&key);
            if base == self.model.key_name()
                || values.contains_key(base)
                || matches!(base, "id" | "created_at" | "updated_at" | "last_used_at")
            {
                return Err(AuthError::Configuration("扩展令牌属性不能覆盖标准凭据字段"));
            }
            values.insert(key, value);
        }
        let access_token = self.model.create(values, transaction).await?;
        Ok(NewAccessToken {
            plain_text_token: Zeroizing::new(format!("{}|{}", access_token.id, secret.as_str())),
            access_token,
        })
    }

    pub async fn find_token(&self, credential: &str) -> AuthResult<Option<PersonalAccessToken>> {
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
    pub async fn revoke_token(
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
        let container = Connection::model_container(Some(&self.connection));
        if !model
            .record_use(seconds_precision(crate::support::utc_now_for(
                container.as_deref(),
            )))
            .await?
        {
            return Ok(false);
        }
        token.last_used_at = model.last_used_at()?;
        token.updated_at = model.updated_at()?;
        token.serialized_model = model.serialized_model();
        token.attribute_values = model.attribute_values();
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
