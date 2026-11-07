//! 显式数据库凭据的统一因子裁决；签名事实不替代当前主库与撤销锚。

mod challenge;
#[cfg(test)]
mod lock_tests;
pub(crate) use challenge::{
    ChallengeCommitCheck, CredentialChallengeVerification, CredentialLoginDecision,
    PreparedCredentialChallenge,
};

#[cfg(any(feature = "auth-oidc", all(test, feature = "db-sqlite")))]
mod attach;
#[cfg(feature = "auth-oidc")]
pub(crate) use attach::PreparedSessionAttachment;

use super::{DatabaseFactorAuthority, DatabaseTwoFactorState};
use crate::Error;
use crate::Result;
use crate::auth::CredentialDomain;
use crate::auth::FactorOrigin;
use crate::auth::JwtTokens;
use crate::auth::RefreshTokens;
use crate::auth::StatefulGuard;
use crate::auth::VerifiedCredential;
use crate::auth::credential::CredentialFactorContext;
use crate::auth::credential::FactorEvidence;
use crate::database::Db;
use crate::database::sea_orm::ConnectionTrait;
use crate::database::sea_orm::DatabaseTransaction;
use crate::database::sea_orm::DbBackend;
use crate::database::sea_orm::Statement;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// 已显式选择的整数users凭据策略，复用同一authority与主库。
///
/// 不验证密码或凭空生成因子事实；公开准备入口要求实际持久Guard身份。
#[derive(Clone)]
pub struct CredentialAuthority {
    db: Db,
    confirm: bool,
    config: Option<crate::config::repository::ConfigRepository>,
    #[cfg(test)]
    parent_prechecked: Option<std::sync::Arc<tokio::sync::Notify>>,
}
impl std::fmt::Debug for CredentialAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialAuthority")
            .finish_non_exhaustive()
    }
}
impl CredentialAuthority {
    pub(crate) fn store_session_context(
        session: &crate::session::Session,
        context: CredentialFactorContext,
    ) -> Result<()> {
        let user_id = context.identifier.as_i64().ok_or(Error::Unauthorized)?;
        context.validate_for_user(user_id)?;
        session.require_authenticated_storage()?;
        let binding = session_context_binding(session, &context)?;
        session.put_typed(
            crate::auth::credential::SESSION_FACTOR_CONTEXT_KEY,
            crate::auth::credential::StoredSessionFactorContext { context, binding },
        )?;
        session.record_new_credential_login(crate::auth::AUTH_USER_ID_KEY)
    }
    pub(crate) async fn authorize_session_context(
        &self,
        user_id: i64,
        session: &crate::session::Session,
    ) -> Result<()> {
        session.require_authenticated_storage()?;
        let stored = session
            .get_typed::<crate::auth::credential::StoredSessionFactorContext>(
                crate::auth::credential::SESSION_FACTOR_CONTEXT_KEY,
            )?
            .ok_or(Error::Unauthorized)?;
        stored.context.validate_for_user(user_id)?;
        if stored.binding != session_context_binding(session, &stored.context)? {
            return Err(Error::Unauthorized);
        }
        let this = self.clone();
        self.db
            .transaction_for_update(move |transaction| {
                Box::pin(async move {
                    this.check_context_in_transaction(transaction, user_id, &stored.context)
                        .await
                })
            })
            .await
    }
    /// 选择真实共享数据库及当前确认策略。
    pub fn new(db: Db, confirm: bool) -> Self {
        Self {
            db,
            confirm,
            config: None,
            #[cfg(test)]
            parent_prechecked: None,
        }
    }
    pub(crate) fn with_config_repository(
        mut self,
        config: crate::config::repository::ConfigRepository,
    ) -> Self {
        self.config = Some(config);
        self
    }
    fn confirmation_required(&self) -> Result<bool> {
        match &self.config {
            Some(config) => super::options::confirmation_required(config),
            None => Ok(self.confirm),
        }
    }
    pub(crate) fn db(&self) -> &Db {
        &self.db
    }
    pub(crate) fn require_store(&self, db: &Db) -> Result<()> {
        if !self.db.same_handle(db) {
            return Err(Error::Unauthorized);
        }
        for table in [
            "users",
            "auth_refresh_tokens",
            "personal_access_tokens",
            "auth_factor_challenges",
        ] {
            if self.db.table_name(table)? != db.table_name(table)? {
                return Err(Error::Unauthorized);
            }
        }
        Ok(())
    }
    fn account(&self, user_id: i64) -> Result<DatabaseFactorAuthority> {
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        Ok(DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(
            self.db.clone(),
            "users",
            "id",
            Value::from(user_id),
        )?))
    }
    pub(crate) async fn lock_active_user_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<()> {
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        let backend = transaction.get_database_backend();
        let (parameter, lock) = match backend {
            DbBackend::Postgres => ("$1", " FOR UPDATE"),
            DbBackend::MySql => ("?", " FOR UPDATE"),
            DbBackend::Sqlite => ("?", ""),
            _ => return Err(Error::Unauthorized),
        };
        transaction
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!(
                    "SELECT id FROM {} WHERE id={parameter} AND disabled_at IS NULL{lock}",
                    self.db.quoted_table_name("users")?
                ),
                [user_id.into()],
            ))
            .await
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        Ok(())
    }
    pub(crate) async fn authorize_live_context(
        &self,
        user: &crate::auth::AuthUser,
        provider: &dyn crate::auth::UserProvider,
        proof: Option<&crate::auth::SessionFactorProof>,
        guard: &str,
        provider_name: Option<&str>,
    ) -> Result<(String, String)> {
        let identifier = user.auth_identifier()?;
        let user_id = identifier
            .as_i64()
            .filter(|id| *id > 0)
            .ok_or(Error::Unauthorized)?;
        if user.auth_identifier_name() != "id"
            || !provider.uses_database_accounts(&self.db, "users")?
        {
            return Err(Error::Unauthorized);
        }
        user.as_two_factor()
            .and_then(|model| model.two_factor_authority())
            .ok_or(Error::Unauthorized)?
            .validate_account(user, provider)
            .await?;
        let this = self.clone();
        let proof = proof.cloned();
        let guard = guard.to_owned();
        let provider_name = provider_name.map(str::to_owned);
        self.db
            .transaction_for_update(move |transaction| {
                Box::pin(async move {
                    this.lock_active_user_in_transaction(transaction, user_id)
                        .await?;
                    let snapshot = this
                        .account(user_id)?
                        .load_in_transaction(&this.db, transaction)
                        .await?;
                    let valid = match &proof {
                        Some(proof) => proof.matches(
                            &identifier,
                            &guard,
                            provider_name.as_deref(),
                            &snapshot.source,
                            &snapshot.generation,
                        ),
                        None => !snapshot.challenge_required(this.confirmation_required()?),
                    };
                    if !valid {
                        return Err(Error::Unauthorized);
                    }
                    Ok((snapshot.source, snapshot.generation))
                })
            })
            .await
    }

    pub(crate) async fn check_context_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
        context: &CredentialFactorContext,
    ) -> Result<()> {
        context.validate_for_user(user_id)?;
        self.lock_active_user_in_transaction(transaction, user_id)
            .await?;
        let current = self
            .account(user_id)?
            .load_in_transaction(&self.db, transaction)
            .await?;
        if context.source != current.source
            || context.generation != current.generation
            || (current.challenge_required(self.confirmation_required()?)
                && context.factor.is_none())
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    // 仅由已完成真实第一因素的AuthFlows调用；无因子需求与已MFA是不同状态。
    pub(crate) async fn context_without_factor_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<CredentialFactorContext> {
        self.lock_active_user_in_transaction(transaction, user_id)
            .await?;
        let current = self
            .account(user_id)?
            .load_in_transaction(&self.db, transaction)
            .await?;
        if current.challenge_required(self.confirmation_required()?) {
            return Err(Error::custom_with_status(
                http::StatusCode::FORBIDDEN,
                "auth_two_factor_required",
                "请先完成当前账户的双因素验证",
            ));
        }
        Ok(CredentialFactorContext {
            scope: None,
            version: 1,
            identifier: Value::from(user_id),
            source: current.source,
            generation: current.generation,
            factor: None,
        })
    }
    /// 从真实持久Session准备一次短时派生票据，保留实际Guard验证来源。
    ///
    /// 在用户事务外解析 Guard/Provider，签发事务内再核实际 Session 绑定、存储记录和期限。
    pub async fn prepare_session(
        &self,
        guard: &dyn StatefulGuard,
        guard_name: &str,
    ) -> Result<VerifiedCredential> {
        let user = guard.user().await?.ok_or(Error::Unauthorized)?;
        if user.auth_identifier_name() != "id"
            || !guard.provider().uses_database_accounts(&self.db, "users")?
        {
            return Err(Error::Unauthorized);
        }
        let identifier = user.auth_identifier()?;
        let user_id = identifier
            .as_i64()
            .filter(|id| *id > 0)
            .ok_or(Error::Unauthorized)?;
        let selected = user
            .as_two_factor()
            .and_then(super::TwoFactorAuthenticatable::two_factor_authority)
            .ok_or(Error::Unauthorized)?;
        selected
            .validate_account(&user, guard.provider().as_ref())
            .await?;
        let management = guard.factor_management_context().await?;
        if management.identifier() != &identifier
            || management.guard() != guard_name
            || management.provider() != guard.provider_name()
        {
            return Err(Error::Unauthorized);
        }
        let current = selected.load().await?;
        let factor = match management.factor_proof() {
            Some(proof)
                if proof.matches(
                    &identifier,
                    guard_name,
                    guard.provider_name(),
                    &current.source,
                    &current.generation,
                ) =>
            {
                Some(FactorEvidence {
                    method: proof.method(),
                    verified_at: proof.verified_at(),
                    origin: FactorOrigin::Guard {
                        guard: guard_name.into(),
                        provider: guard.provider_name().map(str::to_owned),
                    },
                })
            }
            Some(_) => return Err(Error::Unauthorized),
            None if current.challenge_required(self.confirmation_required()?) => {
                return Err(Error::Unauthorized);
            }
            None => None,
        };
        let after = guard.factor_management_context().await?;
        if after.identifier() != &identifier
            || after.guard() != guard_name
            || after.provider() != management.provider()
            || after.session_binding() != management.session_binding()
            || after.factor_proof() != management.factor_proof()
        {
            return Err(Error::Unauthorized);
        }
        let context = CredentialFactorContext {
            scope: None,
            version: 1,
            identifier,
            source: current.source,
            generation: current.generation,
            factor,
        };
        context.validate_for_user(user_id)?;
        let parent = crate::auth::credential::CredentialSessionParent::capture(guard, &after)?;
        let db = self.db.clone();
        let (parent, deadline) = self
            .db
            .transaction_for_update(move |tx| {
                Box::pin(async move {
                    let deadline = parent.validate(&db, tx).await?;
                    Ok((parent, deadline))
                })
            })
            .await?;
        Ok(VerifiedCredential {
            session_parent: Some(Box::new(parent)),
            store: self.db.clone(),
            user_id,
            domain: CredentialDomain::Session {
                guard: guard_name.into(),
                provider: guard.provider_name().map(str::to_owned),
                binding: management.session_binding().into(),
            },
            context: Some(context),
            expires_at: Some(deadline),
            abilities: None,
            prepared_at: Utc::now(),
        })
    }
    /// 同时核验JWT签名、当前账户因子版本及实际refresh家族，不从裸ID建立证明。
    pub async fn authenticate_access_token(
        &self,
        tokens: &JwtTokens,
        raw: &str,
    ) -> Result<VerifiedCredential> {
        let claims = tokens.verify_credential(raw)?;
        let grant = claims.grant.ok_or(Error::Unauthorized)?;
        let this = self.clone();
        let refresh = RefreshTokens::new(self.db.clone());
        self.db
            .transaction_for_update(move |transaction| {
                Box::pin(async move {
                    this.check_context_in_transaction(transaction, claims.user_id, &grant.context)
                        .await?;
                    let family_deadline = refresh
                        .validate_family_in_transaction(
                            transaction,
                            claims.user_id,
                            &grant.family_id,
                            &grant.context,
                        )
                        .await?;
                    let expires_at = DateTime::<Utc>::from(claims.expires_at).min(family_deadline);
                    if expires_at <= Utc::now() {
                        return Err(Error::Unauthorized);
                    }
                    Ok(VerifiedCredential {
                        session_parent: None,
                        store: this.db.clone(),
                        user_id: claims.user_id,
                        domain: CredentialDomain::Jwt {
                            issuer: claims.issuer,
                            audience: claims.audience,
                            jti: claims.jti,
                            family_id: grant.family_id,
                        },
                        context: Some(grant.context),
                        expires_at: Some(expires_at),
                        abilities: None,
                        prepared_at: Utc::now(),
                    })
                })
            })
            .await
    }
    async fn validate_personal_parent(
        &self,
        transaction: &DatabaseTransaction,
        credential: &VerifiedCredential,
        context: &CredentialFactorContext,
        id: i64,
        token_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let query = self
            .db
            .table("personal_access_tokens")?
            .using_transaction(transaction)
            .where_eq("id", id)?
            .where_eq("user_id", credential.user_id)?
            .where_eq("token_hash", token_hash.to_owned())?;
        let query = if transaction.get_database_backend() == DbBackend::Sqlite {
            query
        } else {
            query.lock_for_update()?
        };
        let row = query.first().await?.ok_or(Error::Unauthorized)?;
        if row
            .try_get::<String>("", "token_hash")
            .map_err(Error::internal)?
            != token_hash
        {
            return Err(Error::Unauthorized);
        }
        let current_expiry = row
            .try_get::<Option<DateTime<Utc>>>("", "expires_at")
            .map_err(Error::internal)?;
        if current_expiry != credential.expires_at {
            return Err(Error::Unauthorized);
        }
        if row
            .try_get::<Option<DateTime<Utc>>>("", "revoked_at")
            .map_err(Error::internal)?
            .is_some()
            || row
                .try_get::<Option<DateTime<Utc>>>("", "expires_at")
                .map_err(Error::internal)?
                .is_some_and(|expiry| expiry <= now)
        {
            return Err(Error::Unauthorized);
        }
        let stored = row
            .try_get::<Option<String>>("", "factor_context")
            .map_err(Error::internal)?
            .ok_or(Error::Unauthorized)?;
        if CredentialFactorContext::decode(&stored, credential.user_id)? != *context {
            return Err(Error::Unauthorized);
        }
        let abilities: Value = if transaction.get_database_backend() == DbBackend::Sqlite {
            serde_json::from_str(
                &row.try_get::<String>("", "abilities")
                    .map_err(Error::internal)?,
            )
            .map_err(|_| Error::Unauthorized)?
        } else {
            row.try_get("", "abilities").map_err(Error::internal)?
        };
        let abilities = abilities
            .as_array()
            .ok_or(Error::Unauthorized)?
            .iter()
            .map(|value| value.as_str().map(str::to_owned).ok_or(Error::Unauthorized))
            .collect::<Result<Vec<_>>>()?;
        if credential.abilities.as_ref() != Some(&abilities) {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    pub(crate) async fn validate_parent_in_transaction(
        &self,
        transaction: &DatabaseTransaction,
        credential: &VerifiedCredential,
    ) -> Result<()> {
        self.require_store(&credential.store)?;
        let now = Utc::now();
        if credential.prepared_at > now
            || now
                .signed_duration_since(credential.prepared_at)
                .num_seconds()
                > 30
            || credential.expires_at.is_some_and(|expiry| expiry <= now)
        {
            return Err(Error::Unauthorized);
        }
        #[cfg(test)]
        if let Some(signal) = &self.parent_prechecked {
            signal.notify_one();
        }
        let context = credential.context.as_ref().ok_or(Error::Unauthorized)?;
        self.check_context_in_transaction(transaction, credential.user_id, context)
            .await?;
        match &credential.domain {
            CredentialDomain::Session {
                guard,
                provider,
                binding,
            } => {
                if guard.is_empty()
                    || binding.len() != 64
                    || !binding
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(Error::Unauthorized);
                }
                if let Some(factor) = &context.factor
                    && factor.origin
                        != (FactorOrigin::Guard {
                            guard: guard.clone(),
                            provider: provider.clone(),
                        })
                {
                    return Err(Error::Unauthorized);
                }
                credential
                    .session_parent
                    .as_ref()
                    .ok_or(Error::Unauthorized)?
                    .validate(&self.db, transaction)
                    .await?;
            }
            CredentialDomain::Jwt { family_id, .. } => {
                let deadline = RefreshTokens::new(self.db.clone())
                    .validate_family_in_transaction(
                        transaction,
                        credential.user_id,
                        family_id,
                        context,
                    )
                    .await?;
                if credential.expires_at.is_none_or(|expiry| expiry > deadline) {
                    return Err(Error::Unauthorized);
                }
            }
            CredentialDomain::PersonalAccessToken { id, token_hash } => {
                self.validate_personal_parent(
                    transaction,
                    credential,
                    context,
                    *id,
                    token_hash,
                    now,
                )
                .await?;
            }
            CredentialDomain::Refresh { .. } => return Err(Error::Unauthorized),
        }
        let completed_at = Utc::now();
        if credential.prepared_at > completed_at
            || completed_at
                .signed_duration_since(credential.prepared_at)
                .num_seconds()
                > 30
            || credential
                .expires_at
                .is_some_and(|expiry| expiry <= completed_at)
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}

fn session_context_binding(
    session: &crate::session::Session,
    context: &CredentialFactorContext,
) -> Result<String> {
    use sha2::{Digest, Sha256};
    let encoded = serde_json::to_vec(&(
        session.id()?.to_string(),
        &context.identifier,
        &context.source,
        &context.generation,
    ))
    .map_err(Error::internal)?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}
