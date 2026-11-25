//! 本地账户创建与身份写入共用同一事务；绝不按邮箱自动关联已有账户。
use super::{Proof, identity};
use crate::{Features, Fortify, TwoFactorAuthenticatable};
use openrust::{
    auth::{AuthError, AuthResult},
    database::Transaction,
    http::{HttpError, HttpFuture},
    sanctum::HasApiTokens,
    support::facades::App,
};
use serde::{Deserialize, Serialize};

/// 经原生 Socialite 验证后交给应用的最小资料，不包含 OAuth 访问或刷新令牌。
#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderProfile {
    pub provider: String,
    pub subject: String,
    pub name: String,
    pub verified_email: Option<String>,
}
/// 应用创建动作仅能使用调用方的用户连接事务；可替换以适配自定义用户字段。
pub trait CreatesUsersFromProvider: Send + Sync {
    fn create<'a>(
        &'a self,
        profile: &'a ProviderProfile,
        transaction: &'a mut Transaction,
    ) -> HttpFuture<'a, Result<Box<dyn crate::contracts::User>, HttpError>>;
}

pub(super) fn profile(
    provider: &str,
    user: &openrust_socialite::two::User,
) -> AuthResult<ProviderProfile> {
    let subject = match user.get_id() {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) if n.is_u64() || n.is_i64() => n.to_string(),
        _ => return Err(AuthError::InvalidIdentity),
    };
    identity::key(provider, &subject)?;
    // 固定 Google 返回 verified_email，GitHub 原生驱动只选择 primary+verified 邮箱。
    // 其他驱动仍可登录已显式绑定的身份，创建账户须由应用动作明确提供可信邮箱策略。
    let verified = match provider {
        "google" => {
            user.get_raw()
                .get("verified_email")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
                || user
                    .get_raw()
                    .get("email_verified")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
        }
        "github" => true,
        _ => false,
    };
    let email = verified
        .then(|| user.get_email().map(str::to_owned))
        .flatten();
    let name = user
        .get_name()
        .or_else(|| user.get_nickname())
        .unwrap_or(provider)
        .to_owned();
    Ok(ProviderProfile {
        provider: provider.into(),
        subject,
        name,
        verified_email: email,
    })
}

pub(super) async fn authenticate<M>(
    profile: &ProviderProfile,
    fortify: &Fortify,
) -> AuthResult<(M, Proof)>
where
    M: HasApiTokens + TwoFactorAuthenticatable + Clone,
{
    let model = M::from_state(Default::default());
    let connection = model.get_connection()?;
    if let Some(record) = identity::find(&connection, &profile.provider, &profile.subject).await? {
        let mut query = M::query_on(&connection).use_write_pdo();
        query.r#where(
            model.get_key_name(),
            "=",
            openrust::database::Value::from(record.user.clone()),
        );
        let physical = query.first().await?.ok_or(HttpError::Unauthenticated)?;
        let found = super::super::client::provider(fortify)?
            .retrieve_by_id(&physical.get_auth_identifier())
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let user = (found.as_ref() as &dyn std::any::Any)
            .downcast_ref::<M>()
            .cloned()
            .ok_or(HttpError::Unauthenticated)?;
        if !record.belongs_to(&user)? {
            return Err(HttpError::Unauthenticated.into());
        }
        return Ok((user, Proof::from_identity(&record)?));
    }
    if !Features::new(fortify.config.clone()).enabled(Features::registration())? {
        return Err(super::super::client::failed(
            "provider",
            "user.provider_registration_disabled",
        )
        .into());
    }
    let action = App::make::<dyn CreatesUsersFromProvider>()
        .await
        .map_err(|e| AuthError::Backend(Box::new(e)))?;
    let mut transaction = connection.begin_transaction().await?;
    let created = action.create(profile, &mut transaction).await?;
    let mut user = (created.as_ref() as &dyn std::any::Any)
        .downcast_ref::<M>()
        .cloned()
        .ok_or(AuthError::Configuration(
            "第三方创建Action与用户套件模型不一致",
        ))?;
    if !transaction.belongs_to(&user.get_connection()?) || !user.state().exists() {
        return Err(AuthError::Configuration(
            "第三方创建Action未在选定用户事务中保存账户",
        ));
    }
    // 同一事务生成初始凭据状态；不能在锁内调用另取连接的refresh_created。
    super::super::credentials::stamp(&user)?;
    let identity =
        identity::link(&user, &mut transaction, &profile.provider, &profile.subject).await?;
    user.refresh_for_update(&mut transaction).await?;
    transaction.commit().await?;
    crate::actions::emit(
        openrust::auth::events::Registered {
            user: std::sync::Arc::new(user.clone()),
        },
        &[],
    )
    .await
    .map_err(|e| AuthError::Backend(Box::new(e)))?;
    Ok((user, Proof::from_identity(&identity)?))
}
