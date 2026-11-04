//! 可选 TOTP 多因素认证与一次性登录挑战。

use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::db::sea_orm::{ConnectionTrait, DbBackend, Statement, sea_query::Alias};
use crate::db::sea_orm_migration::prelude::*;
use crate::{
    Error, Result,
    audit::{Audit, AuditEvent, AuditOutcome, AuditPolicy, AuditSubject, DatabaseAuditSink},
    core::{BoxFuture, RateLimiter},
    db::{DatabaseTableNames, DatabaseTransaction, Db, dialect},
    security::Encryptor,
};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::Passwords;
use super::refresh::revoke_all_for_user_in_transaction;

type HmacSha1 = Hmac<Sha1>;

fn totp_code(secret: &[u8], time_step: u64, digits: u32) -> Result<String> {
    if secret.len() < 16 || !(6..=8).contains(&digits) {
        return Err(Error::BadRequest("invalid TOTP parameters".into()));
    }
    let mut mac = HmacSha1::new_from_slice(secret).map_err(Error::internal)?;
    mac.update(&time_step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    let modulus = 10_u32.pow(digits);
    Ok(format!(
        "{:0width$}",
        binary % modulus,
        width = digits as usize
    ))
}

fn constant_time_code_eq(actual: &str, expected: &str) -> bool {
    actual.len() == expected.len() && bool::from(actual.as_bytes().ct_eq(expected.as_bytes()))
}

const TOTP_PERIOD_SECONDS: u64 = 30;
const TOTP_DIGITS: u32 = 6;
const CHALLENGE_TTL_SECONDS: u64 = 300;
const ENROLLMENT_TTL_SECONDS: u64 = 600;
const MAX_CHALLENGE_ATTEMPTS: u32 = 5;
const RECOVERY_CODE_COUNT: usize = 10;
const RATE_LIMIT_WINDOW: Duration = Duration::from_mins(15);

/// 短时且绑定可信请求来源的一次性登录 challenge。
pub struct MfaLoginChallenge {
    token: String,
    user_id: i64,
    expires_at: DateTime<Utc>,
}

impl MfaLoginChallenge {
    fn new(token: String, user_id: i64, expires_at: DateTime<Utc>) -> Self {
        Self {
            token,
            user_id,
            expires_at,
        }
    }

    /// 返回只应一次发送给客户端的不透明 challenge token。
    pub fn expose(&self) -> &str {
        &self.token
    }

    /// 返回已通过密码验证的账户 ID。
    pub const fn user_id(&self) -> i64 {
        self.user_id
    }

    /// 返回 challenge 的数据库有效期。
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

impl fmt::Debug for MfaLoginChallenge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MfaLoginChallenge")
            .field("token", &"[REDACTED]")
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// TOTP 首次验证前的 provisioning URI；应通过受保护页面一次性展示。
pub struct TotpEnrollment {
    uri: String,
    expires_at: DateTime<Utc>,
}

impl TotpEnrollment {
    fn new(uri: String, expires_at: DateTime<Utc>) -> Self {
        Self { uri, expires_at }
    }

    /// 返回供兼容验证器扫描的 `otpauth://` URI。
    pub fn provisioning_uri(&self) -> &str {
        &self.uri
    }

    /// 返回待确认 seed 的过期时间。
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

impl fmt::Debug for TotpEnrollment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TotpEnrollment")
            .field("provisioning_uri", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// 新生成且只返回一次的账户恢复码集合。
pub struct RecoveryCodes(Vec<String>);

impl RecoveryCodes {
    fn new(codes: Vec<String>) -> Self {
        Self(codes)
    }

    /// 返回需由应用仅向已重新认证用户展示一次的恢复码。
    pub fn expose(&self) -> &[String] {
        &self.0
    }
}

impl fmt::Debug for RecoveryCodes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecoveryCodes")
            .field("count", &self.0.len())
            .field("codes", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct MfaService {
    db: Db,
    encryptor: Encryptor,
    limiter: RateLimiter,
    audit: DatabaseAuditSink,
}

impl fmt::Debug for MfaService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MfaService")
    }
}

impl MfaService {
    fn new(db: Db, encryptor: Encryptor, limiter: RateLimiter) -> Self {
        Self {
            audit: DatabaseAuditSink::new(db.clone()),
            db,
            encryptor,
            limiter,
        }
    }

    pub(crate) async fn enabled(&self, user_id: i64) -> Result<bool> {
        let connection = self.db.connection()?;
        let table = self.db.quoted_table_name("auth_mfa_factors")?;
        let backend = connection.get_database_backend();
        let sql = match backend {
            DbBackend::Postgres => format!("SELECT user_id FROM {table} WHERE user_id = $1"),
            DbBackend::MySql | DbBackend::Sqlite => {
                format!("SELECT user_id FROM {table} WHERE user_id = ?")
            }
            _ => return Err(unsupported_backend()),
        };
        Ok(connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .is_some())
    }

    #[cfg(any(feature = "auth-oidc", feature = "auth-mfa"))]
    pub(crate) async fn enabled_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<bool> {
        let backend = transaction.get_database_backend();
        let table = self.db.quoted_table_name("auth_mfa_factors")?;
        let sql = match backend {
            DbBackend::Postgres => format!("SELECT user_id FROM {table} WHERE user_id = $1"),
            DbBackend::MySql | DbBackend::Sqlite => {
                format!("SELECT user_id FROM {table} WHERE user_id = ?")
            }
            _ => return Err(unsupported_backend()),
        };
        Ok(transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .is_some())
    }

    pub(crate) async fn lock_user_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<()> {
        let backend = transaction.get_database_backend();
        let users = self.db.quoted_table_name("users")?;
        let sql = match backend {
            DbBackend::Postgres => format!("SELECT id FROM {users} WHERE id = $1 FOR UPDATE"),
            DbBackend::MySql => format!("SELECT id FROM {users} WHERE id = ? FOR UPDATE"),
            DbBackend::Sqlite => format!("SELECT id FROM {users} WHERE id = ?"),
            _ => return Err(unsupported_backend()),
        };
        transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        Ok(())
    }

    async fn challenge_limit_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        operation: &str,
        identity: &str,
        source: &str,
        sqlite_limit_preflight: bool,
    ) -> Result<()> {
        if transaction.get_database_backend() == DbBackend::Sqlite {
            if !sqlite_limit_preflight {
                return Err(Error::custom_with_status(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "auth_mfa_preflight_required",
                    "SQLite MFA挑战必须在写事务前完成限流",
                ));
            }
            Ok(())
        } else {
            // PG/MySQL 保留 helper 内原计数位置；SQLite 只接受显式前置证明。
            self.limit(operation, source, identity).await
        }
    }

    /// 在 SQLite 写事务外消耗登录挑战限流；调用方锁内还须重读 MFA enabled 状态。
    pub(crate) async fn limit_login_preflight(&self, user_id: i64, source: &str) -> Result<()> {
        self.limit("login", source, &user_id.to_string()).await
    }

    /// 在 SQLite 挑战事务前消耗限流；其他后端仍在原 helper 位置计数。
    pub(crate) async fn limit_challenge_preflight(&self, token: &str, source: &str) -> Result<()> {
        self.limit("challenge", source, token).await
    }

    #[cfg(any(feature = "auth-oidc", feature = "auth-mfa"))]
    pub(crate) async fn create_challenge_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        source: &str,
        sqlite_limit_preflight: bool,
    ) -> Result<MfaLoginChallenge> {
        self.challenge_limit_in_transaction(
            transaction,
            "login",
            &user_id.to_string(),
            source,
            sqlite_limit_preflight,
        )
        .await?;
        let token = random_secret(32)?;
        let expires_at =
            Utc::now() + chrono::Duration::seconds(CHALLENGE_TTL_SECONDS.cast_signed());
        insert_challenge(
            transaction,
            &self.db,
            user_id,
            &digest(&token),
            &digest(source),
            expires_at,
        )
        .await?;
        Ok(MfaLoginChallenge::new(token, user_id, expires_at))
    }

    pub(crate) async fn complete_challenge_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        token: &str,
        source: &str,
        code: &str,
        sqlite_limit_preflight: bool,
    ) -> Result<Option<i64>> {
        self.challenge_limit_in_transaction(
            transaction,
            "challenge",
            token,
            source,
            sqlite_limit_preflight,
        )
        .await?;
        if token.len() > 128 || token.is_empty() {
            return Ok(None);
        }
        let backend = transaction.get_database_backend();
        let challenges = self.db.quoted_table_name("auth_mfa_challenges")?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "SELECT user_id, source_hash, attempts FROM {challenges} WHERE token_hash = $1 AND expires_at > CURRENT_TIMESTAMP FOR UPDATE"
            ),
            DbBackend::MySql => format!(
                "SELECT user_id, source_hash, attempts FROM {challenges} WHERE token_hash = ? AND expires_at > CURRENT_TIMESTAMP(6) FOR UPDATE"
            ),
            DbBackend::Sqlite => format!(
                "SELECT user_id,source_hash,attempts,expires_at FROM {challenges} WHERE token_hash=?"
            ),
            _ => return Err(unsupported_backend()),
        };
        let Some(row) = transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [digest(token).into()],
            ))
            .await
            .map_err(Error::internal)?
        else {
            return Ok(None);
        };
        // 外层 Immediate 已在首次读取前取得写所有权；只用精确 UTC 日期判期限。
        if backend == DbBackend::Sqlite
            && row
                .try_get::<DateTime<Utc>>("", "expires_at")
                .map_err(Error::internal)?
                <= Utc::now()
        {
            return Ok(None);
        }
        let user_id = row.try_get::<i64>("", "user_id").map_err(Error::internal)?;
        let source_hash = row
            .try_get::<String>("", "source_hash")
            .map_err(Error::internal)?;
        if source_hash != digest(source) {
            return Ok(None);
        }
        let attempts = row
            .try_get::<i32>("", "attempts")
            .map_err(Error::internal)?;
        if attempts >= MAX_CHALLENGE_ATTEMPTS.cast_signed() {
            return Ok(None);
        }
        if self
            .consume_factor_in_transaction(transaction, user_id, code)
            .await?
        {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    backend,
                    format!(
                        "DELETE FROM {challenges} WHERE token_hash = {}",
                        placeholder(backend, 1)
                    ),
                    [digest(token).into()],
                ))
                .await
                .map_err(Error::internal)?;
            Ok(Some(user_id))
        } else {
            record_failed_challenge(transaction, &challenges, &digest(token), attempts).await?;
            Ok(None)
        }
    }

    pub(crate) async fn consume_factor_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        code: &str,
    ) -> Result<bool> {
        let backend = transaction.get_database_backend();
        let factors = self.db.quoted_table_name("auth_mfa_factors")?;
        let sql = match backend {
            DbBackend::Postgres => format!(
                "SELECT encrypted_secret,last_used_step FROM {factors} WHERE user_id = $1 FOR UPDATE"
            ),
            DbBackend::MySql => format!(
                "SELECT encrypted_secret,last_used_step FROM {factors} WHERE user_id = ? FOR UPDATE"
            ),
            DbBackend::Sqlite => {
                format!("SELECT encrypted_secret,last_used_step FROM {factors} WHERE user_id = ?")
            }
            _ => return Err(unsupported_backend()),
        };
        let Some(row) = transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
        else {
            return Ok(false);
        };
        let encrypted = row
            .try_get::<String>("", "encrypted_secret")
            .map_err(Error::internal)?;
        let last = std::cmp::max(
            0,
            row.try_get::<i64>("", "last_used_step")
                .map_err(Error::internal)?,
        )
        .cast_unsigned();
        let secret = Zeroizing::new(
            self.encryptor
                .decrypt_with_context(&encrypted, factor_context(user_id).as_bytes())?,
        );
        if let Some(step) = matching_step(&secret, code, unix_step()?).filter(|step| *step > last) {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    backend,
                    format!(
                        "UPDATE {factors} SET last_used_step = {} WHERE user_id = {}",
                        placeholder(backend, 1),
                        placeholder(backend, 2)
                    ),
                    [
                        <i64 as std::convert::TryFrom<u64>>::try_from(step)
                            .map_err(Error::internal)?
                            .into(),
                        user_id.into(),
                    ],
                ))
                .await
                .map_err(Error::internal)?;
            return Ok(true);
        }
        let hash = digest(code);
        let recovery = self.db.quoted_table_name("auth_mfa_recovery_codes")?;
        let deleted = transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "DELETE FROM {recovery} WHERE user_id = {} AND code_hash = {}",
                    placeholder(backend, 1),
                    placeholder(backend, 2)
                ),
                [user_id.into(), hash.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(deleted.rows_affected() == 1)
    }

    pub(crate) async fn disable(&self, user_id: i64, source: &str, code: &str) -> Result<bool> {
        self.limit("disable", source, &user_id.to_string()).await?;
        let db = self.db.clone();
        let audit = self.audit.clone();
        let code = code.to_owned();
        self.db
            .transaction_for_update(|transaction| {
                let db = db.clone();
                let audit = audit.clone();
                let this = self.clone();
                let code = code.clone();
                Box::pin(async move {
                    this.lock_user_in_transaction(transaction, user_id).await?;
                    if !this
                        .consume_factor_in_transaction(transaction, user_id, &code)
                        .await?
                    {
                        return Ok(false);
                    }
                    let backend = transaction.get_database_backend();
                    for name in [
                        "auth_mfa_challenges",
                        "auth_mfa_recovery_codes",
                        "auth_mfa_factors",
                        "auth_mfa_enrollments",
                    ] {
                        let table = db.quoted_table_name(name)?;
                        let sql = format!(
                            "DELETE FROM {table} WHERE user_id = {}",
                            placeholder(backend, 1)
                        );
                        transaction
                            .execute_raw(Statement::from_sql_and_values(
                                backend,
                                &sql,
                                [user_id.into()],
                            ))
                            .await
                            .map_err(Error::internal)?;
                    }
                    revoke_all_for_user_in_transaction(transaction, &db, user_id).await?;
                    record_mfa_audit(&audit, transaction, user_id, "auth.mfa.disabled").await?;
                    Ok(true)
                })
            })
            .await
    }

    pub(crate) async fn regenerate_recovery_codes(
        &self,
        user_id: i64,
        source: &str,
        code: &str,
    ) -> Result<Option<RecoveryCodes>> {
        self.limit("recovery_reset", source, &user_id.to_string())
            .await?;
        let codes = recovery_codes()?;
        let hashes = codes.iter().map(|code| digest(code)).collect::<Vec<_>>();
        let db = self.db.clone();
        let audit = self.audit.clone();
        let this = self.clone();
        let code = code.to_owned();
        let changed = self
            .db
            .transaction_for_update(|transaction| {
                let db = db.clone();
                let audit = audit.clone();
                let hashes = hashes.clone();
                let this = this.clone();
                let code = code.clone();
                Box::pin(async move {
                    if !this
                        .consume_factor_in_transaction(transaction, user_id, &code)
                        .await?
                    {
                        return Ok(false);
                    }
                    let backend = transaction.get_database_backend();
                    let table = db.quoted_table_name("auth_mfa_recovery_codes")?;
                    transaction
                        .execute_raw(Statement::from_sql_and_values(
                            backend,
                            format!(
                                "DELETE FROM {table} WHERE user_id = {}",
                                placeholder(backend, 1)
                            ),
                            [user_id.into()],
                        ))
                        .await
                        .map_err(Error::internal)?;
                    insert_recovery_hashes(transaction, &db, user_id, &hashes).await?;
                    revoke_all_for_user_in_transaction(transaction, &db, user_id).await?;
                    record_mfa_audit(
                        &audit,
                        transaction,
                        user_id,
                        "auth.mfa.recovery_codes_reset",
                    )
                    .await?;
                    Ok(true)
                })
            })
            .await?;
        Ok(changed.then(|| RecoveryCodes::new(codes)))
    }

    pub(crate) async fn record_login_success_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<()> {
        record_mfa_audit(&self.audit, transaction, user_id, "auth.mfa.login").await
    }

    pub(crate) async fn begin_enrollment(
        &self,
        user_id: i64,
        account: &str,
        issuer: &str,
        source: &str,
    ) -> Result<TotpEnrollment> {
        if user_id <= 0 || issuer.trim().is_empty() || issuer.len() > 100 {
            return Err(Error::BadRequest("invalid TOTP enrollment".into()));
        }
        self.limit("enroll", source, &user_id.to_string()).await?;
        if self.enabled(user_id).await? {
            return Err(Error::custom(
                "auth_mfa_already_enabled",
                "multi-factor authentication is already enabled",
            ));
        }
        let mut seed = Zeroizing::new([0_u8; 20]);
        getrandom::fill(&mut *seed).map_err(Error::internal)?;
        let ciphertext = self
            .encryptor
            .encrypt_with_context(&seed[..], factor_context(user_id).as_bytes())?;
        let expires_at =
            Utc::now() + chrono::Duration::seconds(ENROLLMENT_TTL_SECONDS.cast_signed());
        let db = self.db.clone();
        let expires_at = self.db.transaction_for_update(|transaction| {
            let db = db.clone(); let ciphertext = ciphertext.clone();
            Box::pin(async move {
                let backend = transaction.get_database_backend();
                let pending = db.quoted_table_name("auth_mfa_enrollments")?;
                let delete=match backend{DbBackend::Postgres=>format!("DELETE FROM {pending} WHERE user_id = $1"),DbBackend::MySql|DbBackend::Sqlite=>format!("DELETE FROM {pending} WHERE user_id = ?"),_=>return Err(unsupported_backend())};
                transaction.execute_raw(Statement::from_sql_and_values(backend, &delete, [user_id.into()])).await.map_err(Error::internal)?;
                let insert = match backend {
                    DbBackend::Postgres => format!("INSERT INTO {pending} (user_id, encrypted_secret, expires_at, created_at) VALUES ($1, $2, $3, CURRENT_TIMESTAMP)"),
                    DbBackend::MySql => format!("INSERT INTO {pending} (user_id, encrypted_secret, expires_at, created_at) VALUES (?, ?, ?, CURRENT_TIMESTAMP(6))"),
                    DbBackend::Sqlite => format!("INSERT INTO {pending} (user_id, encrypted_secret, expires_at, created_at) VALUES (?,?,?,?)"),
                    _ => return Err(unsupported_backend()),
                };
                let now = Utc::now();
                let expires_at = if backend == DbBackend::Sqlite { now + chrono::Duration::seconds(ENROLLMENT_TTL_SECONDS.cast_signed()) } else { expires_at };
                let values = if backend == DbBackend::Sqlite {
                    vec![user_id.into(),ciphertext.into(),dialect::utc_value(backend,expires_at),dialect::utc_value(backend,now)]
                } else { vec![user_id.into(),ciphertext.into(),expires_at.naive_utc().into()] };
                transaction.execute_raw(Statement::from_sql_and_values(backend, &insert, values)).await.map_err(Error::internal)?;
                Ok(expires_at)
            })
        }).await?;
        let encoded = base32(&seed[..]);
        let label = format!("{issuer}:{account}");
        let mut uri = url::Url::parse("otpauth://totp/openrust").map_err(Error::internal)?;
        uri.set_path(&format!(
            "/{}",
            url::form_urlencoded::byte_serialize(label.as_bytes()).collect::<String>()
        ));
        uri.query_pairs_mut()
            .append_pair("secret", &encoded)
            .append_pair("issuer", issuer)
            .append_pair("algorithm", "SHA1")
            .append_pair("digits", "6")
            .append_pair("period", "30");
        Ok(TotpEnrollment::new(uri.into(), expires_at))
    }

    pub(crate) async fn confirm_enrollment(
        &self,
        user_id: i64,
        source: &str,
        code: &str,
    ) -> Result<Option<RecoveryCodes>> {
        self.limit("enroll_confirm", source, &user_id.to_string())
            .await?;
        let codes = recovery_codes()?;
        let hashes = codes.iter().map(|code| digest(code)).collect::<Vec<_>>();
        let encrypted = self.load_pending(user_id).await?;
        let seed = Zeroizing::new(
            self.encryptor
                .decrypt_with_context(&encrypted, factor_context(user_id).as_bytes())?,
        );
        let accepted_step = matching_step(&seed, code, unix_step()?);
        let Some(step) = accepted_step else {
            return Err(Error::Unauthorized);
        };
        let db = self.db.clone();
        let audit = self.audit.clone();
        let encrypted = encrypted.clone();
        let code = code.to_owned();
        let inserted = self.db.transaction_for_update(|transaction| {
            let db=db.clone(); let audit=audit.clone(); let encrypted=encrypted.clone(); let hashes=hashes.clone(); let seed=seed.clone(); let code=code.clone();
            Box::pin(async move {
                // 与登录入口共用用户行锁，避免 MFA 启用和密码登录并发时绕过第二因子。
                let users = db.quoted_table_name("users")?;
                let user_lock = match transaction.get_database_backend() {
                    DbBackend::Postgres => format!("SELECT id FROM {users} WHERE id = $1 FOR UPDATE"),
                    DbBackend::MySql => format!("SELECT id FROM {users} WHERE id = ? FOR UPDATE"),
                    DbBackend::Sqlite => format!("SELECT id FROM {users} WHERE id = ?"),
                    _ => return Err(unsupported_backend()),
                };
                transaction.query_one_raw(Statement::from_sql_and_values(transaction.get_database_backend(), &user_lock, [user_id.into()])).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
                let backend=transaction.get_database_backend();
                let pending=db.quoted_table_name("auth_mfa_enrollments")?;
                let pending_sql=match backend { DbBackend::Postgres=>format!("SELECT encrypted_secret FROM {pending} WHERE user_id = $1 AND expires_at > CURRENT_TIMESTAMP FOR UPDATE"), DbBackend::MySql=>format!("SELECT encrypted_secret FROM {pending} WHERE user_id = ? AND expires_at > CURRENT_TIMESTAMP(6) FOR UPDATE"), DbBackend::Sqlite=>format!("SELECT encrypted_secret,expires_at FROM {pending} WHERE user_id=?"), _=>return Err(unsupported_backend()) };
                let Some(pending_row)=transaction.query_one_raw(Statement::from_sql_and_values(backend,&pending_sql,[user_id.into()])).await.map_err(Error::internal)? else{return Ok(false)};
                if pending_row.try_get::<String>("","encrypted_secret").map_err(Error::internal)?!=encrypted{return Ok(false)}
                if backend == DbBackend::Sqlite && pending_row.try_get::<DateTime<Utc>>("","expires_at").map_err(Error::internal)? <= Utc::now() { return Ok(false); }
                let step = if backend == DbBackend::Sqlite { matching_step(&seed,&code,unix_step()?).ok_or(Error::Unauthorized)? } else { step };
                let factors=db.quoted_table_name("auth_mfa_factors")?;
                let insert=match backend { DbBackend::Postgres=>format!("INSERT INTO {factors} (user_id, encrypted_secret, last_used_step, enabled_at) VALUES ($1,$2,$3,CURRENT_TIMESTAMP)"), DbBackend::MySql=>format!("INSERT INTO {factors} (user_id, encrypted_secret, last_used_step, enabled_at) VALUES (?,?,?,CURRENT_TIMESTAMP(6))"), DbBackend::Sqlite=>format!("INSERT INTO {factors} (user_id,encrypted_secret,last_used_step,enabled_at) VALUES (?,?,?,?)"), _=>return Err(unsupported_backend()) };
                let mut values=vec![user_id.into(),encrypted.into(),<i64 as std::convert::TryFrom<u64>>::try_from(step).map_err(Error::internal)?.into()];
                if backend == DbBackend::Sqlite { values.push(dialect::utc_value(backend,Utc::now())); }
                transaction.execute_raw(Statement::from_sql_and_values(backend,&insert,values)).await.map_err(Error::internal)?;
                insert_recovery_hashes(transaction,&db,user_id,&hashes).await?;
                transaction.execute_raw(Statement::from_sql_and_values(backend,format!("DELETE FROM {pending} WHERE user_id = {}",placeholder(backend,1)),[user_id.into()])).await.map_err(Error::internal)?;
                record_mfa_audit(&audit,transaction,user_id,"auth.mfa.enabled").await?;
                revoke_all_for_user_in_transaction(transaction,&db,user_id).await?;
                Ok(true)
            })
        }).await?;
        if inserted {
            Ok(Some(RecoveryCodes::new(codes)))
        } else {
            Ok(None)
        }
    }

    async fn load_pending(&self, user_id: i64) -> Result<String> {
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        let table = self.db.quoted_table_name("auth_mfa_enrollments")?;
        if backend == DbBackend::Sqlite {
            return self
                .db
                .transaction_for_update(|transaction| {
                    Box::pin(async move {
                        let now = Utc::now();
                        let sql = format!(
                            "SELECT encrypted_secret,expires_at FROM {table} WHERE user_id=?"
                        );
                        let row = transaction
                            .query_one_raw(Statement::from_sql_and_values(
                                DbBackend::Sqlite,
                                sql,
                                [user_id.into()],
                            ))
                            .await
                            .map_err(Error::internal)?
                            .ok_or(Error::Unauthorized)?;
                        if row
                            .try_get::<DateTime<Utc>>("", "expires_at")
                            .map_err(Error::internal)?
                            <= now
                        {
                            return Err(Error::Unauthorized);
                        }
                        row.try_get("", "encrypted_secret").map_err(Error::internal)
                    })
                })
                .await;
        }
        let sql = match backend {
            DbBackend::Postgres => format!(
                "SELECT encrypted_secret FROM {table} WHERE user_id = $1 AND expires_at > CURRENT_TIMESTAMP"
            ),
            DbBackend::MySql => format!(
                "SELECT encrypted_secret FROM {table} WHERE user_id = ? AND expires_at > CURRENT_TIMESTAMP(6)"
            ),
            _ => return Err(unsupported_backend()),
        };
        connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?
            .try_get("", "encrypted_secret")
            .map_err(Error::internal)
    }

    async fn limit(&self, operation: &str, source: &str, identity: &str) -> Result<()> {
        if source.is_empty()
            || source.len() > 160
            || source.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(Error::BadRequest("invalid trusted request source".into()));
        }
        let key = format!("auth:mfa:{operation}:{source}:{}", digest(identity));
        self.limiter.check(&key, 5, RATE_LIMIT_WINDOW).await?;
        Ok(())
    }

    async fn check_schema(&self) -> Result<()> {
        let connection = self.db.connection()?;
        let backend = connection.get_database_backend();
        for name in [
            "auth_mfa_enrollments",
            "auth_mfa_factors",
            "auth_mfa_recovery_codes",
            "auth_mfa_challenges",
            "audit_records",
        ] {
            let table = self.db.quoted_table_name(name)?;
            let column = if name == "auth_mfa_challenges" {
                "token_hash"
            } else if name == "audit_records" {
                "id"
            } else {
                "user_id"
            };
            let sql = format!("SELECT {column} FROM {table} WHERE 1 = 0");
            connection
                .query_all_raw(Statement::from_string(backend, sql))
                .await
                .map_err(Error::internal)?;
        }
        Ok(())
    }
}

/// 通过统一容器装配多因素认证服务。
#[derive(Debug, Clone, Copy, Default)]
pub struct MfaServiceProvider;

impl crate::core::ServiceProvider for MfaServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::core::RegisterContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<MfaService, _>(|resolver| {
                Box::pin(async move {
                    let db = resolver.resolve::<Db>().await?;
                    resolver.resolve::<Passwords>().await?;
                    let encryptor = resolver.resolve::<Encryptor>().await?;
                    let limiter = resolver.resolve::<RateLimiter>().await?;
                    Ok(std::sync::Arc::new(MfaService::new(
                        (*db).clone(),
                        (*encryptor).clone(),
                        (*limiter).clone(),
                    )))
                })
            })
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let service = app.resolve::<MfaService>().await?;
            if app.is_console_command("migrate") {
                return app.defer_boot_check_for_command(
                    crate::db::migration::framework_migration_command_id(),
                    move || {
                        let service = std::sync::Arc::clone(&service);
                        Box::pin(async move { service.check_schema().await })
                    },
                );
            }
            service.check_schema().await
        })
    }
}

async fn record_failed_challenge(
    transaction: &DatabaseTransaction,
    table: &str,
    token_hash: &str,
    attempts: i32,
) -> Result<()> {
    let backend = transaction.get_database_backend();
    let next = attempts.saturating_add(1);
    let (sql, values) = if next >= MAX_CHALLENGE_ATTEMPTS.cast_signed() {
        (
            format!(
                "DELETE FROM {table} WHERE token_hash = {}",
                placeholder(backend, 1)
            ),
            vec![token_hash.into()],
        )
    } else {
        (
            format!(
                "UPDATE {table} SET attempts = {} WHERE token_hash = {}",
                placeholder(backend, 1),
                placeholder(backend, 2)
            ),
            vec![next.into(), token_hash.into()],
        )
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(backend, sql, values))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn factor_context(user_id: i64) -> String {
    format!("openrust:auth:mfa:totp:{user_id}")
}
fn random_secret(bytes: usize) -> Result<String> {
    let mut value = vec![0; bytes];
    getrandom::fill(&mut value).map_err(Error::internal)?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}
fn recovery_codes() -> Result<Vec<String>> {
    (0..RECOVERY_CODE_COUNT)
        .map(|_| random_secret(20))
        .collect()
}
fn unix_step() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(Error::internal)?
        .as_secs()
        / TOTP_PERIOD_SECONDS)
}
fn matching_step(secret: &[u8], code: &str, step: u64) -> Option<u64> {
    if code.len() != TOTP_DIGITS as usize || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let first = step.saturating_sub(1);
    let last = step.saturating_add(1);
    (first..=last).rev().find(|candidate| {
        totp_code(secret, *candidate, TOTP_DIGITS)
            .is_ok_and(|expected| constant_time_code_eq(code, &expected))
    })
}
fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        output.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
    output
}
fn placeholder(backend: DbBackend, index: usize) -> String {
    if backend == DbBackend::Postgres {
        format!("${index}")
    } else {
        "?".to_owned()
    }
}
fn migration_table(logical: &str, backend: DbBackend) -> std::result::Result<Alias, DbErr> {
    let names = DatabaseTableNames::current().ok_or_else(|| {
        DbErr::Migration(
            "OpenRust migration requires MigrationServiceProvider or TestDatabase table-name scope"
                .into(),
        )
    })?;
    names
        .quote(logical, backend)
        .map_err(|error| DbErr::Migration(error.to_string()))?;
    names
        .resolve(logical)
        .map(Alias::new)
        .map_err(|error| DbErr::Migration(error.to_string()))
}
fn unsupported_backend() -> Error {
    Error::custom(
        "unsupported_auth_database",
        "multi-factor authentication requires PostgreSQL, MySQL or SQLite",
    )
}
async fn insert_recovery_hashes(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    hashes: &[String],
) -> Result<()> {
    let backend = transaction.get_database_backend();
    let table = db.quoted_table_name("auth_mfa_recovery_codes")?;
    let sql = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (user_id, code_hash, created_at) VALUES ($1,$2,CURRENT_TIMESTAMP)"
        ),
        DbBackend::MySql => format!(
            "INSERT INTO {table} (user_id, code_hash, created_at) VALUES (?,?,CURRENT_TIMESTAMP(6))"
        ),
        DbBackend::Sqlite => {
            format!("INSERT INTO {table} (user_id,code_hash,created_at) VALUES (?,?,?)")
        }
        _ => return Err(unsupported_backend()),
    };
    for hash in hashes {
        transaction
            .execute_raw(Statement::from_sql_and_values(
                backend,
                &sql,
                if backend == DbBackend::Sqlite {
                    vec![
                        user_id.into(),
                        hash.clone().into(),
                        dialect::utc_value(backend, Utc::now()),
                    ]
                } else {
                    vec![user_id.into(), hash.clone().into()]
                },
            ))
            .await
            .map_err(Error::internal)?;
    }
    Ok(())
}
async fn record_mfa_audit(
    audit: &DatabaseAuditSink,
    transaction: &DatabaseTransaction,
    user_id: i64,
    action: &str,
) -> Result<()> {
    let event = AuditEvent::new(
        action,
        AuditSubject::new("user", user_id.to_string())?,
        AuditOutcome::Succeeded,
    )?
    .actor(AuditSubject::new("user", user_id.to_string())?);
    let record = Audit::new(audit.clone(), AuditPolicy::default()).prepare(event)?;
    audit.record_in_transaction(transaction, &record).await
}
async fn insert_challenge(
    transaction: &DatabaseTransaction,
    db: &Db,
    user_id: i64,
    token_hash: &str,
    source_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<()> {
    let backend = transaction.get_database_backend();
    let table = db.quoted_table_name("auth_mfa_challenges")?;
    let sql = match backend {
        DbBackend::Postgres => format!(
            "INSERT INTO {table} (token_hash,user_id,source_hash,attempts,expires_at,created_at) VALUES ($1,$2,$3,0,$4,CURRENT_TIMESTAMP)"
        ),
        DbBackend::MySql => format!(
            "INSERT INTO {table} (token_hash,user_id,source_hash,attempts,expires_at,created_at) VALUES (?,?,?,0,?,CURRENT_TIMESTAMP(6))"
        ),
        DbBackend::Sqlite => format!(
            "INSERT INTO {table} (token_hash,user_id,source_hash,attempts,expires_at,created_at) VALUES (?,?,?,0,?,?)"
        ),
        _ => return Err(unsupported_backend()),
    };
    transaction
        .execute_raw(Statement::from_sql_and_values(
            backend,
            &sql,
            if backend == DbBackend::Sqlite {
                vec![
                    token_hash.into(),
                    user_id.into(),
                    source_hash.into(),
                    dialect::utc_value(backend, expires_at),
                    dialect::utc_value(backend, Utc::now()),
                ]
            } else {
                vec![
                    token_hash.into(),
                    user_id.into(),
                    source_hash.into(),
                    expires_at.naive_utc().into(),
                ]
            },
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}
#[derive(DeriveIden)]
#[allow(dead_code)]
enum MfaEnrollments {
    Table,
    UserId,
    EncryptedSecret,
    ExpiresAt,
    CreatedAt,
}
#[derive(DeriveIden)]
#[allow(dead_code)]
enum MfaFactors {
    Table,
    UserId,
    EncryptedSecret,
    LastUsedStep,
    EnabledAt,
}
#[derive(DeriveIden)]
#[allow(dead_code)]
enum MfaRecoveryCodes {
    Table,
    Id,
    UserId,
    CodeHash,
    CreatedAt,
}
#[derive(DeriveIden)]
#[allow(dead_code)]
enum MfaChallenges {
    Table,
    TokenHash,
    UserId,
    SourceHash,
    Attempts,
    ExpiresAt,
    CreatedAt,
}

/// 创建账户 TOTP 因子、待确认 enrollment、恢复码和短时登录 challenge 表。
pub struct AuthMfaMigration;

impl MigrationName for AuthMfaMigration {
    fn name(&self) -> &'static str {
        "m20261027_000007_openrust_auth_mfa"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AuthMfaMigration {
    #[allow(
        clippy::too_many_lines,
        reason = "MFA 生命周期相关四张表和约束需由同一追加迁移创建"
    )]
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let users = migration_table("users", backend)?;
        let enrollments = migration_table("auth_mfa_enrollments", backend)?;
        let factors = migration_table("auth_mfa_factors", backend)?;
        let recovery = migration_table("auth_mfa_recovery_codes", backend)?;
        let challenges = migration_table("auth_mfa_challenges", backend)?;
        manager
            .create_table(
                Table::create()
                    .table(enrollments.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MfaEnrollments::UserId)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MfaEnrollments::EncryptedSecret)
                            .text()
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(MfaEnrollments::ExpiresAt, backend)?)
                    .col(dialect::timestamp_utc(MfaEnrollments::CreatedAt, backend)?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_mfa_enrollment_user")
                            .from(enrollments.clone(), MfaEnrollments::UserId)
                            .to(
                                users.clone(),
                                crate::db::sea_orm::sea_query::Alias::new("id"),
                            )
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(factors.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MfaFactors::UserId)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MfaFactors::EncryptedSecret)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MfaFactors::LastUsedStep)
                            .big_integer()
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(MfaFactors::EnabledAt, backend)?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_mfa_factor_user")
                            .from(factors.clone(), MfaFactors::UserId)
                            .to(
                                users.clone(),
                                crate::db::sea_orm::sea_query::Alias::new("id"),
                            )
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(recovery.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MfaRecoveryCodes::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MfaRecoveryCodes::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MfaRecoveryCodes::CodeHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(dialect::timestamp_utc(
                        MfaRecoveryCodes::CreatedAt,
                        backend,
                    )?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_mfa_recovery_user")
                            .from(recovery.clone(), MfaRecoveryCodes::UserId)
                            .to(
                                users.clone(),
                                crate::db::sea_orm::sea_query::Alias::new("id"),
                            )
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(recovery.clone())
                    .name("uq_tb_mfa_recovery_hash")
                    .unique()
                    .col(MfaRecoveryCodes::CodeHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(recovery.clone())
                    .name("ix_tb_mfa_recovery_user")
                    .col(MfaRecoveryCodes::UserId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(challenges.clone())
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MfaChallenges::TokenHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MfaChallenges::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MfaChallenges::SourceHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(MfaChallenges::Attempts).integer().not_null())
                    .col(dialect::timestamp_utc(MfaChallenges::ExpiresAt, backend)?)
                    .col(dialect::timestamp_utc(MfaChallenges::CreatedAt, backend)?)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tb_mfa_challenge_user")
                            .from(challenges.clone(), MfaChallenges::UserId)
                            .to(users, crate::db::sea_orm::sea_query::Alias::new("id"))
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .table(challenges.clone())
                    .name("ix_tb_mfa_challenge_user")
                    .col(MfaChallenges::UserId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let backend = manager.get_database_backend();
        for table in [
            "auth_mfa_challenges",
            "auth_mfa_recovery_codes",
            "auth_mfa_factors",
            "auth_mfa_enrollments",
        ] {
            manager
                .drop_table(
                    Table::drop()
                        .table(migration_table(table, backend)?)
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{constant_time_code_eq, matching_step, totp_code};
    use crate::Result;

    #[test]
    fn totp_matches_rfc_6238_sha1_known_vectors() -> Result<()> {
        let secret = b"12345678901234567890";
        let vectors = [
            (59, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ];
        for (unix_seconds, expected) in vectors {
            assert_eq!(totp_code(secret, unix_seconds / 30, 8)?, expected);
        }
        Ok(())
    }

    #[test]
    fn code_comparison_checks_length_and_value() {
        assert!(constant_time_code_eq("004201", "004201"));
        assert!(!constant_time_code_eq("004201", "4201"));
        assert!(!constant_time_code_eq("004201", "004202"));
    }

    #[test]
    fn matching_step_accepts_only_the_adjacent_window_and_six_ascii_digits() -> Result<()> {
        let secret = b"0123456789abcdef0123";
        for matched in [99, 100, 101] {
            let code = totp_code(secret, matched, 6)?;
            assert_eq!(matching_step(secret, &code, 100), Some(matched));
        }
        for outside in [98, 102] {
            let code = totp_code(secret, outside, 6)?;
            assert_eq!(matching_step(secret, &code, 100), None);
        }
        assert_eq!(matching_step(secret, "12345", 100), None);
        assert_eq!(matching_step(secret, "12a456", 100), None);
        Ok(())
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::core::ProviderBuilder;

    #[tokio::test]
    async fn missing_dependencies_fail_startup() {
        let mut builder = ProviderBuilder::new();
        builder.provider(MfaServiceProvider);
        assert!(builder.start(figment::Figment::new()).await.is_err());
        let mut migration = ProviderBuilder::new();
        migration
            .console_command("migrate")
            .provider(MfaServiceProvider);
        assert!(migration.start(figment::Figment::new()).await.is_err());
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod sqlite_tests;
