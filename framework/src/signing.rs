//! 提供用途绑定且有效期明确的签名链接，并可选使用共享撤销存储。
#![doc = include_str!("signing/README.md")]

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config::ConfigError,
    core::{App, AppBuilder, BoxFuture, Error, Plugin, Result},
};

pub use crate::config::SignedLinksConfig;

mod migration;
pub use migration::SignedLinkRevocationMigration;

const LINK_TYPE: &str = "openrust-link+jwt";

/// 不透明的签名链接 token。`Debug` 输出不会暴露 bearer 值。
#[derive(Clone)]
pub struct SignedLink(String);
impl SignedLink {
    /// 返回可放入 URL 或消息中的 bearer token。
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for SignedLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SignedLink([REDACTED])")
    }
}

/// 从签名链接中恢复并校验的数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedLinkClaims {
    purpose: String,
    subject: String,
    expires_at: u64,
    jti: String,
}
impl SignedLinkClaims {
    /// 返回签名中绑定的确切用途。
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
    /// 返回签名中绑定的应用定义主体。
    pub fn subject(&self) -> &str {
        &self.subject
    }
    /// 返回以秒为单位的 Unix 过期时间戳。
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// 用于可选单 token 撤销的共享存储。
pub trait SignedLinkRevocationStore: Send + Sync + 'static {
    /// 保存 token 摘要，直至其签名声明的过期时间。
    fn revoke<'a>(&'a self, token_hash: &'a str, expires_at: u64) -> BoxFuture<'a, Result<()>>;
    /// 返回 token 摘要是否已撤销。
    fn is_revoked<'a>(&'a self, token_hash: &'a str) -> BoxFuture<'a, Result<bool>>;
}

/// 对短期应用链接进行签名、验证，并可选地撤销链接。
#[derive(Clone)]
pub struct SignedLinks {
    inner: Arc<SigningConfig>,
    revocations: Option<Arc<dyn SignedLinkRevocationStore>>,
}
impl fmt::Debug for SignedLinks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedLinks")
            .field("issuer", &self.inner.issuer)
            .field("active_key_id", &self.inner.active_key_id)
            .field("key_ids", &self.inner.keys.keys().collect::<Vec<_>>())
            .field("revocation_store", &self.revocations.is_some())
            .finish()
    }
}
impl SignedLinks {
    /// 校验独立的链接签名密钥环。
    pub fn new(config: SignedLinksConfig) -> Result<Self> {
        if config.issuer.trim().is_empty()
            || config.issuer.len() > 256
            || config.max_ttl_secs == 0
            || config.max_ttl_secs > 2_592_000
            || config.keys.is_empty()
            || config.keys.len() > 16
            || !config.keys.contains_key(&config.active_key_id)
            || !valid_key_id(&config.active_key_id)
            || config.keys.keys().any(|id| !valid_key_id(id))
        {
            return Err(invalid_config(
                "signed-link issuer, key ring, key IDs, or maximum TTL is invalid",
            ));
        }
        let mut keys = BTreeMap::new();
        for (id, encoded) in config.keys {
            let bytes = STANDARD
                .decode(encoded.trim())
                .map_err(|_| invalid_config("signed-link keys must be standard Base64"))?;
            if !(32..=4096).contains(&bytes.len()) {
                return Err(invalid_config(
                    "each signed-link key must decode to 32-4096 bytes",
                ));
            }
            keys.insert(
                id,
                LinkKey {
                    encoding: EncodingKey::from_secret(&bytes),
                    decoding: DecodingKey::from_secret(&bytes),
                },
            );
        }
        Ok(Self {
            inner: Arc::new(SigningConfig {
                issuer: config.issuer,
                active_key_id: config.active_key_id,
                keys,
                max_ttl_secs: config.max_ttl_secs,
            }),
            revocations: None,
        })
    }
    /// 添加共享撤销存储；未配置时，链接会一直有效至过期或密钥退役。
    #[must_use]
    pub fn with_revocation_store(mut self, store: impl SignedLinkRevocationStore) -> Self {
        self.revocations = Some(Arc::new(store));
        self
    }

    /// 添加 PostgreSQL/MySQL 共享数据库撤销存储。
    #[must_use]
    pub fn with_database_revocations(self, db: crate::db::Db) -> Self {
        self.with_revocation_store(DatabaseSignedLinkRevocationStore::new(db))
    }
    /// 为用途和主体均绑定的链接签名，并使用调用方指定的有效期。
    pub fn issue(&self, purpose: &str, subject: &str, ttl_secs: u64) -> Result<SignedLink> {
        if !valid_label(purpose, 128)
            || !valid_label(subject, 1024)
            || ttl_secs == 0
            || ttl_secs > self.inner.max_ttl_secs
        {
            return Err(Error::BadRequest(
                "signed-link purpose, subject, or lifetime is invalid".into(),
            ));
        }
        let issued_at = unix_seconds()?;
        let expires_at = issued_at
            .checked_add(ttl_secs)
            .ok_or_else(|| invalid_config("signed-link expiry is out of range"))?;
        let mut id = [0_u8; 32];
        getrandom::fill(&mut id).map_err(Error::internal)?;
        let claims = LinkTokenClaims {
            iss: self.inner.issuer.clone(),
            purpose: purpose.to_owned(),
            sub: subject.to_owned(),
            iat: issued_at,
            exp: expires_at,
            jti: hex(&id),
        };
        let key = self
            .inner
            .keys
            .get(&self.inner.active_key_id)
            .ok_or_else(|| Error::internal(MissingActiveKey))?;
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(self.inner.active_key_id.clone());
        header.typ = Some(LINK_TYPE.to_owned());
        encode(&header, &claims, &key.encoding)
            .map(SignedLink)
            .map_err(|_| Error::internal(SigningFailure))
    }
    /// 验证签名、用途、签发者、有效期以及可选的共享撤销状态。
    pub async fn verify(&self, token: &str, purpose: &str) -> Result<SignedLinkClaims> {
        let claims = self.decode(token)?;
        if claims.purpose != purpose {
            return Err(Error::Unauthorized);
        }
        if let Some(store) = &self.revocations
            && store.is_revoked(&sha256(claims.jti.as_bytes())).await?
        {
            return Err(Error::Unauthorized);
        }
        Ok(SignedLinkClaims {
            purpose: claims.purpose,
            subject: claims.sub,
            expires_at: claims.exp,
            jti: claims.jti,
        })
    }
    /// 撤销有效链接直至其过期；必须已配置共享撤销存储。
    pub async fn revoke(&self, token: &str) -> Result<()> {
        let Some(store) = &self.revocations else {
            return Err(Error::custom(
                "signed_link_revocation_unavailable",
                "signed-link revocation requires a shared revocation store",
            ));
        };
        let claims = self.decode(token)?;
        store
            .revoke(&sha256(claims.jti.as_bytes()), claims.exp)
            .await
    }
    fn decode(&self, token: &str) -> Result<LinkTokenClaims> {
        if token.len() > 8192 {
            return Err(Error::Unauthorized);
        }
        let header = decode_header(token).map_err(|_| Error::Unauthorized)?;
        if header.alg != Algorithm::HS256 || header.typ.as_deref() != Some(LINK_TYPE) {
            return Err(Error::Unauthorized);
        }
        let key_id = header.kid.as_deref().ok_or(Error::Unauthorized)?;
        let key = self.inner.keys.get(key_id).ok_or(Error::Unauthorized)?;
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        validation.validate_nbf = false;
        validation.set_issuer(&[self.inner.issuer.as_str()]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);
        let claims = decode::<LinkTokenClaims>(token, &key.decoding, &validation)
            .map_err(|_| Error::Unauthorized)?
            .claims;
        let now = unix_seconds().map_err(|_| Error::Unauthorized)?;
        if claims.iss != self.inner.issuer
            || !valid_label(&claims.purpose, 128)
            || !valid_label(&claims.sub, 1024)
            || !valid_jti(&claims.jti)
            || claims.exp <= claims.iat
            || claims.exp - claims.iat > self.inner.max_ttl_secs
            || claims.iat > now
            || claims.exp <= now
        {
            return Err(Error::Unauthorized);
        }
        Ok(claims)
    }
}

/// 根据 `[security.signed_links]` 配置注册 [`SignedLinks`]。
pub struct SignedLinksPlugin {
    config: Option<SignedLinksConfig>,
    database: Option<crate::db::Db>,
    database_from_app: bool,
    service: OnceLock<SignedLinks>,
}
impl SignedLinksPlugin {
    /// 在构建应用时加载签名配置。
    pub fn new() -> Self {
        Self {
            config: None,
            database: None,
            database_from_app: false,
            service: OnceLock::new(),
        }
    }
    /// 注册显式指定的签名配置。
    pub fn with_config(config: SignedLinksConfig) -> Self {
        Self {
            config: Some(config),
            database: None,
            database_from_app: false,
            service: OnceLock::new(),
        }
    }

    /// 使用应用的共享数据库启用单链接撤销。
    #[must_use]
    pub fn with_database_revocations(mut self, db: crate::db::Db) -> Self {
        self.database = Some(db);
        self
    }

    /// 使用之前插件注册的 `Db` 启用数据库撤销。
    #[must_use]
    pub const fn with_database_revocations_from_app(mut self) -> Self {
        self.database_from_app = true;
        self
    }
}
impl Default for SignedLinksPlugin {
    fn default() -> Self {
        Self::new()
    }
}
impl Plugin for SignedLinksPlugin {
    fn name(&self) -> &'static str {
        "signed-links"
    }
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        if app.get::<SignedLinks>().is_some() {
            return Err(Error::custom(
                "signed_links_already_registered",
                "register only one signed-link service per application",
            ));
        }
        let config = match &self.config {
            Some(config) => config.clone(),
            None => app
                .sensitive_section::<SignedLinksConfig>("security.signed_links")
                .map_err(Error::Config)?,
        };
        let mut service = SignedLinks::new(config)?;
        if let Some(db) = &self.database {
            service = service.with_database_revocations(db.clone());
        } else if self.database_from_app {
            let db = app.get::<crate::db::Db>().ok_or_else(|| {
                Error::custom(
                    "signed_link_database_missing",
                    "database revocation requires a previously registered database plugin",
                )
            })?;
            service = service.with_database_revocations(db.as_ref().clone());
        }
        self.service.set(service.clone()).map_err(|_| {
            Error::custom(
                "signed_links_plugin_reused",
                "signed-link plugin can only build once",
            )
        })?;
        app.insert(service);
        Ok(())
    }
    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let expected = self
                .service
                .get()
                .ok_or_else(|| Error::internal(MissingPluginBuild))?;
            let actual = app.state().try_get::<SignedLinks>()?;
            if !Arc::ptr_eq(&expected.inner, &actual.inner) {
                return Err(Error::custom(
                    "signed_links_service_replaced",
                    "a later plugin replaced the signed-link service",
                ));
            }
            Ok(())
        })
    }
}

#[derive(Clone)]
struct LinkKey {
    encoding: EncodingKey,
    decoding: DecodingKey,
}
struct SigningConfig {
    issuer: String,
    active_key_id: String,
    keys: BTreeMap<String, LinkKey>,
    max_ttl_secs: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LinkTokenClaims {
    iss: String,
    purpose: String,
    sub: String,
    iat: u64,
    exp: u64,
    jti: String,
}
fn valid_label(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.bytes().any(|b| b.is_ascii_control())
}
fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
fn valid_jti(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn unix_seconds() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(Error::internal)
}
fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(H[usize::from(b >> 4)]),
                char::from(H[usize::from(b & 15)]),
            ]
        })
        .collect()
}
fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn invalid_config(message: &str) -> Error {
    Error::Config(ConfigError::Invalid(message.to_owned()))
}
#[derive(Debug)]
struct MissingActiveKey;
impl fmt::Display for MissingActiveKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("configured active signed-link key is unavailable")
    }
}
impl std::error::Error for MissingActiveKey {}
#[derive(Debug)]
struct SigningFailure;
impl fmt::Display for SigningFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("signed-link token creation failed")
    }
}
impl std::error::Error for SigningFailure {}
#[derive(Debug)]
struct MissingPluginBuild;
impl fmt::Display for MissingPluginBuild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("signed-link plugin build state is unavailable")
    }
}
impl std::error::Error for MissingPluginBuild {}

/// 基于 PostgreSQL/MySQL 的签名链接共享撤销存储。
#[derive(Clone)]
pub struct DatabaseSignedLinkRevocationStore {
    db: crate::db::Db,
}

impl DatabaseSignedLinkRevocationStore {
    /// 使用应用的共享数据库和表名解析器创建存储。
    pub fn new(db: crate::db::Db) -> Self {
        Self { db }
    }

    /// 删除最多 `limit` 条已过期的撤销记录，并返回实际删除数量。
    pub async fn purge_expired(&self, limit: u64) -> Result<u64> {
        use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
        if !(1..=1000).contains(&limit) {
            return Err(Error::BadRequest(
                "signed-link cleanup limit must be between 1 and 1000".into(),
            ));
        }
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let table = self.db.quoted_table_name("signed_link_revocations")?;
        let now = i64::try_from(unix_seconds()?).map_err(Error::internal)?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "DELETE FROM {table} WHERE token_hash IN (SELECT token_hash FROM {table} WHERE expires_at <= $1 ORDER BY expires_at LIMIT $2)"
            ),
            DbBackend::MySql => {
                format!("DELETE FROM {table} WHERE expires_at <= ? ORDER BY expires_at LIMIT ?")
            }
            _ => {
                return Err(Error::custom(
                    "unsupported_signed_link_database",
                    "signed-link revocation requires PostgreSQL or MySQL",
                ));
            }
        };
        let values = vec![
            now.into(),
            i64::try_from(limit).map_err(Error::internal)?.into(),
        ];
        connection
            .execute_raw(Statement::from_sql_and_values(backend, &sql, values))
            .await
            .map(|result| result.rows_affected())
            .map_err(Error::internal)
    }
}

impl SignedLinkRevocationStore for DatabaseSignedLinkRevocationStore {
    fn revoke<'a>(&'a self, token_hash: &'a str, expires_at: u64) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
            let connection = self.db.connection()?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("signed_link_revocations")?;
            let sql = match backend {
                DbBackend::Postgres => format!(
                    "INSERT INTO {table} AS existing (token_hash, expires_at) VALUES ($1, $2) ON CONFLICT (token_hash) DO UPDATE SET expires_at = GREATEST(existing.expires_at, EXCLUDED.expires_at)"
                ),
                DbBackend::MySql => format!(
                    "INSERT INTO {table} (token_hash, expires_at) VALUES (?, ?) ON DUPLICATE KEY UPDATE expires_at = GREATEST(expires_at, VALUES(expires_at))"
                ),
                _ => {
                    return Err(Error::custom(
                        "unsupported_signed_link_database",
                        "signed-link revocation requires PostgreSQL or MySQL",
                    ));
                }
            };
            connection
                .execute_raw(Statement::from_sql_and_values(
                    backend,
                    &sql,
                    [
                        token_hash.into(),
                        i64::try_from(expires_at).map_err(Error::internal)?.into(),
                    ],
                ))
                .await
                .map_err(Error::internal)?;
            Ok(())
        })
    }
    fn is_revoked<'a>(&'a self, token_hash: &'a str) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
            let connection = self.db.connection()?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("signed_link_revocations")?;
            let sql = match backend {
                DbBackend::Postgres => format!(
                    "SELECT 1 AS found FROM {table} WHERE token_hash = $1 AND expires_at > $2"
                ),
                DbBackend::MySql => format!(
                    "SELECT 1 AS found FROM {table} WHERE token_hash = ? AND expires_at > ?"
                ),
                _ => {
                    return Err(Error::custom(
                        "unsupported_signed_link_database",
                        "signed-link revocation requires PostgreSQL or MySQL",
                    ));
                }
            };
            let now = i64::try_from(unix_seconds()?).map_err(Error::internal)?;
            Ok(connection
                .query_one_raw(Statement::from_sql_and_values(
                    backend,
                    &sql,
                    [token_hash.into(), now.into()],
                ))
                .await
                .map_err(Error::internal)?
                .is_some())
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};

    use super::{SignedLinkRevocationStore, SignedLinks, SignedLinksConfig};
    use crate::core::{BoxFuture, Result};

    #[derive(Clone, Default)]
    struct Revocations(Arc<Mutex<Vec<String>>>);
    impl SignedLinkRevocationStore for Revocations {
        fn revoke<'a>(
            &'a self,
            token_hash: &'a str,
            _expires_at: u64,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .map_err(|_| crate::core::Error::internal(std::io::Error::other("poisoned")))?
                    .push(token_hash.to_owned());
                Ok(())
            })
        }
        fn is_revoked<'a>(&'a self, token_hash: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                Ok(self
                    .0
                    .lock()
                    .map_err(|_| crate::core::Error::internal(std::io::Error::other("poisoned")))?
                    .iter()
                    .any(|item| item == token_hash))
            })
        }
    }

    fn config() -> SignedLinksConfig {
        SignedLinksConfig::new(
            "https://example.test",
            "link-1",
            BTreeMap::from([("link-1".to_owned(), STANDARD.encode([7_u8; 32]))]),
        )
    }

    #[tokio::test]
    async fn link_binds_purpose_subject_and_expiry() {
        let links = SignedLinks::new(config()).expect("key ring is valid");
        let token = links
            .issue("email.verify", "user:42", 60)
            .expect("valid link is signed");
        let claims = links
            .verify(token.expose(), "email.verify")
            .await
            .expect("link verifies");
        assert_eq!(claims.subject(), "user:42");
        assert!(
            links
                .verify(token.expose(), "password.reset")
                .await
                .is_err()
        );
        assert!(!format!("{token:?}").contains(token.expose()));
        assert!(links.issue("", "user:42", 60).is_err());
        assert!(links.issue("x", "user:42", 86_401).is_err());
    }

    #[tokio::test]
    async fn shared_store_revokes_valid_link() {
        let store = Revocations::default();
        let links = SignedLinks::new(config())
            .expect("key ring is valid")
            .with_revocation_store(store);
        let token = links
            .issue("file.download", "object:9", 60)
            .expect("valid link is signed");
        links
            .verify(token.expose(), "file.download")
            .await
            .expect("link initially verifies");
        links
            .revoke(token.expose())
            .await
            .expect("revocation is stored");
        assert!(links.verify(token.expose(), "file.download").await.is_err());
    }

    #[tokio::test]
    async fn revocation_requires_shared_store() {
        let links = SignedLinks::new(config()).expect("key ring is valid");
        let token = links
            .issue("email.verify", "user:42", 60)
            .expect("valid link is signed");
        assert!(links.revoke(token.expose()).await.is_err());
    }

    #[tokio::test]
    async fn retained_key_verifies_old_links_and_new_key_signs_new_ones() {
        let old = SignedLinks::new(config()).expect("old key ring is valid");
        let old_token = old
            .issue("file.download", "asset:1", 60)
            .expect("old key signs");
        let rotated = SignedLinks::new(SignedLinksConfig::new(
            "https://example.test",
            "link-2",
            BTreeMap::from([
                ("link-1".to_owned(), STANDARD.encode([7_u8; 32])),
                ("link-2".to_owned(), STANDARD.encode([8_u8; 32])),
            ]),
        ))
        .expect("rotated key ring is valid");
        assert_eq!(
            rotated
                .verify(old_token.expose(), "file.download")
                .await
                .expect("old key remains accepted")
                .subject(),
            "asset:1"
        );
        let new_token = rotated
            .issue("file.download", "asset:2", 60)
            .expect("active key signs");
        assert!(
            old.verify(new_token.expose(), "file.download")
                .await
                .is_err()
        );
        assert!(!format!("{:?}", config()).contains(&STANDARD.encode([7_u8; 32])));
    }
}
