//! 自定义短信供应商的完整 Provider 示例。
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    notifications::{
        NotificationError, NotificationFuture, NotificationResult,
        sms::{SmsDriver, SmsManager, SmsMessage},
    },
    support::{ProviderResult, ServiceProvider, facades::Http},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub struct ExampleSmsProvider;
impl ServiceProvider for ExampleSmsProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<SmsManager>()?
                .extend("example", |profile| {
                    let endpoint = profile["endpoint"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| failure("短信端点未配置"))?;
                    Ok(Arc::new(ExampleSms {
                        endpoint: endpoint.into(),
                    }))
                });
            Ok(())
        })
    }
}

struct ExampleSms {
    endpoint: String,
}
impl SmsDriver for ExampleSms {
    fn send(&self, message: SmsMessage) -> NotificationFuture<'_, NotificationResult<Value>> {
        Box::pin(async move {
            if message.approved_template().is_some() {
                return Err(failure("示例驱动仅支持纯文本"));
            }
            let response = Http::accept_json()
                .map_err(|_| failure("HTTP 客户端不可用"))?
                .without_redirecting()
                .timeout(Duration::from_secs(5))
                .max_response_bytes(16 * 1024)
                .post(
                    &self.endpoint,
                    &json!({"to":message.to(), "text":message.content()}),
                )
                .await
                .map_err(|_| failure("短信供应商连接失败"))?;
            if !response.successful() {
                return Err(failure("短信供应商未接受请求"));
            }
            let receipt: Value = response.json().map_err(|_| failure("短信回执无效"))?;
            if receipt["accepted"] != true {
                return Err(failure("短信供应商拒绝请求"));
            }
            Ok(receipt)
        })
    }
}
fn failure(message: &'static str) -> NotificationError {
    NotificationError::Delivery {
        source: message.into(),
    }
}
