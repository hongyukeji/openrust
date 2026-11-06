//! 直接凭据登录的短时第一因素挑战；验证器异步调用始终位于用户事务之外。

use super::CredentialAuthority;
use crate::auth::fortify::{
    FactorAuthoritySnapshot, TwoFactorActions, TwoFactorCodeVerification, TwoFactorState,
};
use crate::{
    Error, Result,
    auth::{
        FactorVerificationMethod, TwoFactorLoginChallenge,
        credential::{CredentialFactorContext, FactorEvidence, FactorOrigin},
    },
    db::{
        Db,
        sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, QueryResult},
    },
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SubsecRound, Utc};
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const PREFIX: &str = "openrust_factor_";
const MAX_ATTEMPTS: i64 = 5;
const LIFETIME_SECONDS: i64 = 300;

pub(crate) enum CredentialLoginDecision {
    Ready(Box<CredentialFactorContext>),
    Challenge(TwoFactorLoginChallenge),
}
impl fmt::Debug for CredentialLoginDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialLoginDecision")
            .finish_non_exhaustive()
    }
}

/// 仅真实prepare可建立的短时快照；不持有数据库事务或用户锁。
pub(crate) struct PreparedCredentialChallenge {
    db: Db,
    user_id: i64,
    token_hash: String,
    source_hash: String,
    context: CredentialFactorContext,
    purpose: String,
    password_hash_digest: Option<String>,
    snapshot: FactorAuthoritySnapshot,
    expires_at: DateTime<Utc>,
    attempt: i64,
    owner: Arc<()>,
}
/// 事务外验证的受控输入；只有finish能原子消费当前因子与挑战。
pub(crate) struct CredentialChallengeVerification {
    owner: Arc<()>,
    verified_at: u64,
    candidate: VerificationCandidate,
}
enum VerificationCandidate {
    ModernRecovery(TwoFactorState),
    ModernTotp {
        code: Zeroizing<String>,
        verification: TwoFactorCodeVerification,
    },
}
/// 最后一次事务校验只保留挑战摘要、应有状态和本次因子验证事实。
pub(crate) struct ChallengeCommitCheck {
    user_id: i64,
    token_hash: String,
    source_hash: String,
    expected: ChallengeRecord,
    state: TwoFactorState,
    totp: Option<(Zeroizing<String>, TwoFactorCodeVerification)>,
}
impl ChallengeCommitCheck {
    pub(crate) async fn validate(
        &self,
        authority: &CredentialAuthority,
        tx: &DatabaseTransaction,
    ) -> Result<()> {
        authority
            .lock_active_user_in_transaction(tx, self.user_id)
            .await?;
        let row = challenge_row(&authority.db, tx, &self.token_hash)
            .await?
            .ok_or(Error::Unauthorized)?;
        let current = ChallengeRecord::decode(&row, self.user_id, &self.source_hash)?;
        if current != self.expected {
            return Err(Error::Unauthorized);
        }
        require_password_version(
            &authority.db,
            tx,
            self.user_id,
            self.expected.password_hash_digest.as_deref(),
        )
        .await?;
        let snapshot = authority
            .account(self.user_id)?
            .load_in_transaction(&authority.db, tx)
            .await?;
        if snapshot.source != current.context.source
            || snapshot.generation != current.context.generation
            || snapshot.state != self.state
            || !snapshot.challenge_required(authority.confirmation_required()?)
        {
            return Err(Error::Unauthorized);
        }
        if let Some((code, _)) = &self.totp {
            let row = authority
                .db
                .table("users")?
                .using_transaction(tx)
                .select(&["two_factor_consumed_codes"])?
                .where_eq("id", self.user_id)?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            let encoded: Option<String> = row
                .try_get("", "two_factor_consumed_codes")
                .map_err(Error::internal)?;
            let ledger: std::collections::BTreeMap<String, u64> =
                serde_json::from_str(encoded.as_deref().ok_or(Error::Unauthorized)?)
                    .map_err(|_| Error::Unauthorized)?;
            let now = unix_time()?;
            if ledger
                .get(&digest(code))
                .is_none_or(|expiry| *expiry <= now)
            {
                return Err(Error::Unauthorized);
            }
        }
        self.require_current()
    }

    pub(crate) fn require_current(&self) -> Result<()> {
        require_fresh(self.expected.expires_at)?;
        if let Some((code, verification)) = &self.totp {
            verification.require_code(code)?;
        }
        Ok(())
    }

    #[cfg(feature = "auth-oidc")]
    pub(crate) fn purpose(&self) -> &str {
        &self.expected.purpose
    }
}

pub(crate) struct CompletedCredentialChallenge {
    pub(crate) check: ChallengeCommitCheck,
    pub(crate) context: CredentialFactorContext,
    pub(crate) expires_at: DateTime<Utc>,
}
macro_rules! redacted_debug {
    ($($kind:ty),+ $(,)?) => {$ (impl fmt::Debug for $kind {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct(stringify!($kind)).finish_non_exhaustive() }
    })+};
}
redacted_debug!(
    PreparedCredentialChallenge,
    CredentialChallengeVerification,
    CompletedCredentialChallenge
);

impl PreparedCredentialChallenge {
    pub(crate) fn user_id(&self) -> i64 {
        self.user_id
    }
    pub(crate) fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub(crate) fn require_scope(
        &self,
        scope: Option<&crate::auth::credential::CredentialGuardBinding>,
    ) -> Result<()> {
        if self.context.scope.as_ref() != scope {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    pub(crate) async fn verify(
        &self,
        actions: &TwoFactorActions,
        code: Option<&str>,
        recovery: Option<&str>,
    ) -> Result<CredentialChallengeVerification> {
        require_fresh(self.expires_at)?;
        let code = truthy(code);
        let recovery = truthy(recovery);
        let encrypter = actions.default_encrypter();
        if let (Some(input), Some(ciphertext)) =
            (recovery, self.snapshot.state.recovery_codes.as_ref())
        {
            let json = crate::auth::fortify::actions::decrypt_string(&encrypter, ciphertext)?;
            let codes: Zeroizing<Vec<String>> =
                Zeroizing::new(serde_json::from_str(&json).map_err(Error::internal)?);
            if let Some(found) = codes
                .iter()
                .find(|stored| bool::from(stored.as_bytes().ct_eq(input.as_bytes())))
            {
                let generated = Zeroizing::new((actions.recovery_generator)()?);
                let replaced = Zeroizing::new(json.replace(found, &generated));
                let mut state = self.snapshot.state.clone();
                state.recovery_codes = Some(encrypter.encrypt(replaced.as_bytes())?);
                require_fresh(self.expires_at)?;
                return Ok(CredentialChallengeVerification {
                    owner: self.owner.clone(),
                    verified_at: unix_time()?,
                    candidate: VerificationCandidate::ModernRecovery(state),
                });
            }
        }
        let (code, ciphertext) = code
            .zip(self.snapshot.state.secret.as_ref())
            .ok_or(Error::Unauthorized)?;
        let secret = crate::auth::fortify::actions::decrypt_string(&encrypter, ciphertext)?;
        let verification = actions.provider.verify_with_context(&secret, code).await?;
        require_fresh(self.expires_at)?;
        if !verification.is_valid() {
            return Err(Error::Unauthorized);
        }
        Ok(CredentialChallengeVerification {
            owner: self.owner.clone(),
            verified_at: unix_time()?,
            candidate: VerificationCandidate::ModernTotp {
                code: Zeroizing::new(code.to_owned()),
                verification,
            },
        })
    }
}

impl CredentialAuthority {
    pub(crate) async fn require_password_for_login(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        password_hash_digest: &str,
    ) -> Result<()> {
        self.lock_active_user_in_transaction(transaction, user_id)
            .await?;
        require_password_version(&self.db, transaction, user_id, Some(password_hash_digest)).await
    }

    pub(crate) async fn capture_new_challenge(
        &self,
        tx: &DatabaseTransaction,
        challenge: &TwoFactorLoginChallenge,
        source: &str,
        purpose: &str,
        scope: Option<&crate::auth::credential::CredentialGuardBinding>,
    ) -> Result<ChallengeCommitCheck> {
        let user_id = challenge.user_id();
        let token_hash = digest(challenge.expose());
        let source_hash = digest(source);
        let row = challenge_row(&self.db, tx, &token_hash)
            .await?
            .ok_or(Error::Unauthorized)?;
        let expected = ChallengeRecord::decode(&row, user_id, &source_hash)?;
        if expected.consumed
            || expected.attempts != 0
            || expected.purpose != purpose
            || expected.context.scope.as_ref() != scope
            || expected.expires_at != challenge.expires_at()
        {
            return Err(Error::Unauthorized);
        }
        let snapshot = self
            .account(user_id)?
            .load_in_transaction(&self.db, tx)
            .await?;
        Ok(ChallengeCommitCheck {
            user_id,
            token_hash,
            source_hash,
            expected,
            state: snapshot.state,
            totp: None,
        })
    }

    #[cfg(any(test, feature = "auth-oidc"))]
    pub(crate) async fn begin_login_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        source: &str,
        purpose: &str,
    ) -> Result<CredentialLoginDecision> {
        self.begin_login_in_scope_in_transaction(transaction, user_id, source, purpose, None)
            .await
    }

    pub(crate) async fn begin_login_in_scope_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        source: &str,
        purpose: &str,
        scope: Option<crate::auth::credential::CredentialGuardBinding>,
    ) -> Result<CredentialLoginDecision> {
        validate_source(source)?;
        validate_purpose(purpose)?;
        self.lock_active_user_in_transaction(transaction, user_id)
            .await?;
        let snapshot = self
            .account(user_id)?
            .load_in_transaction(&self.db, transaction)
            .await?;
        let mut context = context(user_id, &snapshot, None);
        context.scope = scope;
        context.validate_for_user(user_id)?;
        if !snapshot.challenge_required(self.confirmation_required()?) {
            return Ok(CredentialLoginDecision::Ready(Box::new(context)));
        }
        let password_hash_digest = if password_purpose(purpose) {
            Some(password_digest(&self.db, transaction, user_id).await?)
        } else {
            None
        };
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(Error::internal)?;
        let raw = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(random));
        let created_at = Utc::now().trunc_subsecs(6);
        let expires_at = created_at
            .checked_add_signed(chrono::Duration::seconds(LIFETIME_SECONDS))
            .ok_or(Error::Unauthorized)?;
        let backend = transaction.get_database_backend();
        let changed = self
            .db
            .table("auth_factor_challenges")?
            .using_transaction(transaction)
            .insert(vec![
                ("token_hash", digest(&raw).into()),
                ("user_id", user_id.into()),
                ("source_hash", digest(source).into()),
                ("context", context.encode(user_id)?.into()),
                ("purpose", purpose.to_owned().into()),
                ("password_hash_digest", password_hash_digest.into()),
                ("attempts", 0_i64.into()),
                (
                    "created_at",
                    crate::db::dialect::utc_value(backend, created_at),
                ),
                (
                    "expires_at",
                    crate::db::dialect::utc_value(backend, expires_at),
                ),
            ])
            .await?;
        require_fresh(expires_at)?;
        if changed != 1 {
            return Err(Error::Unauthorized);
        }
        Ok(CredentialLoginDecision::Challenge(
            TwoFactorLoginChallenge::new(raw, user_id, expires_at),
        ))
    }

    #[cfg(test)]
    pub(crate) async fn prepare_challenge(
        &self,
        raw: &str,
        source: &str,
    ) -> Result<PreparedCredentialChallenge> {
        self.prepare_challenge_in_scope(raw, source, None).await
    }

    pub(crate) async fn prepare_challenge_in_scope(
        &self,
        raw: &str,
        source: &str,
        scope: Option<&crate::auth::credential::CredentialGuardBinding>,
    ) -> Result<PreparedCredentialChallenge> {
        let scope = scope.cloned();
        validate_raw(raw)?;
        validate_source(source)?;
        let token_hash = digest(raw);
        let source_hash = digest(source);
        let hint = self
            .db
            .table("auth_factor_challenges")?
            .select(&["user_id"])?
            .where_eq("token_hash", token_hash.clone())?;
        let user_id: i64 = self
            .db
            .with_primary(hint.first())
            .await??
            .ok_or(Error::Unauthorized)?
            .try_get("", "user_id")
            .map_err(|_| Error::Unauthorized)?;
        let policy = self.clone();
        let prepared = self
            .db
            .transaction_for_update(move |transaction| {
                Box::pin(async move {
                    policy
                        .lock_active_user_in_transaction(transaction, user_id)
                        .await?;
                    let row = challenge_row(&policy.db, transaction, &token_hash)
                        .await?
                        .ok_or(Error::Unauthorized)?;
                    let record = ChallengeRecord::decode(&row, user_id, &source_hash)?;
                    if record.context.scope != scope {
                        return Err(Error::Unauthorized);
                    }
                    require_password_version(
                        &policy.db,
                        transaction,
                        user_id,
                        record.password_hash_digest.as_deref(),
                    )
                    .await?;
                    require_fresh(record.expires_at)?;
                    if record.consumed || record.attempts >= MAX_ATTEMPTS {
                        return Err(Error::Unauthorized);
                    }
                    let snapshot = policy
                        .account(user_id)?
                        .load_in_transaction(&policy.db, transaction)
                        .await?;
                    require_fresh(record.expires_at)?;
                    if record.context.source != snapshot.source
                        || record.context.generation != snapshot.generation
                        || !snapshot.challenge_required(policy.confirmation_required()?)
                    {
                        return Err(Error::Unauthorized);
                    }
                    let attempt = record.attempts + 1;
                    let changed = policy
                        .db
                        .table("auth_factor_challenges")?
                        .using_transaction(transaction)
                        .where_eq("token_hash", token_hash.clone())?
                        .where_eq("user_id", user_id)?
                        .where_eq("attempts", record.attempts)?
                        .where_null("consumed_at")?
                        .update(vec![("attempts", attempt.into())])
                        .await?;
                    require_fresh(record.expires_at)?;
                    if changed != 1 {
                        return Err(Error::Unauthorized);
                    }
                    Ok(PreparedCredentialChallenge {
                        db: policy.db.clone(),
                        user_id,
                        token_hash,
                        source_hash,
                        context: record.context,
                        purpose: record.purpose,
                        password_hash_digest: record.password_hash_digest,
                        snapshot,
                        expires_at: record.expires_at,
                        attempt,
                        owner: Arc::new(()),
                    })
                })
            })
            .await?;
        require_fresh(prepared.expires_at)?;
        Ok(prepared)
    }

    // 因子消费、挑战单次消费及每个await后的期限检查须在同一外层事务中顺序可审查。
    #[allow(clippy::too_many_lines)]
    pub(crate) async fn finish_challenge_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        prepared: PreparedCredentialChallenge,
        verification: CredentialChallengeVerification,
    ) -> Result<Option<CompletedCredentialChallenge>> {
        self.require_store(&prepared.db)?;
        if !Arc::ptr_eq(&prepared.owner, &verification.owner) {
            return Err(Error::Unauthorized);
        }
        require_fresh(prepared.expires_at)?;
        self.lock_active_user_in_transaction(transaction, prepared.user_id)
            .await?;
        require_fresh(prepared.expires_at)?;
        let Some(row) = challenge_row(&self.db, transaction, &prepared.token_hash).await? else {
            return Ok(None);
        };
        require_fresh(prepared.expires_at)?;
        let mut record = ChallengeRecord::decode(&row, prepared.user_id, &prepared.source_hash)?;
        if record.consumed {
            return Ok(None);
        }
        if record.attempts < prepared.attempt
            || record.attempts > MAX_ATTEMPTS
            || record.context != prepared.context
            || record.purpose != prepared.purpose
            || record.password_hash_digest != prepared.password_hash_digest
            || record.expires_at != prepared.expires_at
        {
            return Err(Error::Unauthorized);
        }
        require_password_version(
            &self.db,
            transaction,
            prepared.user_id,
            record.password_hash_digest.as_deref(),
        )
        .await?;
        require_fresh(prepared.expires_at)?;
        let authority = self.account(prepared.user_id)?;
        let current = authority.load_in_transaction(&self.db, transaction).await?;
        require_fresh(prepared.expires_at)?;
        // 锁外验证后的因子快照已被消费时，登录失败按认证拒绝返回；管理冲突仍保留409。
        if current != prepared.snapshot
            || current.source != prepared.context.source
            || current.generation != prepared.context.generation
            || !current.challenge_required(self.confirmation_required()?)
        {
            return Err(Error::Unauthorized);
        }
        let (snapshot, method, verified_at, totp) = match verification.candidate {
            VerificationCandidate::ModernRecovery(replacement) => {
                let snapshot = authority
                    .consume_modern_in_transaction(
                        &self.db,
                        transaction,
                        &prepared.snapshot,
                        Some(&replacement),
                        None,
                    )
                    .await?;
                (
                    snapshot,
                    FactorVerificationMethod::RecoveryCode,
                    verification.verified_at,
                    None,
                )
            }
            VerificationCandidate::ModernTotp {
                code,
                verification: fact,
            } => {
                let snapshot = authority
                    .with_provider_verification(&fact)?
                    .consume_modern_in_transaction(
                        &self.db,
                        transaction,
                        &prepared.snapshot,
                        None,
                        Some(&code),
                    )
                    .await?;
                (
                    snapshot,
                    FactorVerificationMethod::Totp,
                    verification.verified_at,
                    Some((code, fact)),
                )
            }
        };
        require_fresh(prepared.expires_at)?;
        let factor = FactorEvidence {
            method,
            verified_at,
            origin: match &prepared.context.scope {
                Some(scope) => FactorOrigin::Guard {
                    guard: scope.guard.clone(),
                    provider: Some(scope.provider.clone()),
                },
                None => FactorOrigin::DatabaseFlow {
                    source: snapshot.source.clone(),
                    purpose: prepared.purpose,
                },
            },
        };
        let mut context = context(prepared.user_id, &snapshot, Some(factor));
        context.scope = prepared.context.scope;
        context.validate_for_user(prepared.user_id)?;
        let consumed_at = Utc::now();
        let changed = self
            .db
            .table("auth_factor_challenges")?
            .using_transaction(transaction)
            .where_eq("token_hash", prepared.token_hash.clone())?
            .where_eq("user_id", prepared.user_id)?
            .where_null("consumed_at")?
            .update(vec![(
                "consumed_at",
                crate::db::dialect::utc_value(transaction.get_database_backend(), consumed_at),
            )])
            .await?;
        require_fresh(prepared.expires_at)?;
        if changed != 1 {
            return Err(Error::Unauthorized);
        }
        record.consumed = true;
        Ok(Some(CompletedCredentialChallenge {
            check: ChallengeCommitCheck {
                user_id: prepared.user_id,
                token_hash: prepared.token_hash,
                source_hash: prepared.source_hash,
                expected: record,
                state: snapshot.state,
                totp,
            },
            context,
            expires_at: prepared.expires_at,
        }))
    }
}

#[derive(PartialEq, Eq)]
struct ChallengeRecord {
    context: CredentialFactorContext,
    purpose: String,
    password_hash_digest: Option<String>,
    attempts: i64,
    expires_at: DateTime<Utc>,
    consumed: bool,
}
impl ChallengeRecord {
    fn decode(row: &QueryResult, user_id: i64, source_hash: &str) -> Result<Self> {
        let owner: i64 = row
            .try_get("", "user_id")
            .map_err(|_| Error::Unauthorized)?;
        let stored_source: String = row
            .try_get("", "source_hash")
            .map_err(|_| Error::Unauthorized)?;
        if owner != user_id
            || owner <= 0
            || !bool::from(stored_source.as_bytes().ct_eq(source_hash.as_bytes()))
        {
            return Err(Error::Unauthorized);
        }
        let encoded: String = row
            .try_get("", "context")
            .map_err(|_| Error::Unauthorized)?;
        let context = CredentialFactorContext::decode(&encoded, user_id)?;
        if context.factor.is_some() {
            return Err(Error::Unauthorized);
        }
        let purpose: String = row
            .try_get("", "purpose")
            .map_err(|_| Error::Unauthorized)?;
        validate_purpose(&purpose)?;
        let password_hash_digest: Option<String> = row
            .try_get("", "password_hash_digest")
            .map_err(|_| Error::Unauthorized)?;
        if password_purpose(&purpose) != password_hash_digest.is_some()
            || password_hash_digest.as_ref().is_some_and(|digest| {
                digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            return Err(Error::Unauthorized);
        }
        let attempts: i64 = row
            .try_get("", "attempts")
            .map_err(|_| Error::Unauthorized)?;
        let expires_at: DateTime<Utc> = row
            .try_get("", "expires_at")
            .map_err(|_| Error::Unauthorized)?;
        let created_at: DateTime<Utc> = row
            .try_get("", "created_at")
            .map_err(|_| Error::Unauthorized)?;
        let consumed: Option<DateTime<Utc>> = row
            .try_get("", "consumed_at")
            .map_err(|_| Error::Unauthorized)?;
        if !(0..=MAX_ATTEMPTS).contains(&attempts)
            || created_at > Utc::now()
            || expires_at <= created_at
            || expires_at.signed_duration_since(created_at)
                > chrono::Duration::seconds(LIFETIME_SECONDS)
        {
            return Err(Error::Unauthorized);
        }
        require_fresh(expires_at)?;
        Ok(Self {
            context,
            purpose,
            password_hash_digest,
            attempts,
            expires_at,
            consumed: consumed.is_some(),
        })
    }
}
async fn challenge_row(
    db: &Db,
    transaction: &DatabaseTransaction,
    hash: &str,
) -> Result<Option<QueryResult>> {
    let query = db
        .table("auth_factor_challenges")?
        .using_transaction(transaction)
        .where_eq("token_hash", hash.to_owned())?;
    let query = if transaction.get_database_backend() == DbBackend::Sqlite {
        query
    } else {
        query.lock_for_update()?
    };
    query.first().await
}
fn password_purpose(purpose: &str) -> bool {
    matches!(purpose, "password" | "jwt-api-password")
}
async fn password_digest(
    db: &Db,
    transaction: &DatabaseTransaction,
    user_id: i64,
) -> Result<String> {
    let row = db
        .table("users")?
        .using_transaction(transaction)
        .select(&["password_hash"])?
        .where_eq("id", user_id)?
        .first()
        .await?
        .ok_or(Error::Unauthorized)?;
    let hash: Option<String> = row
        .try_get("", "password_hash")
        .map_err(|_| Error::Unauthorized)?;
    Ok(digest(hash.as_deref().ok_or(Error::Unauthorized)?))
}
async fn require_password_version(
    db: &Db,
    transaction: &DatabaseTransaction,
    user_id: i64,
    expected: Option<&str>,
) -> Result<()> {
    if let Some(expected) = expected {
        let current = password_digest(db, transaction, user_id).await?;
        if !bool::from(current.as_bytes().ct_eq(expected.as_bytes())) {
            return Err(Error::Unauthorized);
        }
    }
    Ok(())
}
fn require_fresh(expires_at: DateTime<Utc>) -> Result<()> {
    if expires_at <= Utc::now() {
        Err(Error::Unauthorized)
    } else {
        Ok(())
    }
}
fn validate_source(source: &str) -> Result<()> {
    if source.is_empty() || source.len() > 4_096 {
        Err(Error::Unauthorized)
    } else {
        Ok(())
    }
}
fn validate_purpose(purpose: &str) -> Result<()> {
    if purpose.is_empty() || purpose.len() > 1_024 {
        Err(Error::Unauthorized)
    } else {
        Ok(())
    }
}
fn validate_raw(raw: &str) -> Result<()> {
    let encoded = raw.strip_prefix(PREFIX).ok_or(Error::Unauthorized)?;
    if encoded.len() != 43 {
        return Err(Error::Unauthorized);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Error::Unauthorized)?;
    if bytes.len() != 32 || URL_SAFE_NO_PAD.encode(bytes) != encoded {
        return Err(Error::Unauthorized);
    }
    Ok(())
}
fn truthy(input: Option<&str>) -> Option<&str> {
    input.filter(|value| !value.is_empty() && *value != "0")
}
fn context(
    user_id: i64,
    snapshot: &FactorAuthoritySnapshot,
    factor: Option<FactorEvidence>,
) -> CredentialFactorContext {
    CredentialFactorContext {
        scope: None,
        version: 1,
        identifier: user_id.into(),
        source: snapshot.source.clone(),
        generation: snapshot.generation.clone(),
        factor,
    }
}
fn unix_time() -> Result<u64> {
    u64::try_from(Utc::now().timestamp()).map_err(Error::internal)
}
fn digest(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "challenge_tests.rs"]
mod tests;
