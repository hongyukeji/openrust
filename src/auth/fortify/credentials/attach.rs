//! OIDC关联复用已验证的实际Session来源，不为旧流程制造具名Guard。

use super::{
    CredentialAuthority, CredentialDomain, CredentialFactorContext, DatabaseTransaction, DateTime,
    Error, Result, StatefulGuard, Utc, Value, VerifiedCredential,
};
use crate::{
    auth::{SessionFactorProof, credential::StoredSessionFactorContext},
    session::{Session, SessionId, SessionStore},
};
use sha2::{Digest as _, Sha256};
use std::sync::Arc;

/// 只供同一次受控关联流程使用，不可从账户ID或布尔值构造。
pub(crate) struct PreparedSessionAttachment {
    authority: CredentialAuthority,
    session: Session,
    session_id: SessionId,
    handler: Arc<dyn SessionStore>,
    user_id: i64,
    prepared_at: DateTime<Utc>,
    origin: AttachmentOrigin,
}
enum AttachmentOrigin {
    SessionContext {
        parent: Box<crate::auth::credential::CredentialSessionParent>,
        context: CredentialFactorContext,
        binding: String,
    },
    Guard {
        credential: Box<VerifiedCredential>,
        session_key: String,
        proof_key: String,
        proof: Option<Value>,
    },
}
impl std::fmt::Debug for PreparedSessionAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSessionAttachment")
            .finish_non_exhaustive()
    }
}
impl PreparedSessionAttachment {
    pub(crate) const fn user_id(&self) -> i64 {
        self.user_id
    }
    fn check_age(&self) -> Result<()> {
        let now = Utc::now();
        if self.prepared_at > now || now.signed_duration_since(self.prepared_at).num_seconds() > 30
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    fn check_session(&self, session: &Session) -> Result<()> {
        session.require_authenticated_storage()?;
        if session.id()? != self.session_id || !Arc::ptr_eq(&self.handler, &session.handler()?) {
            return Err(Error::Unauthorized);
        }
        match &self.origin {
            AttachmentOrigin::SessionContext {
                context, binding, ..
            } => {
                if session.get_typed::<i64>(crate::auth::AUTH_USER_ID_KEY)? != Some(self.user_id) {
                    return Err(Error::Unauthorized);
                }
                let stored = session
                    .get_typed::<StoredSessionFactorContext>(
                        crate::auth::credential::SESSION_FACTOR_CONTEXT_KEY,
                    )?
                    .ok_or(Error::Unauthorized)?;
                if &stored.context != context
                    || &stored.binding != binding
                    || stored.binding != super::session_context_binding(session, &stored.context)?
                {
                    return Err(Error::Unauthorized);
                }
            }
            AttachmentOrigin::Guard {
                credential,
                session_key,
                proof_key,
                proof,
            } => {
                let context = credential.context.as_ref().ok_or(Error::Unauthorized)?;
                if session.get_json(session_key)?.as_ref() != Some(&context.identifier)
                    || session.get_json(proof_key)? != *proof
                {
                    return Err(Error::Unauthorized);
                }
                let CredentialDomain::Session {
                    guard,
                    provider,
                    binding,
                } = &credential.domain
                else {
                    return Err(Error::Unauthorized);
                };
                // 固定实际Guard/Provider/typedID公式；不可只比较传入对象里的同user快照。
                let encoded = serde_json::to_vec(&(
                    &self.session_id,
                    guard,
                    provider.as_deref(),
                    &context.identifier,
                ))
                .map_err(Error::internal)?;
                if *binding != format!("{:x}", Sha256::digest(encoded)) {
                    return Err(Error::Unauthorized);
                }
                match proof {
                    Some(raw) => {
                        let actual = SessionFactorProof::from_value(raw.clone())?;
                        if !actual.matches(
                            &context.identifier,
                            guard,
                            provider.as_deref(),
                            &context.source,
                            &context.generation,
                        ) || context.factor.as_ref().is_none_or(|factor| {
                            factor.method != actual.method()
                                || factor.verified_at != actual.verified_at()
                        }) {
                            return Err(Error::Unauthorized);
                        }
                    }
                    None if context.factor.is_some() => return Err(Error::Unauthorized),
                    None => {}
                }
            }
        }
        Ok(())
    }
    /// 可在关联事务的UPDATE前后调用；无Guard/Provider或存储await。
    pub(crate) fn recheck_session(&self, session: &Session) -> Result<()> {
        self.check_age()?;
        self.check_session(&self.session)?;
        self.check_session(session)
    }
    /// 只能在用户事务外调用；重新读取真实Guard，之后同步复核Session没有变化。
    pub(crate) async fn recheck_guard(
        &self,
        session: &Session,
        guard: &dyn StatefulGuard,
    ) -> Result<()> {
        self.recheck_session(session)?;
        let AttachmentOrigin::Guard { credential, .. } = &self.origin else {
            return Err(Error::Unauthorized);
        };
        let management = guard.factor_management_context().await?;
        if !management.matches_session_id(&self.session_id)? {
            return Err(Error::Unauthorized);
        }
        let again = self
            .authority
            .prepare_session(guard, management.guard())
            .await?;
        if again.user_id != self.user_id
            || again.domain != credential.domain
            || again.context != credential.context
        {
            return Err(Error::Unauthorized);
        }
        self.recheck_session(session)
    }
}
impl CredentialAuthority {
    pub(crate) async fn prepare_session_context_attachment(
        &self,
        user_id: i64,
        session: &Session,
    ) -> Result<PreparedSessionAttachment> {
        session.require_authenticated_storage()?;
        if session.get_typed::<i64>(crate::auth::AUTH_USER_ID_KEY)? != Some(user_id) {
            return Err(Error::Unauthorized);
        }
        let stored = session
            .get_typed::<StoredSessionFactorContext>(
                crate::auth::credential::SESSION_FACTOR_CONTEXT_KEY,
            )?
            .ok_or(Error::Unauthorized)?;
        stored.context.validate_for_user(user_id)?;
        if stored.binding != super::session_context_binding(session, &stored.context)? {
            return Err(Error::Unauthorized);
        }
        let prepared = PreparedSessionAttachment {
            authority: self.clone(),
            session: session.clone(),
            session_id: session.id()?,
            handler: session.handler()?,
            user_id,
            prepared_at: Utc::now(),
            origin: AttachmentOrigin::SessionContext {
                parent: Box::new(
                    crate::auth::credential::CredentialSessionParent::capture_session_context(
                        session, &stored,
                    )?,
                ),
                context: stored.context,
                binding: stored.binding,
            },
        };
        prepared.recheck_session(session)?;
        let this = self.clone();
        let prepared = self
            .db
            .transaction_for_update(move |tx| {
                Box::pin(async move {
                    this.validate_attachment_in_transaction(tx, &prepared)
                        .await?;
                    Ok(prepared)
                })
            })
            .await?;
        prepared.recheck_session(session)?;
        Ok(prepared)
    }
    pub(crate) async fn prepare_guard_attachment(
        &self,
        user_id: i64,
        session: &Session,
        guard: &dyn StatefulGuard,
    ) -> Result<PreparedSessionAttachment> {
        session.require_authenticated_storage()?;
        let management = guard.factor_management_context().await?;
        let session_id = session.id()?;
        if management.identifier() != &Value::from(user_id)
            || !management.matches_session_id(&session_id)?
        {
            return Err(Error::Unauthorized);
        }
        let session_key =
            crate::auth::session::guard_session_key(session, guard, &Value::from(user_id)).await?;
        let credential = self.prepare_session(guard, management.guard()).await?;
        if credential.user_id != user_id {
            return Err(Error::Unauthorized);
        }
        let after = guard.factor_management_context().await?;
        if !after.matches_session_id(&session_id)?
            || after.session_binding() != management.session_binding()
            || after.guard() != management.guard()
            || after.provider() != management.provider()
        {
            return Err(Error::Unauthorized);
        }
        let proof_key = crate::auth::guard::session_factor_key(&session_key);
        let proof = session.get_json(&proof_key)?;
        let prepared = PreparedSessionAttachment {
            authority: self.clone(),
            session: session.clone(),
            session_id,
            handler: session.handler()?,
            user_id,
            prepared_at: credential.prepared_at,
            origin: AttachmentOrigin::Guard {
                credential: Box::new(credential),
                session_key,
                proof_key,
                proof,
            },
        };
        prepared.recheck_session(session)?;
        Ok(prepared)
    }
    pub(crate) async fn validate_attachment_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        prepared: &PreparedSessionAttachment,
    ) -> Result<()> {
        self.require_store(&prepared.authority.db)?;
        prepared.recheck_session(&prepared.session)?;
        match &prepared.origin {
            AttachmentOrigin::SessionContext {
                context, parent, ..
            } => {
                self.check_context_in_transaction(transaction, prepared.user_id, context)
                    .await?;
                parent.validate(&self.db, transaction).await?;
            }
            AttachmentOrigin::Guard { credential, .. } => {
                self.validate_parent_in_transaction(transaction, credential)
                    .await?;
            }
        }
        prepared.recheck_session(&prepared.session)
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests;
