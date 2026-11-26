//! 接收端人工重放的授权与调度入口。
use crate::{DatabaseWebhookStore, Error, Result, WebhookStore as _};
use openrust::{
    container::Container,
    contracts::{container::Container as _, foundation::Application as _},
    http::HttpFuture,
    support::facades::{App, Facade as _},
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

const POLICY: &str = "webhook-client.replay-policy";

#[derive(Clone, Debug, serde::Serialize)]
pub struct ExhaustedReceipt {
    pub id: String,
    pub scope: String,
    pub profile: String,
    pub revision: String,
    pub replay_generation: u32,
}

/// 只向应用授权器传递接收记录身份和操作审计字段。
pub struct ReplayRequest {
    pub receipt: ExhaustedReceipt,
    pub generation: u32,
    pub actor: String,
    pub reason: String,
}

pub struct ReplayLimits {
    pub max_generations: u32,
    pub lifetime_seconds: u32,
}

pub trait ReplayPolicy: Send + Sync {
    /// 应用负责验证可信操作身份、范围和当前配置／凭据修订许可。
    fn authorize<'a>(
        &'a self,
        request: &'a ReplayRequest,
    ) -> HttpFuture<'a, Result<Option<ReplayLimits>>>;
}

struct Policy(Arc<dyn ReplayPolicy>);

pub fn register_policy(container: &Container, policy: Arc<dyn ReplayPolicy>) -> Result<()> {
    container.instance_named(POLICY, Arc::new(Policy(policy)))?;
    Ok(())
}

async fn check(request: &ReplayRequest) -> Result<ReplayLimits> {
    let app = App::get_facade_root()?;
    let policy = app
        .container()
        .get::<Policy>(POLICY)
        .map_err(|_| Error::ReplayDenied)?;
    let limits = policy
        .0
        .authorize(request)
        .await?
        .ok_or(Error::ReplayDenied)?;
    if !(1..=10).contains(&limits.max_generations)
        || !(60..=86_400).contains(&limits.lifetime_seconds)
        || request.generation > limits.max_generations
    {
        return Err(Error::ReplayDenied);
    }
    Ok(limits)
}

/// 从仍保留原始受信任接收记录的耗尽意图创建一次有界重放。
pub async fn request(
    store: &DatabaseWebhookStore,
    id: &str,
    actor: &str,
    reason: &str,
) -> Result<u32> {
    for (value, limit) in [(actor, 128), (reason, 512)] {
        if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
            return Err(Error::InvalidConfig("replay.actor/reason"));
        }
    }
    let receipt = store
        .exhausted_receipt(id)
        .await?
        .ok_or(Error::ReplayUnavailable)?;
    let generation = receipt.replay_generation + 1;
    let request = ReplayRequest {
        receipt,
        generation,
        actor: actor.into(),
        reason: reason.into(),
    };
    let limits = check(&request).await?;
    // 复用正文读取及附件摘要校验，历史记录仍须具备完整的有效载荷。
    store.find(id).await?.ok_or(Error::ReplayUnavailable)?;
    store
        .schedule_replay(&request, limits.lifetime_seconds)
        .await?;
    Ok(generation)
}

pub(crate) async fn authorize_generation(
    store: &DatabaseWebhookStore,
    id: &str,
    generation: u32,
) -> Result<Option<SystemTime>> {
    if generation == 0 {
        return Ok(None);
    }
    let (request, expires_at) = store
        .active_replay(id, generation)
        .await?
        .ok_or(Error::ReplayDenied)?;
    check(&request).await?;
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(expires_at))
        .ok_or(Error::InvalidRecord)
        .map(Some)
}
