//! 真实凭据的受控身份与可派生因子事实；不将凭据种类伪装为Guard。

use crate::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const SESSION_FACTOR_CONTEXT_KEY: &str = "openrust.auth.factor_context";

#[cfg(feature = "auth-fortify")]
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredSessionFactorContext {
    pub(crate) context: CredentialFactorContext,
    pub(crate) binding: String,
}

/// 完成因子验证的实际来源；目标token可保留原来源而使用自己的绑定。
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum FactorOrigin {
    /// 同一请求内真实的具名 `Guard` 和 `Provider`。
    Guard {
        /// 实际Guard名称。
        guard: String,
        /// 实际Provider名称。
        provider: Option<String>,
    },
    /// 既有整数users域的显式应用登录流程。
    DatabaseFlow {
        /// 真实authority来源，不是虚构的Guard名称。
        source: String,
        /// 已验证第一因素对应的流程目的。
        purpose: String,
    },
}
impl std::fmt::Debug for FactorOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactorOrigin").finish_non_exhaustive()
    }
}

/// 当前已经验证的实际凭据；不包含原始token或Cookie值。
#[derive(Clone, PartialEq, Eq)]
pub enum CredentialDomain {
    /// 实际持久Session及其具名身份域。
    Session {
        /// 实际Guard名称。
        guard: String,
        /// 实际Provider名称。
        provider: Option<String>,
        /// `Guard` 核验后的 `Session` 绑定摘要。
        binding: String,
    },
    /// 已校验签名及持久家族的JWT。
    Jwt {
        /// 签名声明的issuer。
        issuer: String,
        /// 签名声明的audience。
        audience: String,
        /// 签名声明的唯一ID。
        jti: String,
        /// 实际活动的refresh家族。
        family_id: String,
    },
    /// 已验证数据库摘要的refresh凭据。
    Refresh {
        /// 实际家族标识。
        family_id: String,
        /// 当前原始token的SHA-256摘要。
        token_hash: String,
    },
    /// 已验证数据库摘要与owner的个人令牌。
    PersonalAccessToken {
        /// 实际数据库令牌行ID。
        id: i64,
        /// 当前原始token的SHA-256摘要。
        token_hash: String,
    },
}
impl std::fmt::Debug for CredentialDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialDomain").finish_non_exhaustive()
    }
}

// 自定义 Guard 无需构造框架内部租约。只从真实 Provider 及已保存服务器端身份取回管理上下文。
#[cfg(feature = "auth-fortify")]
pub(crate) async fn persisted_guard_management<G: super::StatefulGuard + ?Sized>(
    guard: &G,
    name: &str,
    session: &crate::session::Session,
) -> Result<super::FactorManagementContext> {
    use sha2::{Digest as _, Sha256};
    session.require_authenticated_storage()?;
    let session_id = session.id()?;
    let key = guard
        .session_name()
        .filter(|key| !key.is_empty())
        .ok_or(Error::Unauthorized)?;
    let proof_key = super::guard::session_factor_key(&key);
    let user = guard.user().await?.ok_or(Error::Unauthorized)?;
    let identifier = user.auth_identifier()?;
    let provider = guard.provider();
    let authority = user
        .as_two_factor()
        .and_then(super::fortify::TwoFactorAuthenticatable::two_factor_authority)
        .ok_or(Error::Unauthorized)?;
    authority.validate_account(&user, provider.as_ref()).await?;
    let current = provider
        .retrieve_by_id(&identifier)
        .await?
        .ok_or(Error::Unauthorized)?;
    authority
        .validate_account(&current, provider.as_ref())
        .await?;
    let actual = session.handler()?;
    let reader = actual
        .independent_authorization_reader()?
        .unwrap_or_else(|| actual.clone());
    if reader.guarantees() != actual.guarantees()
        || reader.guarantees() == crate::session::SessionStoreGuarantees::ClientSideSnapshot
    {
        return Err(Error::Unauthorized);
    }
    let maximum = session.credential_parent_max_bytes();
    let record = reader
        .load_authorized(&session_id, maximum)
        .await?
        .ok_or(Error::Unauthorized)?;
    record.validate_for(actual.semantics(), maximum)?;
    if record.id != session_id
        || crate::support::data::get(&record.data, &key).as_ref() != Some(&identifier)
    {
        return Err(Error::Unauthorized);
    }
    let proof = crate::support::data::get(&record.data, &proof_key);
    if guard
        .user()
        .await?
        .ok_or(Error::Unauthorized)?
        .auth_identifier()?
        != identifier
    {
        return Err(Error::Unauthorized);
    }
    session.credential_parent_state(
        &session_id,
        &actual,
        (&key, &identifier),
        (&proof_key, proof.as_ref()),
        None,
    )?;
    if record.expiry_date <= time::OffsetDateTime::now_utc() {
        return Err(Error::Unauthorized);
    }
    let binding = serde_json::to_vec(&(&session_id, name, guard.provider_name(), &identifier))
        .map_err(Error::internal)?;
    super::FactorManagementContext::issue(
        identifier,
        name.to_owned(),
        guard.provider_name().map(str::to_owned),
        format!("{:x}", Sha256::digest(binding)),
        proof
            .map(super::SessionFactorProof::from_value)
            .transpose()?,
    )
}

/// 已验证具名 Guard 的本请求身份和原始存储，只在短时派生票据内流转。
#[cfg(feature = "auth-fortify")]
pub(crate) struct CredentialSessionParent {
    session: crate::session::Session,
    id: crate::session::SessionId,
    handler: std::sync::Arc<dyn crate::session::SessionStore>,
    reader: std::sync::Arc<dyn crate::session::SessionStore>,
    session_key: String,
    proof_key: String,
    identifier: Value,
    proof: Option<Value>,
    guard_owner: Option<std::sync::Arc<()>>,
    fresh_at_prepare: bool,
    first_deadline: std::sync::OnceLock<time::OffsetDateTime>,
}

#[cfg(feature = "auth-fortify")]
impl CredentialSessionParent {
    pub(crate) fn capture(
        guard: &dyn super::StatefulGuard,
        context: &super::FactorManagementContext,
    ) -> Result<Self> {
        let (actual_name, session) = guard.credential_parent_session().ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_credential_parent_unavailable",
                "所选 Guard 未提供可复核的父 Session",
            )
        })?;
        let id = session.id()?;
        if actual_name != context.guard() || !context.matches_session_id(&id)? {
            return Err(Error::Unauthorized);
        }
        let session_key = guard
            .session_name()
            .filter(|key| !key.is_empty())
            .ok_or(Error::Unauthorized)?;
        let proof_key = super::guard::session_factor_key(&session_key);
        let owner = session.credential_guard_owner(&session_key)?;
        let allow_fresh = owner.is_some();
        Self::capture_keys(
            &session,
            session_key,
            context.identifier().clone(),
            proof_key,
            context
                .factor_proof()
                .map(serde_json::to_value)
                .transpose()
                .map_err(Error::internal)?,
            owner,
            allow_fresh,
        )
    }

    #[cfg(any(feature = "auth-oidc", all(test, feature = "db-sqlite")))]
    pub(crate) fn capture_session_context(
        session: &crate::session::Session,
        stored: &StoredSessionFactorContext,
    ) -> Result<Self> {
        Self::capture_keys(
            session,
            super::AUTH_USER_ID_KEY.into(),
            stored.context.identifier.clone(),
            SESSION_FACTOR_CONTEXT_KEY.into(),
            Some(serde_json::to_value(stored).map_err(Error::internal)?),
            None,
            true,
        )
    }

    fn capture_keys(
        session: &crate::session::Session,
        session_key: String,
        identifier: Value,
        proof_key: String,
        proof: Option<Value>,
        guard_owner: Option<std::sync::Arc<()>>,
        allow_fresh: bool,
    ) -> Result<Self> {
        let handler = session.handler()?;
        let reader = handler
            .independent_authorization_reader()?
            .unwrap_or_else(|| handler.clone());
        if reader.guarantees() != handler.guarantees()
            || reader.guarantees() == crate::session::SessionStoreGuarantees::ClientSideSnapshot
        {
            return Err(Error::Unauthorized);
        }
        session.require_authenticated_storage()?;
        let id = session.id()?;
        let (_, fresh_at_prepare) = session.credential_parent_state(
            &id,
            &handler,
            (&session_key, &identifier),
            (&proof_key, proof.as_ref()),
            guard_owner.as_ref(),
        )?;
        Ok(Self {
            id,
            handler,
            reader,
            proof_key,
            identifier,
            proof,
            guard_owner,
            fresh_at_prepare: fresh_at_prepare && allow_fresh,
            session_key,
            session: session.clone(),
            first_deadline: std::sync::OnceLock::new(),
        })
    }

    fn check_local(&self) -> Result<(time::OffsetDateTime, bool)> {
        self.session.require_authenticated_storage()?;
        self.session.credential_parent_state(
            &self.id,
            &self.handler,
            (&self.session_key, &self.identifier),
            (&self.proof_key, self.proof.as_ref()),
            self.guard_owner.as_ref(),
        )
    }

    pub(crate) async fn validate(
        &self,
        db: &crate::db::Db,
        tx: &crate::db::DatabaseTransaction,
    ) -> Result<DateTime<Utc>> {
        let reader = self.reader.credential_parent_reader(db)?;
        let (mut cutoff, fresh) = self.check_local()?;
        if !(self.fresh_at_prepare && fresh) {
            let maximum = self.session.credential_parent_max_bytes();
            let record = reader
                .load_authorized_for_credential(tx, &self.id, maximum)
                .await?
                .ok_or(Error::Unauthorized)?;
            record.validate_for(self.handler.semantics(), maximum)?;
            if record.id != self.id
                || crate::support::data::get(&record.data, &self.session_key).as_ref()
                    != Some(&self.identifier)
                || crate::support::data::get(&record.data, &self.proof_key) != self.proof
            {
                return Err(Error::Unauthorized);
            }
            cutoff = cutoff.min(record.expiry_date).min(self.check_local()?.0);
        }
        cutoff = cutoff.min(*self.first_deadline.get_or_init(|| cutoff));
        if cutoff <= time::OffsetDateTime::now_utc() {
            return Err(Error::Unauthorized);
        }
        DateTime::from_timestamp(cutoff.unix_timestamp(), cutoff.nanosecond())
            .ok_or(Error::Unauthorized)
    }
}

/// 实际验证过的凭据身份；只能由框架认证链建立，不能反序列化或从裸ID构造。
pub struct VerifiedCredential {
    #[cfg(feature = "auth-fortify")]
    pub(crate) session_parent: Option<Box<CredentialSessionParent>>,
    #[cfg(feature = "auth-fortify")]
    pub(crate) store: crate::db::Db,
    pub(crate) user_id: i64,
    pub(crate) domain: CredentialDomain,
    pub(crate) context: Option<CredentialFactorContext>,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) abilities: Option<Vec<String>>,
    #[cfg(feature = "auth-fortify")]
    pub(crate) prepared_at: DateTime<Utc>,
}
impl VerifiedCredential {
    /// 当前显式数据库账户域中的正整数ID。
    pub fn user_id(&self) -> i64 {
        self.user_id
    }
    /// 实际使用的凭据种类和绑定。
    pub fn domain(&self) -> &CredentialDomain {
        &self.domain
    }
    /// 凭据明确声明的期限；没有固定期限或无法投影时为None。
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }
    /// 实际因子验证来源；没有证明时为None。
    pub fn factor_origin(&self) -> Option<&FactorOrigin> {
        self.context
            .as_ref()?
            .factor
            .as_ref()
            .map(|factor| &factor.origin)
    }
    /// 实际因子验证时间；派生或轮换不会把它改成当前时间。
    pub fn factor_verified_at(&self) -> Option<u64> {
        self.context
            .as_ref()?
            .factor
            .as_ref()
            .map(|factor| factor.verified_at)
    }
    /// 实际PAT权限上限；派生不能扩展此集合。
    pub fn token_abilities(&self) -> Option<&[String]> {
        self.abilities.as_deref()
    }
}
impl std::fmt::Debug for VerifiedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedCredential").finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FactorEvidence {
    pub(crate) method: super::FactorVerificationMethod,
    pub(crate) verified_at: u64,
    pub(crate) origin: FactorOrigin,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialGuardBinding {
    pub(crate) issuer: String,
    pub(crate) audience: String,
    pub(crate) guard: String,
    pub(crate) provider: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialFactorContext {
    pub(crate) scope: Option<CredentialGuardBinding>,
    pub(crate) version: u8,
    pub(crate) identifier: Value,
    pub(crate) source: String,
    pub(crate) generation: String,
    pub(crate) factor: Option<FactorEvidence>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JwtCredentialGrant {
    pub(crate) family_id: String,
    pub(crate) context: CredentialFactorContext,
}
impl JwtCredentialGrant {
    pub(crate) fn validate_for_user(&self, user_id: i64) -> Result<()> {
        if self.family_id.len() != 32
            || !self
                .family_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::Unauthorized);
        }
        self.context.validate_for_user(user_id)
    }
}
impl CredentialFactorContext {
    pub(crate) fn validate_for_user(&self, user_id: i64) -> Result<()> {
        if user_id <= 0
            || self.version != 1
            || self.identifier.as_i64() != Some(user_id)
            || self.source.is_empty()
            || self.source.len() > 2_048
            || self.generation.is_empty()
            || self.generation.len() > 2_048
        {
            return Err(Error::Unauthorized);
        }
        if self.scope.as_ref().is_some_and(|scope| {
            scope.issuer.is_empty()
                || scope.audience.is_empty()
                || scope.guard.is_empty()
                || scope.guard.len() > 1_024
                || scope.provider.is_empty()
                || scope.provider.len() > 1_024
        }) {
            return Err(Error::Unauthorized);
        }
        if let Some(factor) = &self.factor {
            let now = u64::try_from(Utc::now().timestamp()).map_err(|_| Error::Unauthorized)?;
            if factor.verified_at > now.saturating_add(30) {
                return Err(Error::Unauthorized);
            }
            if let Some(scope) = &self.scope
                && factor.origin
                    != (FactorOrigin::Guard {
                        guard: scope.guard.clone(),
                        provider: Some(scope.provider.clone()),
                    })
            {
                return Err(Error::Unauthorized);
            }
            match &factor.origin {
                FactorOrigin::Guard { guard, provider }
                    if guard.is_empty()
                        || guard.len() > 1_024
                        || provider
                            .as_ref()
                            .is_some_and(|value| value.is_empty() || value.len() > 1_024) =>
                {
                    return Err(Error::Unauthorized);
                }
                FactorOrigin::DatabaseFlow { source, purpose }
                    if source != &self.source || purpose.is_empty() || purpose.len() > 1_024 =>
                {
                    return Err(Error::Unauthorized);
                }
                _ => {}
            }
        }
        Ok(())
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn encode(&self, user_id: i64) -> Result<String> {
        self.validate_for_user(user_id)?;
        let value = serde_json::to_string(self).map_err(Error::internal)?;
        if value.len() > 8_192 {
            return Err(Error::Unauthorized);
        }
        Ok(value)
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn decode(value: &str, user_id: i64) -> Result<Self> {
        if value.len() > 8_192 {
            return Err(Error::Unauthorized);
        }
        let context: Self = serde_json::from_str(value).map_err(|_| Error::Unauthorized)?;
        context.validate_for_user(user_id)?;
        Ok(context)
    }
}

#[cfg(feature = "auth-fortify")]
/// 短时且绑定可信请求来源的一次性登录 challenge。
pub struct TwoFactorLoginChallenge {
    token: String,
    user_id: i64,
    expires_at: DateTime<Utc>,
}

#[cfg(feature = "auth-fortify")]
impl TwoFactorLoginChallenge {
    pub(crate) fn new(token: String, user_id: i64, expires_at: DateTime<Utc>) -> Self {
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

#[cfg(feature = "auth-fortify")]
impl std::fmt::Debug for TwoFactorLoginChallenge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TwoFactorLoginChallenge")
            .field("token", &"[REDACTED]")
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}
