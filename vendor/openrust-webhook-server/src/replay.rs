use std::sync::Arc;

use openrust::{
    container::Container,
    contracts::container::Container as _,
    contracts::foundation::Application as _,
    http::HttpFuture,
    support::facades::{App, Facade as _},
};

use crate::{DeliveryService, Error, Result, extensions, snapshot::Snapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayPhase {
    Request,
    Attempt,
}

pub struct ReplayRequest<'a> {
    pub phase: ReplayPhase,
    pub scope: &'a str,
    pub original_delivery_id: &'a str,
    pub message_id: &'a str,
    pub url: &'a str,
    pub original_credential: Option<&'a str>,
    pub generation: u32,
    pub actor: &'a str,
    pub reason: &'a str,
    pub source_succeeded: bool,
}

pub struct ReplayLimits {
    pub max_generations: u32,
    pub max_attempts: u32,
    pub lifetime_seconds: u32,
    pub allow_succeeded: bool,
    pub credential_reference: Option<String>,
}

pub trait ReplayPolicy: Send + Sync {
    /// 应用负责验证操作者、端点与当前凭据的授权。
    fn authorize<'a>(
        &'a self,
        request: ReplayRequest<'a>,
    ) -> HttpFuture<'a, Result<Option<ReplayLimits>>>;
}

pub(crate) struct RegisteredReplayPolicy(Arc<dyn ReplayPolicy>);

pub fn register_replay_policy(container: &Container, policy: Arc<dyn ReplayPolicy>) -> Result<()> {
    container
        .instance(Arc::new(RegisteredReplayPolicy(policy)))
        .map(|_| ())
        .map_err(|_| Error::ReplayDenied)
}

#[derive(serde::Serialize)]
pub struct ReplayDelivery {
    pub original_delivery_id: String,
    pub delivery_id: String,
    pub generation: u32,
}

pub(crate) struct ReplayRecord {
    pub original_id: String,
    pub generation: u32,
    pub actor: String,
    pub reason: String,
    pub source_state: String,
    pub original_credential: Option<String>,
}

impl ReplayRecord {
    fn request<'a>(
        &'a self,
        scope: &'a str,
        snapshot: &'a Snapshot,
        phase: ReplayPhase,
    ) -> ReplayRequest<'a> {
        ReplayRequest {
            phase,
            scope,
            original_delivery_id: &self.original_id,
            message_id: &snapshot.uuid,
            url: &snapshot.url,
            original_credential: self.original_credential.as_deref(),
            generation: self.generation,
            actor: &self.actor,
            reason: &self.reason,
            source_succeeded: self.source_state == "succeeded",
        }
    }
}

impl DeliveryService {
    /// 登记新的重放投递；应用通常由 Scheduler 将它交给 Worker。
    pub async fn replay(
        &self,
        original_id: &str,
        actor: &str,
        reason: &str,
        include_succeeded: bool,
    ) -> Result<ReplayDelivery> {
        for (value, maximum) in [(actor, 128), (reason, 512)] {
            if value.trim().is_empty()
                || value.len() > maximum
                || value.chars().any(char::is_control)
            {
                return Err(Error::InvalidConfig("replay.actor/reason"));
            }
        }
        let (mut snapshot, state) = self.store.replay_source(original_id).await?;
        if state != "failed" && !(state == "succeeded" && include_succeeded) {
            return Err(Error::ReplayUnavailable);
        }
        let generation = self.store.replay_generation(original_id).await?;
        let record = ReplayRecord {
            original_id: original_id.into(),
            generation,
            actor: actor.into(),
            reason: reason.into(),
            source_state: state,
            original_credential: snapshot.credential.clone(),
        };
        let limits =
            authorize(record.request(self.store.scope(), &snapshot, ReplayPhase::Request)).await?;
        snapshot.credential = credential(&record, &limits)?;
        snapshot.tries = limits.max_attempts;
        snapshot.lifetime = limits.lifetime_seconds;
        snapshot.replay = true;

        let delivery_id = crate::delivery_store::replay_id(original_id, generation);
        self.check_replay_destination(&delivery_id, &snapshot)
            .await?;
        self.store
            .persist_replay(&delivery_id, &record, &snapshot)
            .await?;
        Ok(ReplayDelivery {
            original_delivery_id: original_id.into(),
            delivery_id,
            generation,
        })
    }
}

pub(crate) async fn authorize_attempt(
    scope: &str,
    record: &ReplayRecord,
    snapshot: &Snapshot,
) -> Result<()> {
    let limits = authorize(record.request(scope, snapshot, ReplayPhase::Attempt)).await?;
    if snapshot.credential != credential(record, &limits)?
        || snapshot.tries > limits.max_attempts
        || snapshot.lifetime > limits.lifetime_seconds
    {
        return Err(Error::ReplayDenied);
    }
    Ok(())
}

async fn authorize(request: ReplayRequest<'_>) -> Result<ReplayLimits> {
    let policy = App::get_facade_root()
        .map_err(|_| Error::ReplayDenied)?
        .container()
        .make::<RegisteredReplayPolicy>()
        .map_err(|_| Error::ReplayDenied)?;
    let generation = request.generation;
    let source_succeeded = request.source_succeeded;
    let limits = policy
        .0
        .authorize(request)
        .await
        .map_err(|_| Error::ReplayDenied)?
        .ok_or(Error::ReplayDenied)?;
    if !(1..=10).contains(&generation)
        || !(generation..=10).contains(&limits.max_generations)
        || !(1..=10).contains(&limits.max_attempts)
        || !(1..=86_400).contains(&limits.lifetime_seconds)
        || source_succeeded && !limits.allow_succeeded
    {
        return Err(Error::ReplayDenied);
    }
    Ok(limits)
}

fn credential(record: &ReplayRecord, limits: &ReplayLimits) -> Result<Option<String>> {
    if let Some(new) = &limits.credential_reference {
        if record.original_credential.is_none()
            || extensions::validate_name("replay.credential", new).is_err()
        {
            return Err(Error::ReplayDenied);
        }
        return Ok(Some(new.clone()));
    }
    Ok(record.original_credential.clone())
}
