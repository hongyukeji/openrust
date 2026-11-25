use std::{sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::{
    contracts::events::Event,
    support::facades::{Event as Events, Log},
};
use serde_json::Value;

use crate::{DeliveryProgress, Error, Result, snapshot::Snapshot};

/// 进程内事件载荷。通过显式访问器读取正文及元数据。
/// 事件队列和诊断上下文不自动序列化该载荷。
pub struct WebhookCallEvent {
    pub scope: String,
    pub delivery_id: String,
    pub attempt: Option<WebhookAttempt>,
    pub(crate) snapshot: Snapshot,
}

pub struct WebhookAttempt {
    pub number: u32,
    pub status: Option<u16>,
    pub outcome: &'static str,
    pub progress: DeliveryProgress,
    pub elapsed: Duration,
}

impl WebhookCallEvent {
    pub fn message_id(&self) -> &str {
        &self.snapshot.uuid
    }

    pub fn method(&self) -> &str {
        &self.snapshot.method
    }

    /// 保留来源和路径，查询串可能含凭据。
    pub fn url(&self) -> &str {
        self.snapshot
            .url
            .split_once('?')
            .map_or(&self.snapshot.url, |(url, _)| url)
    }

    /// 返回签名所覆盖的正文。GET 仍可通过此入口读取生成查询的原始 JSON。
    pub fn body(&self) -> Result<Vec<u8>> {
        STANDARD
            .decode(&self.snapshot.body)
            .map_err(|_| Error::InvalidRecord)
    }

    /// 请求头仅提供准备阶段的名称，签名及其他请求头的值由传输自行处理。
    pub fn header_names(&self) -> impl Iterator<Item = &str> {
        self.snapshot.headers.iter().map(|(name, _)| name.as_str())
    }

    pub fn meta(&self) -> &Value {
        &self.snapshot.meta
    }

    pub fn tags(&self) -> &[String] {
        &self.snapshot.tags
    }

    pub(crate) fn new(
        scope: &str,
        id: &str,
        snapshot: Snapshot,
        attempt: Option<WebhookAttempt>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scope: scope.into(),
            delivery_id: id.into(),
            attempt,
            snapshot,
        })
    }
}

macro_rules! delivery_event {
    ($name:ident) => {
        pub struct $name(pub Arc<WebhookCallEvent>);
        impl Event for $name {
            fn name(&self) -> &str {
                concat!("openrust_webhook_server::events::", stringify!($name))
            }
        }
    };
}

delivery_event!(DispatchingWebhookCallEvent);
delivery_event!(WebhookCallSucceededEvent);
delivery_event!(WebhookCallFailedEvent);
delivery_event!(FinalWebhookCallFailedEvent);

pub(crate) async fn dispatching(scope: &str, id: &str, snapshot: Snapshot) -> Result<()> {
    let event = WebhookCallEvent::new(scope, id, snapshot, None);
    Events::dispatch(DispatchingWebhookCallEvent(event))
        .await
        .map(|_| ())
        .map_err(|_| Error::DispatchEventFailed {
            delivery_id: id.into(),
        })
}

pub(crate) async fn attempted(event: Arc<WebhookCallEvent>, progress: DeliveryProgress) {
    if progress == DeliveryProgress::Succeeded {
        publish(WebhookCallSucceededEvent(event)).await;
    } else {
        publish(WebhookCallFailedEvent(event.clone())).await;
        if progress.is_final_failure() {
            publish(FinalWebhookCallFailedEvent(event)).await;
        }
    }
}

async fn publish(event: impl Event) {
    if Events::dispatch(event).await.is_err() {
        let _ = Log::warning("Webhook 投递结果监听器失败", &Default::default());
    }
}
