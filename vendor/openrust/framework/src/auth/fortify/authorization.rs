//! 已选择统一策略的资源授权；不将普通登录或remember恢复当作因子验证。

use crate::{Error, Result, auth::Guard};

/// 按当前主库因子状态核验同一Session证明，不要求每个请求重复输入验证码。
///
/// 本增强显式要求同源数据库authority；JWT/PAT等无Session收据的来源不能借同值ID通过。
pub async fn authorize_session_factor(
    guard: &dyn Guard,
    guard_name: &str,
    confirm: bool,
) -> Result<()> {
    let user = guard.user().await?.ok_or(Error::Unauthorized)?;
    let stateful = guard.as_stateful().ok_or(Error::Unauthorized)?;
    let authority = user
        .as_two_factor()
        .and_then(super::TwoFactorAuthenticatable::two_factor_authority)
        .ok_or_else(|| {
            Error::custom_with_status(
                http::StatusCode::FORBIDDEN,
                "factor_authority_required",
                "当前账户未接入所选统一因子策略",
            )
        })?;
    authority
        .validate_account(&user, stateful.provider().as_ref())
        .await?;
    let snapshot = authority.load().await?;
    if !snapshot.challenge_required(confirm) {
        return Ok(());
    }
    let proof = stateful
        .session_factor_proof()
        .await?
        .ok_or(Error::Unauthorized)?;
    if !proof.matches(
        &user.auth_identifier()?,
        guard_name,
        guard.provider_name(),
        &snapshot.source,
        &snapshot.generation,
    ) {
        return Err(Error::Unauthorized);
    }
    Ok(())
}
