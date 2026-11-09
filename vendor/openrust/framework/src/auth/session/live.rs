//! 只读长连接凭据；每次从真实服务器端记录读取身份及因子证明。

use crate::{
    Error, Result,
    auth::{SessionFactorProof, StatefulGuard, UserProvider, fortify::CredentialAuthority},
    session::{DEFAULT_MAX_BYTES, Session, SessionId, SessionStore, SessionStoreGuarantees},
};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use time::OffsetDateTime;

/// 绑定实际持久Session身份的只读长连接凭据。
///
/// 不能从标识或布尔值构造，不保存请求Scope、Guard缓存或请求属性快照。
/// 每次授权重读真实服务器端记录，不续期、恢复remember或签发因子证明。
#[derive(Clone)]
pub struct LiveSessionCredential {
    handler: Arc<dyn SessionStore>,
    provider: Arc<dyn UserProvider>,
    session_id: SessionId,
    session_key: String,
    proof_key: String,
    identifier: Value,
    guard_name: String,
    provider_name: Option<String>,
    handshake_deadline: SystemTime,
    stamp: Arc<std::sync::Mutex<Option<(String, String)>>>,
}
impl std::fmt::Debug for LiveSessionCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveSessionCredential")
            .finish_non_exhaustive()
    }
}
impl super::SessionStoreHandle {
    /// 从当前真实持久Guard准备可跨请求使用的只读凭据。
    ///
    /// 驱动须提供独立请求的真实存储读取；客户端Cookie快照不具此能力。
    /// 原生数据库只承诺本次load时点，不提升其既有删除/并发写保证。
    #[allow(
        clippy::unused_self,
        reason = "公开工厂保留句柄入口，读取域必须绑定真实Session而非外部句柄"
    )]
    pub async fn prepare_live_guard(
        &self,
        session: &Session,
        guard: &dyn StatefulGuard,
    ) -> Result<LiveSessionCredential> {
        session.require_authenticated_storage()?;
        let context = guard.factor_management_context().await?;
        let identifier = guard.id().await?.ok_or(Error::Unauthorized)?;
        if context.identifier() != &identifier || context.provider() != guard.provider_name() {
            return Err(Error::Unauthorized);
        }
        let session_key = super::guard_session_key(session, guard, &identifier).await?;
        let session_id = session.id()?;
        if !context.matches_session_id(&session_id)? {
            return Err(Error::Unauthorized);
        }
        let actual = session.handler()?;
        let handler = actual.independent_authorization_reader()?.unwrap_or(actual);
        if handler.guarantees() == SessionStoreGuarantees::ClientSideSnapshot {
            return Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_live_reader_unsupported",
                "当前Session驱动不能提供独立请求的服务器端实时凭据读取",
            ));
        }
        let handshake_deadline = super::persisted_until(session, &session_key, &identifier).await?;
        let independent = handler
            .load_authorized(&session_id, DEFAULT_MAX_BYTES)
            .await?
            .ok_or(Error::Unauthorized)?;
        independent.encode(DEFAULT_MAX_BYTES)?;
        if independent.id != session_id
            || independent.expiry_date <= OffsetDateTime::now_utc()
            || crate::support::data::get(&independent.data, &session_key).as_ref()
                != Some(&identifier)
        {
            return Err(Error::Unauthorized);
        }
        let after = guard.factor_management_context().await?;
        if session.id()? != session_id
            || context.identifier() != after.identifier()
            || context.guard() != after.guard()
            || context.provider() != after.provider()
            || context.session_binding() != after.session_binding()
            || !after.matches_session_id(&session_id)?
        {
            return Err(Error::Unauthorized);
        }
        session.require_authenticated_storage()?;
        Ok(LiveSessionCredential {
            handler,
            provider: guard.provider(),
            session_id,
            proof_key: crate::auth::guard::session_factor_key(&session_key),
            session_key,
            identifier,
            guard_name: context.guard().into(),
            provider_name: context.provider().map(str::to_owned),
            handshake_deadline,
            stamp: Arc::new(std::sync::Mutex::new(None)),
        })
    }
}
impl LiveSessionCredential {
    /// 重读同一Session、实际Provider及当前因子authority后返回最短有效期限。
    ///
    /// 任意存储、Provider或证明错误均拒绝；回调返回前不持有数据库锁。
    pub async fn authorize(&self, authority: &CredentialAuthority) -> Result<SystemTime> {
        if self.handshake_deadline <= SystemTime::now() {
            return Err(Error::Unauthorized);
        }
        let record = self
            .handler
            .load_authorized(&self.session_id, DEFAULT_MAX_BYTES)
            .await?
            .ok_or(Error::Unauthorized)?;
        if record.id != self.session_id
            || record.expiry_date <= OffsetDateTime::now_utc()
            || crate::support::data::get(&record.data, &self.session_key).as_ref()
                != Some(&self.identifier)
        {
            return Err(Error::Unauthorized);
        }
        record.encode(DEFAULT_MAX_BYTES)?;
        let proof = crate::support::data::get(&record.data, &self.proof_key)
            .map(SessionFactorProof::from_value)
            .transpose()?;
        if proof.as_ref().is_some_and(|proof| {
            !proof.matches_identity(
                &self.identifier,
                &self.guard_name,
                self.provider_name.as_deref(),
            )
        }) {
            return Err(Error::Unauthorized);
        }
        // Provider检索结束才进入authority用户锁；不持数据库锁跨自定义Provider await。
        let user = self
            .provider
            .retrieve_by_id(&self.identifier)
            .await?
            .ok_or(Error::Unauthorized)?;
        if user.auth_identifier()? != self.identifier {
            return Err(Error::Unauthorized);
        }
        let stamp = authority
            .authorize_live_context(
                &user,
                self.provider.as_ref(),
                proof.as_ref(),
                &self.guard_name,
                self.provider_name.as_deref(),
            )
            .await?;
        let seconds =
            u64::try_from(record.expiry_date.unix_timestamp()).map_err(|_| Error::Unauthorized)?;
        let deadline = UNIX_EPOCH
            .checked_add(Duration::new(seconds, record.expiry_date.nanosecond()))
            .ok_or(Error::Unauthorized)?
            .min(self.handshake_deadline);
        // 上述handler/Provider/数据库锁都可能等待；返回前必须重新采样真实时间。
        if deadline <= SystemTime::now() {
            return Err(Error::Unauthorized);
        }
        let mut bound = self.stamp.lock().map_err(|_| Error::Unauthorized)?;
        if let Some(expected) = bound.as_ref() {
            if expected != &stamp {
                return Err(Error::Unauthorized);
            }
        } else {
            *bound = Some(stamp);
        }
        Ok(deadline)
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "../../../tests/unit/auth/live_session.rs"]
mod tests;
