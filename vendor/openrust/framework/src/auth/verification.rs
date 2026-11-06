//! 用途绑定的一次性验证码及其存储契约。

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::core::{Error, Result};
use crate::database::sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait,
};
use sha2::Digest;

use crate::auth::EncodedPasswordHash;

mod sqlite;

const MIN_DIGITS: u8 = 6;
const MAX_DIGITS: u8 = 10;
const MAX_LIFETIME_SECS: u64 = 86_400;
const MAX_ATTEMPTS: u32 = 10;

/// [`VerificationCodeStore`] 返回的 future。
pub type VerificationCodeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 单个验证码的有效期、重发间隔和尝试次数策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationCodePolicy {
    digits: u8,
    lifetime: Duration,
    resend_after: Duration,
    max_attempts: u32,
}

impl VerificationCodePolicy {
    /// 创建具有明确资源边界的数字验证码策略。
    pub fn new(
        digits: u8,
        lifetime: Duration,
        resend_after: Duration,
        max_attempts: u32,
    ) -> Result<Self> {
        let policy = Self {
            digits,
            lifetime,
            resend_after,
            max_attempts,
        };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(self) -> Result<()> {
        let valid_duration = |duration: Duration| {
            (1..=MAX_LIFETIME_SECS).contains(&duration.as_secs()) && duration.subsec_nanos() == 0
        };
        if !(MIN_DIGITS..=MAX_DIGITS).contains(&self.digits)
            || !valid_duration(self.lifetime)
            || !valid_duration(self.resend_after)
            || !(1..=MAX_ATTEMPTS).contains(&self.max_attempts)
        {
            return Err(Error::custom(
                "invalid_verification_code_policy",
                "verification code policy has an unsupported digit count, duration, or attempt limit",
            ));
        }
        Ok(())
    }

    /// 返回新签发验证码的十进制位数。
    pub const fn digits(self) -> u8 {
        self.digits
    }

    /// 返回验证码有效期。
    pub const fn lifetime(self) -> Duration {
        self.lifetime
    }

    /// 返回同一用途和身份两次签发之间的最短间隔。
    pub const fn resend_after(self) -> Duration {
        self.resend_after
    }

    /// 返回验证码允许的最多错误尝试次数。
    pub const fn max_attempts(self) -> u32 {
        self.max_attempts
    }
}

impl Default for VerificationCodePolicy {
    fn default() -> Self {
        Self {
            digits: 6,
            lifetime: Duration::from_secs(600),
            resend_after: Duration::from_secs(60),
            max_attempts: 5,
        }
    }
}

/// 新签发的验证码，仅向调用方暴露一次，以便通过 Mail 或 SMS 发送。
pub struct IssuedVerificationCode {
    code: String,
}

impl IssuedVerificationCode {
    /// 返回数字验证码。调用方应及时发送，且绝不能将其写入日志。
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Debug for IssuedVerificationCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedVerificationCode")
            .field("code", &"[REDACTED]")
            .finish()
    }
}

/// 自定义 store 为密码校验返回的一个活动挑战。
///
/// 此类型包含验证码哈希，因此有意不实现 `Debug`。
pub struct VerificationChallenge {
    challenge_id: String,
    code_hash: EncodedPasswordHash,
    digits: u8,
}

impl VerificationChallenge {
    /// 为自定义 store 实现创建已加载的挑战。
    pub fn new(
        challenge_id: impl Into<String>,
        code_hash: EncodedPasswordHash,
        digits: u8,
    ) -> Result<Self> {
        let challenge_id = challenge_id.into();
        if challenge_id.is_empty()
            || challenge_id.len() > 64
            || !(MIN_DIGITS..=MAX_DIGITS).contains(&digits)
        {
            return Err(Error::internal(InvalidVerificationChallenge));
        }
        Ok(Self {
            challenge_id,
            code_hash,
            digits,
        })
    }

    /// 返回 store 的不透明挑战版本 ID。
    pub fn challenge_id(&self) -> &str {
        &self.challenge_id
    }

    /// 返回 store 为当前活动挑战加载的 PHC 哈希。
    pub fn code_hash(&self) -> &EncodedPasswordHash {
        &self.code_hash
    }

    /// 返回预期验证码长度。
    pub const fn digits(&self) -> u8 {
        self.digits
    }
}

/// 共享一次性验证挑战的原子存储契约。
///
/// `key_hash` 是用途与调用方规范化身份的 SHA-256 摘要。Store 不得暴露或记录原始身份或验证码。
/// 实现必须在其声明的共享范围内协调签发冷却期、失败次数和单次消费。
pub trait VerificationCodeStore: Send + Sync + 'static {
    /// 创建或替换挑战；现有挑战处于重发冷却期时除外。
    fn issue<'a>(
        &'a self,
        key_hash: &'a str,
        challenge_id: &'a str,
        code_hash: &'a EncodedPasswordHash,
        policy: VerificationCodePolicy,
    ) -> VerificationCodeFuture<'a, Result<()>>;

    /// 加载当前尚未消费且仍有尝试次数的挑战。
    fn load_active<'a>(
        &'a self,
        key_hash: &'a str,
    ) -> VerificationCodeFuture<'a, Result<Option<VerificationChallenge>>>;

    /// 记录一次尝试；仅当挑战仍为当前挑战且有效时才消费它。
    fn finish_attempt<'a>(
        &'a self,
        key_hash: &'a str,
        challenge_id: &'a str,
        code_matches: bool,
    ) -> VerificationCodeFuture<'a, Result<bool>>;
}

/// 用于签发并原子消费用途绑定验证码的可克隆服务。
#[derive(Clone)]
pub struct VerificationCodes {
    store: Arc<dyn VerificationCodeStore>,
    passwords: std::sync::Arc<dyn crate::hashing::Hasher>,
}

impl fmt::Debug for VerificationCodes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerificationCodes")
    }
}

impl VerificationCodes {
    /// 使用自定义或框架提供的 store 与密码哈希器创建服务。
    pub fn new(
        store: impl VerificationCodeStore,
        passwords: std::sync::Arc<dyn crate::hashing::Hasher>,
    ) -> Self {
        Self {
            store: Arc::new(store),
            passwords,
        }
    }

    /// 为指定用途和调用方规范化后的身份签发数字验证码。
    ///
    /// 返回的验证码不会自动发送。调用方应先应用共享的 [`crate::core::RateLimiter`] 策略，再通过 Mail/SMS
    /// 发送验证码，并且绝不能将其写入日志。在策略冷却期结束前重新签发会返回带 `Retry-After` 的 429。
    pub async fn issue(
        &self,
        purpose: &str,
        identity: &str,
        policy: VerificationCodePolicy,
    ) -> Result<IssuedVerificationCode> {
        validate_binding(purpose, identity)?;
        policy.validate()?;
        let code = generate_code(policy.digits)?;
        let code_hash =
            crate::auth::EncodedPasswordHash::make(self.passwords.as_ref(), &code).await?;
        let challenge_id = random_hex(16)?;
        let key_hash = binding_hash(purpose, identity);
        self.store
            .issue(&key_hash, &challenge_id, &code_hash, policy)
            .await?;
        Ok(IssuedVerificationCode { code })
    }

    /// 验证一个验证码，并在成功时原子消费。
    ///
    /// 缺失、过期、已消费、尝试次数耗尽、过期版本或不匹配的挑战均返回 `Ok(false)`。Store 或哈希失败会作为服务错误传播。
    pub async fn verify(&self, purpose: &str, identity: &str, code: &str) -> Result<bool> {
        validate_binding(purpose, identity)?;
        let key_hash = binding_hash(purpose, identity);
        let Some(challenge) = self.store.load_active(&key_hash).await? else {
            return Ok(false);
        };
        let code_matches = code.len() == usize::from(challenge.digits)
            && code.bytes().all(|byte| byte.is_ascii_digit())
            && challenge
                .code_hash
                .verify(self.passwords.as_ref(), code)
                .await?;
        self.store
            .finish_attempt(&key_hash, &challenge.challenge_id, code_matches)
            .await
    }
}

fn validate_binding(purpose: &str, identity: &str) -> Result<()> {
    let valid_purpose = !purpose.is_empty()
        && purpose.len() <= 64
        && purpose
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'));
    if !valid_purpose
        || identity.is_empty()
        || identity.len() > 320
        || identity.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(Error::custom(
            "invalid_verification_binding",
            "verification purpose or identity is invalid",
        ));
    }
    Ok(())
}

fn binding_hash(purpose: &str, identity: &str) -> String {
    let mut binding = Vec::with_capacity(purpose.len() + identity.len() + 1);
    binding.extend_from_slice(purpose.as_bytes());
    binding.push(0);
    binding.extend_from_slice(identity.as_bytes());
    sha256_hex(&binding)
}

fn generate_code(digits: u8) -> Result<String> {
    let range = 10_u32.pow(u32::from(digits));
    let acceptance_bound = u32::MAX - (u32::MAX % range);
    loop {
        let mut bytes = [0_u8; 4];
        getrandom::fill(&mut bytes).map_err(Error::internal)?;
        let value = u32::from_be_bytes(bytes);
        if value < acceptance_bound {
            return Ok(format!(
                "{:0width$}",
                value % range,
                width = usize::from(digits)
            ));
        }
    }
}

fn random_hex(length: usize) -> Result<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = vec![0_u8; length];
    getrandom::fill(&mut bytes).map_err(Error::internal)?;
    let mut output = String::with_capacity(length * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(output)
}

fn sha256_hex(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = sha2::Sha256::digest(value);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[derive(Debug)]
struct InvalidVerificationChallenge;

impl fmt::Display for InvalidVerificationChallenge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("verification store returned an invalid challenge")
    }
}

impl std::error::Error for InvalidVerificationChallenge {}

/// [`VerificationCodes`] 使用的共享数据库 store。
#[derive(Clone)]
pub struct DatabaseVerificationCodeStore {
    db: crate::database::Db,
    cleanup_counter: Arc<AtomicU64>,
}

impl fmt::Debug for DatabaseVerificationCodeStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DatabaseVerificationCodeStore")
    }
}

impl DatabaseVerificationCodeStore {
    /// 使用应用共享数据库和表名解析器创建 store。
    pub fn new(db: crate::database::Db) -> Self {
        Self {
            db,
            cleanup_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    async fn maybe_prune(&self, connection: &DatabaseConnection) -> Result<()> {
        if self
            .cleanup_counter
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(256)
        {
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("verification_codes")?;
            if backend == DbBackend::Sqlite {
                return sqlite::prune(connection, &table).await;
            }
            let sql = match backend {
                DbBackend::Postgres => format!(
                    "DELETE FROM {table} WHERE key_hash IN (SELECT key_hash FROM {table} WHERE expires_at <= CURRENT_TIMESTAMP LIMIT 512 FOR UPDATE SKIP LOCKED)"
                ),
                DbBackend::MySql => format!(
                    "DELETE FROM {table} WHERE expires_at <= CURRENT_TIMESTAMP(6) ORDER BY expires_at LIMIT 512"
                ),
                _ => {
                    return Err(Error::custom(
                        "unsupported_verification_backend",
                        "verification codes require PostgreSQL or MySQL",
                    ));
                }
            };
            connection
                .execute_raw(Statement::from_string(backend, sql))
                .await
                .map_err(Error::internal)?;
        }
        Ok(())
    }
}

impl VerificationCodeStore for DatabaseVerificationCodeStore {
    #[allow(clippy::too_many_lines)] // Backend SQL must share one transaction and cooldown decision.
    fn issue<'a>(
        &'a self,
        key_hash: &'a str,
        challenge_id: &'a str,
        code_hash: &'a EncodedPasswordHash,
        policy: VerificationCodePolicy,
    ) -> VerificationCodeFuture<'a, Result<()>> {
        Box::pin(async move {
            let connection = self.db.connection()?;
            self.maybe_prune(&connection).await?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("verification_codes")?;
            if backend == DbBackend::Sqlite {
                return sqlite::issue(
                    &connection,
                    &table,
                    key_hash,
                    challenge_id,
                    code_hash,
                    policy,
                )
                .await;
            }
            let transaction = connection.begin().await.map_err(Error::internal)?;
            let digits = i32::from(policy.digits);
            let max_attempts = i32::try_from(policy.max_attempts).map_err(Error::internal)?;
            let lifetime = i64::try_from(policy.lifetime.as_secs()).map_err(Error::internal)?;
            let resend_after =
                i64::try_from(policy.resend_after.as_secs()).map_err(Error::internal)?;
            let insert = match backend {
                DbBackend::Postgres => format!(
                    "INSERT INTO {table} (key_hash, challenge_id, code_hash, code_digits, attempt_count, max_attempts, issued_at, expires_at, resend_after_at, consumed_at) VALUES ($1, $2, $3, $4, 0, $5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP + ($6::double precision * INTERVAL '1 second'), CURRENT_TIMESTAMP + ($7::double precision * INTERVAL '1 second'), NULL) ON CONFLICT (key_hash) DO NOTHING"
                ),
                DbBackend::MySql => format!(
                    "INSERT INTO {table} (key_hash, challenge_id, code_hash, code_digits, attempt_count, max_attempts, issued_at, expires_at, resend_after_at, consumed_at) VALUES (?, ?, ?, ?, 0, ?, CURRENT_TIMESTAMP(6), DATE_ADD(CURRENT_TIMESTAMP(6), INTERVAL ? SECOND), DATE_ADD(CURRENT_TIMESTAMP(6), INTERVAL ? SECOND), NULL) ON DUPLICATE KEY UPDATE key_hash = VALUES(key_hash)"
                ),
                _ => {
                    return Err(Error::custom(
                        "unsupported_verification_backend",
                        "verification codes require PostgreSQL or MySQL",
                    ));
                }
            };
            let values = [
                key_hash.to_owned().into(),
                challenge_id.to_owned().into(),
                code_hash.as_str().to_owned().into(),
                digits.into(),
                max_attempts.into(),
                lifetime.into(),
                resend_after.into(),
            ];
            transaction
                .execute_raw(Statement::from_sql_and_values(backend, insert, values))
                .await
                .map_err(Error::internal)?;
            let (select, select_values) = match backend {
                DbBackend::Postgres => (
                    format!(
                        "SELECT challenge_id, GREATEST(0, CEIL(EXTRACT(EPOCH FROM (resend_after_at - CURRENT_TIMESTAMP))))::BIGINT AS wait_seconds FROM {table} WHERE key_hash = $1 FOR UPDATE"
                    ),
                    vec![key_hash.to_owned().into()],
                ),
                DbBackend::MySql => (
                    format!(
                        "SELECT challenge_id, CAST(GREATEST(0, CEIL(TIMESTAMPDIFF(MICROSECOND, CURRENT_TIMESTAMP(6), resend_after_at) / 1000000)) AS SIGNED) AS wait_seconds FROM {table} WHERE key_hash = ? FOR UPDATE"
                    ),
                    vec![key_hash.to_owned().into()],
                ),
                _ => return Err(unsupported_verification_backend()),
            };
            let row = transaction
                .query_one_raw(Statement::from_sql_and_values(
                    backend,
                    select,
                    select_values,
                ))
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| Error::internal(VerificationStoreError))?;
            let current_id = row
                .try_get::<String>("", "challenge_id")
                .map_err(Error::internal)?;
            if current_id != challenge_id {
                let wait_seconds = row
                    .try_get::<i64>("", "wait_seconds")
                    .map_err(Error::internal)?;
                if wait_seconds > 0 {
                    transaction.rollback().await.map_err(Error::internal)?;
                    let retry_after = u64::try_from(wait_seconds).map_err(Error::internal)?;
                    return Err(Error::rate_limited(retry_after.max(1)));
                }
                let update = match backend {
                    DbBackend::Postgres => format!(
                        "UPDATE {table} SET challenge_id = $2, code_hash = $3, code_digits = $4, attempt_count = 0, max_attempts = $5, issued_at = CURRENT_TIMESTAMP, expires_at = CURRENT_TIMESTAMP + ($6::double precision * INTERVAL '1 second'), resend_after_at = CURRENT_TIMESTAMP + ($7::double precision * INTERVAL '1 second'), consumed_at = NULL WHERE key_hash = $1"
                    ),
                    DbBackend::MySql => format!(
                        "UPDATE {table} SET challenge_id = ?, code_hash = ?, code_digits = ?, attempt_count = 0, max_attempts = ?, issued_at = CURRENT_TIMESTAMP(6), expires_at = DATE_ADD(CURRENT_TIMESTAMP(6), INTERVAL ? SECOND), resend_after_at = DATE_ADD(CURRENT_TIMESTAMP(6), INTERVAL ? SECOND), consumed_at = NULL WHERE key_hash = ?"
                    ),
                    _ => return Err(unsupported_verification_backend()),
                };
                let values = match backend {
                    DbBackend::Postgres => vec![
                        key_hash.to_owned().into(),
                        challenge_id.to_owned().into(),
                        code_hash.as_str().to_owned().into(),
                        digits.into(),
                        max_attempts.into(),
                        lifetime.into(),
                        resend_after.into(),
                    ],
                    DbBackend::MySql => vec![
                        challenge_id.to_owned().into(),
                        code_hash.as_str().to_owned().into(),
                        digits.into(),
                        max_attempts.into(),
                        lifetime.into(),
                        resend_after.into(),
                        key_hash.to_owned().into(),
                    ],
                    _ => return Err(unsupported_verification_backend()),
                };
                transaction
                    .execute_raw(Statement::from_sql_and_values(backend, update, values))
                    .await
                    .map_err(Error::internal)?;
            }
            transaction.commit().await.map_err(Error::internal)
        })
    }

    fn load_active<'a>(
        &'a self,
        key_hash: &'a str,
    ) -> VerificationCodeFuture<'a, Result<Option<VerificationChallenge>>> {
        Box::pin(async move {
            let connection = self.db.connection()?;
            self.maybe_prune(&connection).await?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("verification_codes")?;
            if backend == DbBackend::Sqlite {
                return sqlite::load_active(&connection, &table, key_hash).await;
            }
            let (sql, values) = match backend {
                DbBackend::Postgres => (
                    format!(
                        "SELECT challenge_id, code_hash, code_digits FROM {table} WHERE key_hash = $1 AND consumed_at IS NULL AND expires_at > CURRENT_TIMESTAMP AND attempt_count < max_attempts"
                    ),
                    vec![key_hash.to_owned().into()],
                ),
                DbBackend::MySql => (
                    format!(
                        "SELECT challenge_id, code_hash, code_digits FROM {table} WHERE key_hash = ? AND consumed_at IS NULL AND expires_at > CURRENT_TIMESTAMP(6) AND attempt_count < max_attempts"
                    ),
                    vec![key_hash.to_owned().into()],
                ),
                _ => {
                    return Err(Error::custom(
                        "unsupported_verification_backend",
                        "verification codes require PostgreSQL or MySQL",
                    ));
                }
            };
            let Some(row) = connection
                .query_one_raw(Statement::from_sql_and_values(backend, sql, values))
                .await
                .map_err(Error::internal)?
            else {
                return Ok(None);
            };
            let challenge_id = row
                .try_get::<String>("", "challenge_id")
                .map_err(Error::internal)?;
            let encoded = row
                .try_get::<String>("", "code_hash")
                .map_err(Error::internal)?;
            let digits = row
                .try_get::<i32>("", "code_digits")
                .map_err(Error::internal)?;
            let digits = u8::try_from(digits).map_err(Error::internal)?;
            Ok(Some(VerificationChallenge::new(
                challenge_id,
                EncodedPasswordHash::from_encoded(encoded),
                digits,
            )?))
        })
    }

    fn finish_attempt<'a>(
        &'a self,
        key_hash: &'a str,
        challenge_id: &'a str,
        code_matches: bool,
    ) -> VerificationCodeFuture<'a, Result<bool>> {
        Box::pin(async move {
            let connection = self.db.connection()?;
            self.maybe_prune(&connection).await?;
            let backend = connection.get_database_backend();
            let table = self.db.quoted_table_name("verification_codes")?;
            if backend == DbBackend::Sqlite {
                return sqlite::finish_attempt(
                    &connection,
                    &table,
                    key_hash,
                    challenge_id,
                    code_matches,
                )
                .await;
            }
            let (sql, values) = match backend {
                DbBackend::Postgres => (
                    format!(
                        "UPDATE {table} SET attempt_count = attempt_count + CASE WHEN $3 THEN 0 ELSE 1 END, consumed_at = CASE WHEN $3 THEN CURRENT_TIMESTAMP ELSE consumed_at END WHERE key_hash = $1 AND challenge_id = $2 AND consumed_at IS NULL AND expires_at > CURRENT_TIMESTAMP AND attempt_count < max_attempts"
                    ),
                    vec![
                        key_hash.to_owned().into(),
                        challenge_id.to_owned().into(),
                        code_matches.into(),
                    ],
                ),
                DbBackend::MySql => (
                    format!(
                        "UPDATE {table} SET attempt_count = attempt_count + IF(?, 0, 1), consumed_at = IF(?, CURRENT_TIMESTAMP(6), consumed_at) WHERE key_hash = ? AND challenge_id = ? AND consumed_at IS NULL AND expires_at > CURRENT_TIMESTAMP(6) AND attempt_count < max_attempts"
                    ),
                    vec![
                        code_matches.into(),
                        code_matches.into(),
                        key_hash.to_owned().into(),
                        challenge_id.to_owned().into(),
                    ],
                ),
                _ => {
                    return Err(Error::custom(
                        "unsupported_verification_backend",
                        "verification codes require PostgreSQL or MySQL",
                    ));
                }
            };
            let result = connection
                .execute_raw(Statement::from_sql_and_values(backend, sql, values))
                .await
                .map_err(Error::internal)?;
            Ok(code_matches && result.rows_affected() > 0)
        })
    }
}

#[derive(Debug)]
struct VerificationStoreError;

impl fmt::Display for VerificationStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("verification code operation did not find its state row")
    }
}

impl std::error::Error for VerificationStoreError {}

fn unsupported_verification_backend() -> Error {
    Error::custom(
        "unsupported_verification_backend",
        "verification codes require PostgreSQL or MySQL",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::core::Error;

    use super::{
        VerificationChallenge, VerificationCodeFuture, VerificationCodePolicy,
        VerificationCodeStore, VerificationCodes,
    };
    use crate::auth::EncodedPasswordHash;

    type FakeRow = (String, String, u8, u32, u32, bool);
    type FakeRows = Arc<Mutex<std::collections::HashMap<String, FakeRow>>>;

    #[derive(Default)]
    struct FakeStore {
        rows: FakeRows,
    }

    impl VerificationCodeStore for FakeStore {
        fn issue<'a>(
            &'a self,
            key_hash: &'a str,
            challenge_id: &'a str,
            code_hash: &'a EncodedPasswordHash,
            policy: VerificationCodePolicy,
        ) -> VerificationCodeFuture<'a, crate::Result<()>> {
            Box::pin(async move {
                let mut rows = self.rows.lock().map_err(|_| {
                    Error::internal(std::io::Error::other("verification fake store poisoned"))
                })?;
                rows.insert(
                    key_hash.to_owned(),
                    (
                        challenge_id.to_owned(),
                        code_hash.as_str().to_owned(),
                        policy.digits,
                        0,
                        policy.max_attempts,
                        false,
                    ),
                );
                Ok(())
            })
        }

        fn load_active<'a>(
            &'a self,
            key_hash: &'a str,
        ) -> VerificationCodeFuture<'a, crate::Result<Option<VerificationChallenge>>> {
            Box::pin(async move {
                let rows = self.rows.lock().map_err(|_| {
                    Error::internal(std::io::Error::other("verification fake store poisoned"))
                })?;
                let Some((id, hash, digits, attempts, max_attempts, consumed)) = rows.get(key_hash)
                else {
                    return Ok(None);
                };
                if *attempts >= *max_attempts || *consumed {
                    return Ok(None);
                }
                Ok(Some(VerificationChallenge::new(
                    id.clone(),
                    EncodedPasswordHash::from_encoded(hash.clone()),
                    *digits,
                )?))
            })
        }

        fn finish_attempt<'a>(
            &'a self,
            key_hash: &'a str,
            challenge_id: &'a str,
            code_matches: bool,
        ) -> VerificationCodeFuture<'a, crate::Result<bool>> {
            Box::pin(async move {
                let mut rows = self.rows.lock().map_err(|_| {
                    Error::internal(std::io::Error::other("verification fake store poisoned"))
                })?;
                let Some((id, _, _, attempts, max_attempts, consumed)) = rows.get_mut(key_hash)
                else {
                    return Ok(false);
                };
                if id != challenge_id || *consumed || *attempts >= *max_attempts {
                    return Ok(false);
                }
                if code_matches {
                    *consumed = true;
                    Ok(true)
                } else {
                    *attempts += 1;
                    Ok(false)
                }
            })
        }
    }

    fn fast_passwords() -> crate::Result<std::sync::Arc<dyn crate::hashing::Hasher>> {
        Ok(std::sync::Arc::new(crate::hashing::HashManager::new(
            crate::hashing::HashingConfig {
                driver: "argon2id".into(),
                argon: crate::hashing::ArgonConfig {
                    memory: 19_456,
                    time: 1,
                    threads: 1,
                    verify: true,
                },
                ..Default::default()
            },
        )?) as std::sync::Arc<dyn crate::hashing::Hasher>)
    }

    #[tokio::test]
    async fn binds_codes_to_purpose_and_identity_and_consumes_once() -> crate::Result<()> {
        let service = VerificationCodes::new(FakeStore::default(), fast_passwords()?);
        let policy = VerificationCodePolicy::default();
        let issued = service
            .issue("email.verify", "person@example.test", policy)
            .await?;
        assert_eq!(issued.code().len(), 6);
        assert!(
            !service
                .verify("password.reset", "person@example.test", issued.code())
                .await?
        );
        assert!(
            !service
                .verify("email.verify", "other@example.test", issued.code())
                .await?
        );
        assert!(
            service
                .verify("email.verify", "person@example.test", issued.code())
                .await?
        );
        assert!(
            !service
                .verify("email.verify", "person@example.test", issued.code())
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn invalid_codes_are_bounded_and_issue_redacts_the_code() -> crate::Result<()> {
        let service = VerificationCodes::new(FakeStore::default(), fast_passwords()?);
        let policy = VerificationCodePolicy::new(
            6,
            std::time::Duration::from_secs(60),
            std::time::Duration::from_secs(30),
            2,
        )?;
        let issued = service
            .issue("phone.verify", "+15551234567", policy)
            .await?;
        assert!(
            !service
                .verify("phone.verify", "+15551234567", "000000")
                .await?
        );
        assert!(
            !service
                .verify("phone.verify", "+15551234567", "000000")
                .await?
        );
        assert!(
            !service
                .verify("phone.verify", "+15551234567", issued.code())
                .await?
        );
        assert_eq!(
            format!("{issued:?}"),
            "IssuedVerificationCode { code: \"[REDACTED]\" }"
        );
        Ok(())
    }

    #[test]
    fn rejects_unbounded_code_policies() {
        assert!(
            VerificationCodePolicy::new(
                5,
                std::time::Duration::from_secs(60),
                std::time::Duration::from_secs(30),
                2,
            )
            .is_err()
        );
        assert!(
            VerificationCodePolicy::new(
                6,
                std::time::Duration::from_secs(0),
                std::time::Duration::from_secs(30),
                2,
            )
            .is_err()
        );
    }
}
