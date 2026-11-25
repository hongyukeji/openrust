use openrust::queue::{DispatchResult, QueueManager};

use crate::{DeliveryProgress, DeliveryService, Error, Result, TargetPolicy, WebhookCall};

pub struct QueuedDelivery {
    pub delivery_id: String,
    pub dispatch: DispatchResult,
}

pub struct SynchronousDelivery {
    pub delivery_id: String,
    pub progress: DeliveryProgress,
}

impl WebhookCall {
    pub async fn dispatch(
        self,
        service: &DeliveryService,
        queue: &QueueManager,
        target: &TargetPolicy,
        key: &str,
        lifetime: u32,
    ) -> Result<QueuedDelivery> {
        let prepared = self.prepare(target)?;
        service.queue_route(queue, prepared.job(), prepared.connection())?;
        let delivery_id = service.store.submit(key, &prepared, lifetime).await?;
        let dispatch = service.dispatch(queue, &delivery_id).await?;
        Ok(QueuedDelivery {
            delivery_id,
            dispatch,
        })
    }

    pub async fn dispatch_if(
        self,
        condition: bool,
        service: &DeliveryService,
        queue: &QueueManager,
        target: &TargetPolicy,
        key: &str,
        lifetime: u32,
    ) -> Result<Option<QueuedDelivery>> {
        if condition {
            self.dispatch(service, queue, target, key, lifetime)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    pub async fn dispatch_unless(
        self,
        condition: bool,
        service: &DeliveryService,
        queue: &QueueManager,
        target: &TargetPolicy,
        key: &str,
        lifetime: u32,
    ) -> Result<Option<QueuedDelivery>> {
        self.dispatch_if(!condition, service, queue, target, key, lifetime)
            .await
    }

    pub async fn dispatch_sync(
        self,
        service: &DeliveryService,
        target: &TargetPolicy,
        key: &str,
        lifetime: u32,
    ) -> Result<SynchronousDelivery> {
        let prepared = self.prepare(target)?;
        let delivery_id = service.store.submit(key, &prepared, lifetime).await?;
        let (progress, throw) = service.run(&delivery_id).await?;
        if throw && progress.is_final_failure() {
            return Err(Error::SynchronousDeliveryFailed { delivery_id });
        }
        Ok(SynchronousDelivery {
            delivery_id,
            progress,
        })
    }

    pub async fn dispatch_sync_if(
        self,
        condition: bool,
        service: &DeliveryService,
        target: &TargetPolicy,
        key: &str,
        lifetime: u32,
    ) -> Result<Option<SynchronousDelivery>> {
        if condition {
            self.dispatch_sync(service, target, key, lifetime)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    pub async fn dispatch_sync_unless(
        self,
        condition: bool,
        service: &DeliveryService,
        target: &TargetPolicy,
        key: &str,
        lifetime: u32,
    ) -> Result<Option<SynchronousDelivery>> {
        self.dispatch_sync_if(!condition, service, target, key, lifetime)
            .await
    }
}
