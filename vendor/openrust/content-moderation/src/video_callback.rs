use openrust_webhook_client::{Error, Result, jobs::WebhookProcessingOutcome, models::WebhookCall};

use crate::{
    ModerationError, ModerationResult, ModerationStore, ReviewState, TaskId,
    credentials::valid_identifier, storage::ProviderTask,
};

pub(crate) struct CallbackConfirmation {
    store: ModerationStore,
    driver: &'static str,
    account: String,
    route_profile: String,
    route_revision: String,
    webhook_scope: String,
}

impl CallbackConfirmation {
    pub(crate) fn new(
        store: ModerationStore,
        driver: &'static str,
        account: &str,
        webhook_scope: &str,
        route_profile: &str,
        route_revision: &str,
    ) -> ModerationResult<Self> {
        if ![account, route_profile, route_revision]
            .into_iter()
            .all(valid_identifier)
            || webhook_scope.is_empty()
            || webhook_scope.len() > 128
            || webhook_scope.chars().any(char::is_control)
        {
            return Err(ModerationError::InvalidConfiguration(
                "moderation callback scope",
            ));
        }
        Ok(Self {
            store,
            driver,
            account: account.to_owned(),
            route_profile: route_profile.to_owned(),
            route_revision: route_revision.to_owned(),
            webhook_scope: webhook_scope.to_owned(),
        })
    }

    pub(crate) fn accepts(&self, call: &WebhookCall) -> bool {
        call.scope() == self.webhook_scope
            && call.name() == self.route_profile
            && call.revision() == self.route_revision
    }

    pub(crate) async fn confirm(
        &self,
        receipt: &str,
        task: TaskId,
    ) -> Result<WebhookProcessingOutcome> {
        use WebhookProcessingOutcome::{Completed, Deferred, PermanentFailure};
        let review = match self
            .store
            .find_provider_task(self.driver, &self.account, &task)
            .await
            .map_err(storage_error)?
        {
            ProviderTask::Missing => return Ok(Deferred { delay_seconds: 10 }),
            ProviderTask::Ambiguous => return Ok(PermanentFailure),
            ProviderTask::Found(review) => review,
        };
        if review.state() != ReviewState::Running
            || review.confirmation_callback_id().is_some()
            || self
                .store
                .request_confirmation(&review, receipt)
                .await
                .map_err(storage_error)?
        {
            return Ok(Completed);
        }
        // 领取或终态写入可能在匹配后先胜出，再读取一次后决定是否延期。
        let current = self.store.find(review.id()).await.map_err(storage_error)?;
        if current.is_none_or(|review| {
            review.state() != ReviewState::Running || review.confirmation_callback_id().is_some()
        }) {
            Ok(Completed)
        } else {
            Ok(Deferred { delay_seconds: 10 })
        }
    }
}

fn storage_error(source: ModerationError) -> Error {
    Error::Runtime {
        source: Box::new(source),
    }
}
