use crate::{container::ServiceKey, support::facades::Facade};

use super::{NotificationResult, SmsManager, SmsSender, SmsSubmission, delivery};

/// Resolves SMS drivers for the current application.
pub struct Sms;

#[allow(non_snake_case, reason = "Implements the application facade contract")]
impl Facade for Sms {
    type Service = SmsManager;

    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<SmsManager>()
    }
}

impl Sms {
    #[allow(non_snake_case, reason = "Matches Laravel Manager")]
    pub fn getDefaultDriver() -> NotificationResult<String> {
        Self::getFacadeRoot().map_err(delivery)?.getDefaultDriver()
    }

    #[allow(non_snake_case, reason = "Matches Laravel Manager")]
    pub fn forgetDrivers() -> NotificationResult<()> {
        Self::getFacadeRoot().map_err(delivery)?.forgetDrivers();
        Ok(())
    }

    pub fn driver(name: &str) -> NotificationResult<SmsSender> {
        Self::getFacadeRoot().map_err(delivery)?.driver(name)
    }

    pub fn default_driver() -> NotificationResult<SmsSender> {
        Self::getFacadeRoot().map_err(delivery)?.default_driver()
    }

    pub async fn send_text(
        to: impl Into<String>,
        content: impl Into<String>,
    ) -> NotificationResult<SmsSubmission> {
        Self::default_driver()?.send_text(to, content).await
    }

    pub async fn send_template<K: Into<String>, V: Into<String>>(
        to: impl Into<String>,
        name: impl Into<String>,
        parameters: impl IntoIterator<Item = (K, V)>,
    ) -> NotificationResult<SmsSubmission> {
        Self::default_driver()?
            .send_template(to, name, parameters)
            .await
    }
}
