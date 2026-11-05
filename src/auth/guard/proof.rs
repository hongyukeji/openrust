//! 因子验证与同一 Session 认证转换之间的受控凭据。

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// 已实际消费或验证的因子方式，不以布尔值代替验证凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactorVerificationMethod {
    /// 现代 TOTP。
    Totp,
    /// 现代恢复码。
    RecoveryCode,
}

/// 已提交因子验证的内部收据；外部代码不能构造或反序列化。
///
/// 签发方须先完成数据库事务，不能持有数据库锁跨 Guard 登录事件 await。
pub struct FactorReceipt {
    proof: SessionFactorProof,
}
impl FactorReceipt {
    #[cfg(any(test, feature = "auth-fortify"))]
    pub(crate) fn issue(
        identifier: Value,
        guard: String,
        provider: Option<String>,
        source: String,
        generation: String,
        method: super::FactorVerificationMethod,
        verified_at: u64,
    ) -> Result<Self> {
        let proof = SessionFactorProof {
            identifier,
            guard,
            provider,
            source,
            generation,
            method,
            verified_at,
        };
        proof.validate()?;
        Ok(Self { proof })
    }
    /// 消费真实收据取得只读证明，供自定义StatefulGuard持久化；不能反向构造收据。
    pub fn into_proof(self) -> SessionFactorProof {
        self.proof
    }
}
impl fmt::Debug for FactorReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FactorReceipt").finish_non_exhaustive()
    }
}

/// 受保护 Session 内的因子证明快照；使用前必须向同一 authority 核当前代际。
///
/// 读取此值不代表当前数据库因子仍有效，亦不能据此签发新的验证收据。
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct SessionFactorProof {
    identifier: Value,
    guard: String,
    provider: Option<String>,
    source: String,
    generation: String,
    method: super::FactorVerificationMethod,
    verified_at: u64,
}
#[derive(Deserialize)]
struct StoredProof {
    identifier: Value,
    guard: String,
    provider: Option<String>,
    source: String,
    generation: String,
    method: super::FactorVerificationMethod,
    verified_at: u64,
}
impl SessionFactorProof {
    pub(crate) fn from_value(value: Value) -> Result<Self> {
        let stored: StoredProof = serde_json::from_value(value).map_err(Error::internal)?;
        let proof = Self {
            identifier: stored.identifier,
            guard: stored.guard,
            provider: stored.provider,
            source: stored.source,
            generation: stored.generation,
            method: stored.method,
            verified_at: stored.verified_at,
        };
        proof.validate()?;
        Ok(proof)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if !matches!(&self.identifier, Value::String(value) if !value.is_empty())
            && !matches!(&self.identifier, Value::Number(_))
        {
            return Err(invalid());
        }
        if self.guard.is_empty()
            || self.source.is_empty()
            || self.generation.is_empty()
            || self.provider.as_ref().is_some_and(String::is_empty)
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// 原始类型的账户标识。
    pub fn identifier(&self) -> &Value {
        &self.identifier
    }
    /// 验证时的 Guard 名称。
    pub fn guard(&self) -> &str {
        &self.guard
    }
    /// 验证时的 Provider 名称。
    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
    /// authority 的非空来源标识，例如数据库与表的稳定绑定。
    pub fn source(&self) -> &str {
        &self.source
    }
    /// authority 的当前状态代际。
    pub fn generation(&self) -> &str {
        &self.generation
    }
    /// 实际验证方式。
    pub fn method(&self) -> super::FactorVerificationMethod {
        self.method
    }
    /// 验证时间，Unix 秒。
    pub fn verified_at(&self) -> u64 {
        self.verified_at
    }
    /// 严格比较全部身份、来源和代际，不转换数字与字符串标识。
    pub fn matches(
        &self,
        identifier: &Value,
        guard: &str,
        provider: Option<&str>,
        source: &str,
        generation: &str,
    ) -> bool {
        self.validate().is_ok()
            && &self.identifier == identifier
            && self.guard == guard
            && self.provider() == provider
            && self.source == source
            && self.generation == generation
    }
    pub(crate) fn matches_identity(
        &self,
        identifier: &Value,
        guard: &str,
        provider: Option<&str>,
    ) -> bool {
        self.validate().is_ok()
            && &self.identifier == identifier
            && self.guard == guard
            && self.provider() == provider
    }
}
impl fmt::Debug for SessionFactorProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionFactorProof").finish_non_exhaustive()
    }
}
/// 当前真实持久身份的受限因子管理上下文；不能用作资源的第二因子证明。
///
/// 由真实Guard或受控JWT管理适配创建，数据库将管理连续性绑定到该凭据及准确代际。
#[derive(Clone)]
pub struct FactorManagementContext {
    // 即使关闭Fortify使领域字段被裁剪，公开类型仍不可由外部空字面量构造。
    _sealed: (),
    #[cfg(any(test, feature = "auth-fortify"))]
    identifier: Value,
    #[cfg(feature = "auth-fortify")]
    guard: String,
    #[cfg(feature = "auth-fortify")]
    provider: Option<String>,
    #[cfg(any(test, feature = "auth-fortify"))]
    session_binding: String,
    #[cfg(any(test, feature = "auth-fortify"))]
    factor_proof: Option<SessionFactorProof>,
    #[cfg(feature = "auth-fortify")]
    api_authorization: Option<crate::auth::fortify::ApiManagementAuthorization>,
    #[cfg(feature = "auth-fortify")]
    api_restart_pending: bool,
}
impl FactorManagementContext {
    pub(crate) fn issue(
        identifier: Value,
        guard: String,
        provider: Option<String>,
        session_binding: String,
        factor_proof: Option<SessionFactorProof>,
    ) -> Result<Self> {
        if !matches!(&identifier, Value::String(id) if !id.trim().is_empty())
            && !matches!(&identifier, Value::Number(_))
            || guard.is_empty()
            || provider.as_ref().is_some_and(String::is_empty)
            || session_binding.len() != 64
            || !session_binding
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || factor_proof.as_ref().is_some_and(|proof| {
                !proof.matches_identity(&identifier, &guard, provider.as_deref())
            })
        {
            return Err(invalid());
        }
        // 保留跨feature的所有权签发契约；关闭Fortify时验证后及时释放不持有的领域信息。
        #[cfg(not(feature = "auth-fortify"))]
        drop((guard, provider));
        #[cfg(not(any(test, feature = "auth-fortify")))]
        drop((identifier, session_binding, factor_proof));
        Ok(Self {
            _sealed: (),
            #[cfg(any(test, feature = "auth-fortify"))]
            identifier,
            #[cfg(feature = "auth-fortify")]
            guard,
            #[cfg(feature = "auth-fortify")]
            provider,
            #[cfg(any(test, feature = "auth-fortify"))]
            session_binding,
            #[cfg(any(test, feature = "auth-fortify"))]
            factor_proof,
            #[cfg(feature = "auth-fortify")]
            api_authorization: None,
            #[cfg(feature = "auth-fortify")]
            api_restart_pending: false,
        })
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn with_api_authorization(
        mut self,
        authorization: crate::auth::fortify::ApiManagementAuthorization,
    ) -> Self {
        self.api_authorization = Some(authorization);
        self
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn for_api_pending_restart(mut self) -> Self {
        self.api_restart_pending = true;
        self
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn is_api_pending_restart(&self) -> bool {
        self.api_restart_pending
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn has_api_authorization(&self) -> bool {
        self.api_authorization.is_some()
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn api_initial_snapshot_matches(&self, source: &str, generation: &str) -> bool {
        self.api_authorization
            .as_ref()
            .is_none_or(|authorization| authorization.initial_snapshot_matches(source, generation))
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) async fn validate_api_in_transaction(
        &self,
        db: &crate::db::Db,
        transaction: &crate::db::sea_orm::DatabaseTransaction,
    ) -> Result<()> {
        if let Some(authorization) = &self.api_authorization {
            let user_id = self.identifier.as_i64().ok_or(Error::Unauthorized)?;
            authorization
                .validate_in_transaction(db, transaction, user_id)
                .await?;
        }
        Ok(())
    }
    #[cfg(any(test, feature = "auth-fortify"))]
    pub(crate) fn identifier(&self) -> &Value {
        &self.identifier
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn guard(&self) -> &str {
        &self.guard
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
    #[cfg(any(test, feature = "auth-fortify"))]
    pub(crate) fn session_binding(&self) -> &str {
        &self.session_binding
    }
    #[cfg(feature = "auth-fortify")]
    pub(crate) fn matches_session_id(
        &self,
        session_id: &crate::session::SessionId,
    ) -> Result<bool> {
        use sha2::{Digest as _, Sha256};
        let binding = serde_json::to_string(&(
            session_id,
            &self.guard,
            self.provider.as_deref(),
            &self.identifier,
        ))
        .map_err(Error::internal)?;
        Ok(self.session_binding == format!("{:x}", Sha256::digest(binding.as_bytes())))
    }
    #[cfg(any(test, feature = "auth-fortify"))]
    pub(crate) fn factor_proof(&self) -> Option<&SessionFactorProof> {
        self.factor_proof.as_ref()
    }
}
impl fmt::Debug for FactorManagementContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FactorManagementContext")
            .finish_non_exhaustive()
    }
}

fn invalid() -> Error {
    Error::custom(
        "auth_factor_receipt_invalid",
        "因子收据的身份、来源或代际无效",
    )
}

pub(crate) fn session_factor_key(session_key: &str) -> String {
    format!("_openrust_factor_proof_{session_key}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipts_reject_ambiguous_identity_and_empty_authority_bindings() {
        for identifier in [
            Value::Null,
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!(true),
        ] {
            assert!(
                FactorReceipt::issue(
                    identifier,
                    "web".into(),
                    None,
                    "source".into(),
                    "1".into(),
                    FactorVerificationMethod::Totp,
                    0
                )
                .is_err()
            );
        }
        for (source, generation) in [("", "1"), ("source", "")] {
            assert!(
                FactorReceipt::issue(
                    serde_json::json!(42),
                    "web".into(),
                    None,
                    source.into(),
                    generation.into(),
                    FactorVerificationMethod::RecoveryCode,
                    0
                )
                .is_err()
            );
        }
    }
}
