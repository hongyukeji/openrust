//! 显式JWT API配置的短期因子管理授权；不建立Session，也不是资源凭据。
use super::{ConfirmPasswordsUsing, CredentialAuthority, TwoFactorActions};
use crate::Error;
use crate::Result;
use crate::auth::AuthUser;
use crate::auth::CredentialDomain;
use crate::auth::Credentials;
use crate::auth::FactorManagementContext;
use crate::auth::FactorOrigin;
use crate::auth::JwtTokens;
use crate::auth::RefreshTokens;
use crate::auth::SessionFactorProof;
use crate::auth::UserProvider;
use crate::auth::credential::CredentialFactorContext;
use crate::database::DatabaseTransaction;
use crate::database::Db;
use crate::database::sea_orm::DbBackend;
use crate::encryption::Encrypter;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

const PREFIX: &str = "openrust_factor_management_v1.";
const MAX_SECONDS: i64 = 300;
// context与Guard字段已有各自上限；此上限包含加密envelope的两层Base64膨胀。
const MAX_GRANT_BYTES: usize = 32 * 1024;

/// 显式JWT API管理入口的固定来源及密码确认扩展。
pub struct JwtApiFactorManagementOptions {
    /// 认证加密管理grant的应用加密器；使用独立AAD域，不产生第二套密码学。
    pub encrypter: Encrypter,
    /// 绑定的实际API Guard名称，不将原登录来源改写为Web身份。
    pub guard: String,
    /// 绑定的实际账户 `Provider` 名称；必须显式提供非空Some值。
    pub provider_name: Option<String>,
    /// 可选的官方密码确认回调；未配置时使用当前Provider验证真实密码。
    pub confirm_passwords: Option<ConfirmPasswordsUsing>,
}
impl std::fmt::Debug for JwtApiFactorManagementOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JwtApiFactorManagementOptions([REDACTED])")
    }
}

/// 当前整数users账户域的无 `Session` 因子管理适配。
///
/// `prepare`只接受当前有效access JWT及真实密码；返回值仅用于因子管理，不能访问资源。
/// 不支持UUID或其它账户表的完整管理闭环。
#[derive(Clone)]
pub struct JwtApiFactorManagement {
    authority: CredentialAuthority,
    actions: TwoFactorActions,
    tokens: JwtTokens,
    provider: Arc<dyn UserProvider>,
    encrypter: Encrypter,
    guard: String,
    provider_name: Option<String>,
    confirm_passwords: Option<ConfirmPasswordsUsing>,
}
impl std::fmt::Debug for JwtApiFactorManagement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JwtApiFactorManagement([REDACTED])")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApiManagementAuthorization {
    version: u8,
    user_id: i64,
    guard: String,
    provider: Option<String>,
    family_id: String,
    issuer: String,
    audience: String,
    context: CredentialFactorContext,
    issued_at: i64,
    expires_at: i64,
    nonce: String,
    password_hash_digest: String,
}
impl std::fmt::Debug for ApiManagementAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiManagementAuthorization([REDACTED])")
    }
}
impl ApiManagementAuthorization {
    fn validate(&self) -> Result<()> {
        let now = Utc::now().timestamp();
        if self.version != 1
            || self.user_id <= 0
            || !valid_name(&self.guard)
            || self.provider.as_ref().is_some_and(|name| !valid_name(name))
            || self.issuer.is_empty()
            || self.audience.is_empty()
            || !hex(&self.family_id, 32)
            || !hex(&self.nonce, 64)
            || !hex(&self.password_hash_digest, 64)
            || self.issued_at < 0
            || self.issued_at > now
            || self.expires_at <= now
            || self.expires_at <= self.issued_at
            || self
                .expires_at
                .checked_sub(self.issued_at)
                .is_none_or(|seconds| seconds > MAX_SECONDS)
        {
            return Err(Error::Unauthorized);
        }
        self.context.validate_for_user(self.user_id)?;
        let scope = self.context.scope.as_ref().ok_or(Error::Unauthorized)?;
        if scope.issuer != self.issuer || scope.audience != self.audience {
            return Err(Error::Unauthorized);
        }
        require_scope(&self.context, &self.guard, self.provider.as_deref())
    }
    pub(crate) fn initial_snapshot_matches(&self, source: &str, generation: &str) -> bool {
        self.context.source == source && self.context.generation == generation
    }
    pub(crate) async fn validate_in_transaction(
        &self,
        db: &Db,
        transaction: &DatabaseTransaction,
        user_id: i64,
    ) -> Result<()> {
        self.validate()?;
        if self.user_id != user_id {
            return Err(Error::Unauthorized);
        }
        let current = password_digest_in_transaction(db, transaction, user_id).await?;
        self.validate()?;
        if !bool::from(
            current
                .as_bytes()
                .ct_eq(self.password_hash_digest.as_bytes()),
        ) {
            return Err(Error::Unauthorized);
        }
        let deadline = RefreshTokens::new(db.clone())
            .validate_family_in_transaction(transaction, user_id, &self.family_id, &self.context)
            .await?;
        self.validate()?;
        if self.expires_at > deadline.timestamp() {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    fn binding(&self) -> Result<String> {
        Ok(digest(
            &serde_json::to_vec(&(
                PREFIX,
                &self.nonce,
                &self.family_id,
                &self.issuer,
                &self.audience,
                &self.guard,
                self.provider.as_deref(),
                &self.context.identifier,
            ))
            .map_err(Error::internal)?,
        ))
    }
    fn management_context(&self) -> Result<FactorManagementContext> {
        let proof = self.context.factor.as_ref().map(|factor| SessionFactorProof::from_value(json!({
            "identifier":self.context.identifier,"guard":self.guard,"provider":self.provider,
            "source":self.context.source,"generation":self.context.generation,
            "method":factor.method,"verified_at":factor.verified_at,
        }))).transpose()?;
        Ok(FactorManagementContext::issue(
            self.context.identifier.clone(),
            self.guard.clone(),
            self.provider.clone(),
            self.binding()?,
            proof,
        )?
        .with_api_authorization(self.clone()))
    }
}
impl JwtApiFactorManagement {
    /// 选择唯一authority、动作、JWT及实际Provider，拒绝其它账户表或数据域。
    pub fn new(
        authority: CredentialAuthority,
        actions: TwoFactorActions,
        tokens: JwtTokens,
        provider: Arc<dyn UserProvider>,
        options: JwtApiFactorManagementOptions,
    ) -> Result<Self> {
        if !valid_name(&options.guard)
            || options
                .provider_name
                .as_ref()
                .is_none_or(|name| !valid_name(name))
            || !provider.uses_database_accounts(authority.db(), "users")?
        {
            return Err(Error::Unauthorized);
        }
        Ok(Self {
            authority,
            actions,
            tokens,
            provider,
            encrypter: options.encrypter,
            guard: options.guard,
            provider_name: options.provider_name,
            confirm_passwords: options.confirm_passwords,
        })
    }
    async fn user(&self, user_id: i64) -> Result<AuthUser> {
        if user_id <= 0 {
            return Err(Error::Unauthorized);
        }
        let identifier = Value::from(user_id);
        let user = self
            .authority
            .db()
            .with_primary(self.provider.retrieve_by_id(&identifier))
            .await??
            .ok_or(Error::Unauthorized)?;
        if user.auth_identifier()? != identifier {
            return Err(Error::Unauthorized);
        }
        let authority = user
            .as_two_factor()
            .and_then(super::TwoFactorAuthenticatable::two_factor_authority)
            .ok_or(Error::Unauthorized)?;
        authority
            .validate_account(&user, self.provider.as_ref())
            .await?;
        Ok(user)
    }
    async fn confirmed_password_digest(&self, user_id: i64, password: &str) -> Result<String> {
        let user = self.user(user_id).await?;
        let password_hash_digest =
            digest(user.auth_password()?.ok_or(Error::Unauthorized)?.as_bytes());
        let valid = if let Some(callback) = &self.confirm_passwords {
            callback(user.clone(), Some(password.to_owned())).await?
        } else {
            self.provider
                .validate_credentials(
                    user.as_ref(),
                    &Credentials::new(Map::from_iter([("password".into(), Value::from(password))])),
                )
                .await?
        };
        if !valid {
            return Err(Error::Unauthorized);
        }
        Ok(password_hash_digest)
    }

    fn encrypt_grant(&self, authorization: &ApiManagementAuthorization) -> Result<String> {
        let encoded = Zeroizing::new(serde_json::to_vec(authorization).map_err(Error::internal)?);
        let encrypted = self
            .encrypter
            .encrypt_with_context(&encoded, PREFIX.as_bytes())?;
        let raw = format!("{PREFIX}{encrypted}");
        if raw.len() > MAX_GRANT_BYTES {
            return Err(Error::Unauthorized);
        }
        Ok(raw)
    }

    async fn validate_grant_before_commit(
        &self,
        transaction: &DatabaseTransaction,
        parent: &crate::auth::VerifiedCredential,
        authorization: &ApiManagementAuthorization,
    ) -> Result<()> {
        self.authority
            .validate_parent_in_transaction(transaction, parent)
            .await?;
        authorization
            .validate_in_transaction(self.authority.db(), transaction, parent.user_id)
            .await?;
        let now = Utc::now();
        if parent.prepared_at > now
            || now.signed_duration_since(parent.prepared_at).num_seconds() > 30
            || parent.expires_at.is_none_or(|expiry| expiry <= now)
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    /// 当前有效JWT与真实密码核验后，签发至多300秒且不超过原凭据截止的管理grant。
    pub async fn prepare(&self, access: &str, password: &str) -> Result<String> {
        let verified = self
            .authority
            .authenticate_access_token(&self.tokens, access)
            .await?;
        let verified = Arc::new(verified);
        let (family_id, issuer, audience) = match &verified.domain {
            CredentialDomain::Jwt {
                family_id,
                issuer,
                audience,
                ..
            } => (family_id.clone(), issuer.clone(), audience.clone()),
            _ => return Err(Error::Unauthorized),
        };
        let context = verified
            .context
            .as_ref()
            .ok_or(Error::Unauthorized)?
            .clone();
        require_scope(&context, &self.guard, self.provider_name.as_deref())?;
        let password_hash_digest = self
            .confirmed_password_digest(verified.user_id, password)
            .await?;
        let deadline = verified.expires_at.ok_or(Error::Unauthorized)?.timestamp();
        let adapter = self.clone();
        let final_adapter = self.clone();
        let final_parent = verified.clone();
        let checkpoint = Arc::new(std::sync::OnceLock::<ApiManagementAuthorization>::new());
        let captured = checkpoint.clone();
        self.authority
            .db()
            .transaction_for_update_checked(
                move |transaction| {
                    Box::pin(async move {
                        adapter
                            .authority
                            .validate_parent_in_transaction(transaction, &verified)
                            .await?;
                        let issued_at = Utc::now().timestamp();
                        let expires_at = issued_at
                            .checked_add(MAX_SECONDS)
                            .ok_or(Error::Unauthorized)?
                            .min(deadline);
                        let mut nonce = [0_u8; 32];
                        getrandom::fill(&mut nonce).map_err(Error::internal)?;
                        let authorization = ApiManagementAuthorization {
                            version: 1,
                            user_id: verified.user_id,
                            guard: adapter.guard.clone(),
                            provider: adapter.provider_name.clone(),
                            family_id,
                            issuer,
                            audience,
                            context,
                            issued_at,
                            expires_at,
                            nonce: encode_hex(&nonce),
                            password_hash_digest,
                        };
                        authorization
                            .validate_in_transaction(
                                adapter.authority.db(),
                                transaction,
                                verified.user_id,
                            )
                            .await?;
                        let raw = adapter.encrypt_grant(&authorization)?;
                        authorization.validate()?;
                        captured
                            .set(authorization)
                            .map_err(|_| Error::Unauthorized)?;
                        Ok(raw)
                    })
                },
                move |transaction| {
                    Box::pin(async move {
                        final_adapter
                            .validate_grant_before_commit(
                                transaction,
                                &final_parent,
                                checkpoint.get().ok_or(Error::Unauthorized)?,
                            )
                            .await
                    })
                },
            )
            .await
    }
    /// 重新开始未确认绑定；真实当前JWT和密码均需重核，旧绑定材料不会返回。
    ///
    /// 只允许Pending状态，生成新secret和独立管理continuation；不用于重置已启用因子。
    pub async fn restart_pending(&self, access: &str, password: &str) -> Result<String> {
        let raw = self.prepare(access, password).await?;
        let authorization = self.decode(&raw)?;
        let user = self.user(authorization.user_id).await?;
        let context = authorization
            .management_context()?
            .for_api_pending_restart();
        let actions = self
            .actions
            .with_management_context(context, self.actions.confirmation_required()?);
        actions.enable(&user, true).await?;
        Ok(raw)
    }

    /// 验证短期管理grant并返回受限动作；每次真实读写事务仍须复核撤销、密码和截止。
    pub async fn actions(&self, raw: &str) -> Result<(AuthUser, TwoFactorActions)> {
        let authorization = self.decode(raw)?;
        let user = self.user(authorization.user_id).await?;
        authorization.validate()?;
        let account = user
            .as_two_factor()
            .and_then(super::TwoFactorAuthenticatable::two_factor_authority)
            .ok_or(Error::Unauthorized)?;
        let snapshot = account.load().await?;
        authorization.validate()?;
        let confirm = self.actions.confirmation_required()?;
        let context = authorization.management_context()?;
        account
            .authorize_management(&snapshot, &context, confirm)
            .await?;
        authorization.validate()?;
        Ok((user, self.actions.with_management_context(context, confirm)))
    }
    fn decode(&self, raw: &str) -> Result<ApiManagementAuthorization> {
        if raw.len() > MAX_GRANT_BYTES {
            return Err(Error::Unauthorized);
        }
        let encrypted = raw.strip_prefix(PREFIX).ok_or(Error::Unauthorized)?;
        let plaintext = Zeroizing::new(
            self.encrypter
                .decrypt_with_context(encrypted, PREFIX.as_bytes())
                .map_err(|_| Error::Unauthorized)?,
        );
        let authorization: ApiManagementAuthorization =
            serde_json::from_slice(&plaintext).map_err(|_| Error::Unauthorized)?;
        authorization.validate()?;
        let (issuer, audience) = self.tokens.credential_domain();
        if authorization.issuer != issuer
            || authorization.audience != audience
            || authorization.guard != self.guard
            || authorization.provider != self.provider_name
        {
            return Err(Error::Unauthorized);
        }
        Ok(authorization)
    }
}
async fn password_digest_in_transaction(
    db: &Db,
    transaction: &DatabaseTransaction,
    user_id: i64,
) -> Result<String> {
    let query = db
        .table("users")?
        .using_transaction(transaction)
        .select(&["password_hash"])?
        .where_eq("id", user_id)?
        .where_null("disabled_at")?;
    let query = if db.backend()? == DbBackend::Sqlite {
        query
    } else {
        query.lock_for_update()?
    };
    let row = query.first().await?.ok_or(Error::Unauthorized)?;
    let password: Option<String> = row.try_get("", "password_hash").map_err(Error::internal)?;
    Ok(digest(password.ok_or(Error::Unauthorized)?.as_bytes()))
}
fn require_scope(
    context: &CredentialFactorContext,
    guard: &str,
    provider: Option<&str>,
) -> Result<()> {
    let scope = context.scope.as_ref().ok_or(Error::Unauthorized)?;
    if scope.guard != guard || Some(scope.provider.as_str()) != provider {
        return Err(Error::Unauthorized);
    }
    if context.factor.as_ref().is_some_and(|factor| !matches!(&factor.origin, FactorOrigin::Guard { guard: origin_guard, provider: origin_provider } if origin_guard == guard && origin_provider.as_deref() == provider)) { return Err(Error::Unauthorized); }
    Ok(())
}
fn valid_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn encode_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|byte| {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}
fn digest(value: &[u8]) -> String {
    encode_hex(&Sha256::digest(value))
}

#[cfg(all(
    test,
    any(
        feature = "db-sqlite",
        all(feature = "queue", any(feature = "db-mysql", feature = "db-postgres"))
    )
))]
mod tests;
