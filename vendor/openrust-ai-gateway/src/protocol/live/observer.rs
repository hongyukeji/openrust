use std::{sync::Arc, time::Duration};

use futures_util::StreamExt;
use openrust::{
    foundation::{Runner, RunnerContext, RuntimeResult},
    http::{HttpFuture, client::WebSocketMessage},
    support::Str,
};
use tokio::{
    task::JoinSet,
    time::{Instant, timeout},
};

use super::{
    Control, Entry, Gateway, LiveCall, Outcome, RENEW_INTERVAL, SOCKET_TIMEOUT, Target, UsageStore,
    close_upstream, id_for, is_gone, now, target_scope, terminal_event, terminate_upstream,
};

const POLL: Duration = Duration::from_millis(250);
const MAX_OBSERVERS: usize = 32;

pub(crate) struct LiveObserver(pub(crate) Arc<Gateway>);

impl Runner for LiveObserver {
    fn signature(&self) -> &str {
        "ai-gateway-live"
    }

    fn run(&self, context: RunnerContext) -> HttpFuture<'_, RuntimeResult<()>> {
        Box::pin(async move {
            let Some(store) = &self.0.usage_store else {
                return Ok(());
            };
            let mut observers = JoinSet::new();
            let mut scan = tokio::time::interval(Duration::from_secs(1));
            scan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    () = context.cancelled() => break,
                    _ = observers.join_next(), if !observers.is_empty() => {}
                    _ = scan.tick(), if observers.len() < MAX_OBSERVERS => {
                        let limit = (MAX_OBSERVERS - observers.len()) as u64;
                        let Ok(Ok(calls)) = timeout(SOCKET_TIMEOUT, store.live_observation_candidates(limit)).await else {
                            continue;
                        };
                        for call in calls {
                            let store = store.clone();
                            let gateway = self.0.clone();
                            let context = context.clone();
                            observers.spawn(observe(gateway, store, call, context));
                        }
                    }
                }
            }
            while observers.join_next().await.is_some() {}
            Ok(())
        })
    }
}

async fn observe(
    gateway: Arc<Gateway>,
    store: UsageStore,
    call: LiveCall,
    shutdown: RunnerContext,
) {
    let controller = Str::ulid().to_string();
    if !matches!(
        timeout(
            SOCKET_TIMEOUT,
            store.claim_live_observer(&call, &controller)
        )
        .await,
        Ok(Ok(true))
    ) {
        return;
    }
    let control = Control {
        call,
        controller,
        store,
    };
    let result = observe_claimed(&gateway, &control, shutdown).await;
    if result.is_none() {
        let _ = control
            .store
            .delay_live_observer(&control.call, &control.controller)
            .await;
    }
    control.finish(result).await;
}

async fn observe_claimed(
    gateway: &Gateway,
    control: &Control,
    shutdown: RunnerContext,
) -> Option<&'static str> {
    let call = &control.call;
    if call.upstream_id.is_none() {
        return (now() >= call.expires_at).then_some("session_unconfirmed");
    }
    let adapter = original_target(gateway, call)?.provider.live_gateway()?;
    let id = id_for(call);
    if now() >= call.expires_at {
        return terminate_upstream(adapter, id, None)
            .await
            .then_some("session_expired");
    }
    let connected = tokio::select! {
        () = shutdown.cancelled() => return None,
        connected = timeout(SOCKET_TIMEOUT, adapter.connect_live_sideband(id)) => connected,
    };
    let mut socket = match connected {
        Ok(Ok(socket)) => socket,
        Ok(Err(error)) if is_gone(&error) => return Some("session_ended"),
        _ => return None,
    };

    let seconds = call.expires_at.saturating_sub(now()).max(0) as u64;
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let mut poll = tokio::time::interval(POLL);
    let mut renew = tokio::time::interval_at(Instant::now() + RENEW_INTERVAL, RENEW_INTERVAL);
    let outcome = loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => break Outcome::Disconnected,
            () = &mut deadline => break Outcome::Expired,
            _ = renew.tick() => {
                if !matches!(timeout(SOCKET_TIMEOUT, control.store.renew_live_control(call, &control.controller)).await, Ok(Ok(true))) {
                    break Outcome::Disconnected;
                }
            }
            _ = poll.tick() => {
                if !matches!(timeout(POLL, control.store.live_observer_should_yield(call, &control.controller)).await, Ok(Ok(false))) {
                    break Outcome::Disconnected;
                }
            }
            incoming = socket.next() => match incoming {
                Some(Ok(WebSocketMessage::Text(text))) if terminal_event(text.as_str()) => break Outcome::Ended,
                Some(Ok(WebSocketMessage::Close(_))) | Some(Err(_)) | None => break Outcome::Disconnected,
                Some(Ok(_)) => {}
            }
        }
    };
    let terminal = match outcome {
        Outcome::Ended => Some("session_ended"),
        Outcome::Expired => terminate_upstream(adapter, id, Some(&mut socket))
            .await
            .then_some("session_expired"),
        Outcome::Disconnected => None,
    };
    close_upstream(&mut socket).await;
    // 先释放上游连接，再让下一个进程获得控制权。
    drop(socket);
    terminal
}

fn original_target<'a>(gateway: &'a Gateway, call: &LiveCall) -> Option<&'a Target> {
    gateway
        .models
        .get(&call.model)?
        .live_targets()
        .find(|target| {
            let Some(adapter) = target.provider.live_gateway() else {
                return false;
            };
            [Entry::Versioned, Entry::Codex].into_iter().any(|entry| {
                entry.permits(adapter)
                    && target_scope(entry, target, adapter).as_ref() == Some(&call.scope)
            })
        })
}
