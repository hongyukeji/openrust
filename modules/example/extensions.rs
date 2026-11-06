//! 模块自定义缓存 driver 与显式测试通信替换。
//!
//! 缓存 Provider 可由模块 descriptor 挂载；捕获驱动只经测试的 register 后覆盖使用，
//! 不改变生产通信 Provider 的配置 schema，也不会向外发送消息。

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use openrust::BoxFuture;
use openrust::Error;
use openrust::Result;
use openrust::cache::Cache;
use openrust::cache::CacheBackend;
use openrust::cache::CacheDriver;
use openrust::cache::CacheDriverConfig;
use openrust::cache::CacheScope;
#[cfg(test)]
use openrust::config::Environment;
#[cfg(test)]
use openrust::container::Container;
#[cfg(test)]
use openrust::container::Resolver;
use openrust::foundation::BootContext;
#[cfg(test)]
use openrust::mail::MailDriver;
#[cfg(test)]
use openrust::mail::Mailer;
#[cfg(test)]
use openrust::mail::lettre::Message;
#[cfg(test)]
use openrust::sms::SmsDriver;
#[cfg(test)]
use openrust::sms::SmsMessage;
#[cfg(test)]
use openrust::sms::SmsReceipt;
#[cfg(test)]
use openrust::sms::SmsSender;
use openrust::support::ServiceProvider;
use tokio::sync::Mutex;

/// 注册业务缓存 driver；仅配置选择 `example-prefix` 的 store 才会实例化它。
pub struct PrefixCacheServiceProvider;
impl ServiceProvider for PrefixCacheServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.resolve::<Cache>().await?.extend(PrefixDriver)?;
            // 显式同名配置（包括 Redis）归应用所有，不被示例默认覆盖。
            let config = app.section::<openrust::config::CacheConfig>("cache")?;
            if !config.stores.contains_key("example-prefix") {
                let store = openrust::config::CacheStoreConfig {
                    driver: "example-prefix".into(),
                    namespace: "example-articles".into(),
                    options: [("key_prefix".into(), serde_json::Value::from("article"))].into(),
                };
                app.overlay(
                    "cache",
                    serde_json::json!({"stores":{"example-prefix":store}}),
                )?;
            }
            Ok(())
        })
    }
}

struct PrefixDriver;

impl CacheDriver for PrefixDriver {
    fn name(&self) -> &'static str {
        "example-prefix"
    }

    fn validate(&self, config: &CacheDriverConfig) -> Result<()> {
        for name in config.option_names() {
            if name != "key_prefix" {
                return Err(Error::custom(
                    "example_cache_option_invalid",
                    format!("cache.stores.{}.{name} 不受支持", config.store_name()),
                ));
            }
        }
        let prefix = config
            .option::<String>("key_prefix")?
            .unwrap_or_else(|| config.store_name().to_owned());
        if prefix.trim().is_empty() {
            return Err(Error::custom(
                "example_cache_prefix_invalid",
                format!("cache.stores.{}.key_prefix 不能为空", config.store_name()),
            ));
        }
        Ok(())
    }

    fn build<'a>(
        &'a self,
        config: &'a CacheDriverConfig,
    ) -> BoxFuture<'a, Result<Arc<dyn CacheBackend>>> {
        Box::pin(async move {
            self.validate(config)?;
            let prefix = config
                .option::<String>("key_prefix")?
                .unwrap_or_else(|| config.store_name().to_owned());
            Ok(Arc::new(PrefixBackend {
                prefix,
                values: Mutex::new(HashMap::new()),
            }) as Arc<dyn CacheBackend>)
        })
    }
}

struct CacheEntry {
    value: Vec<u8>,
    expires_at: Option<Instant>,
}

struct PrefixBackend {
    prefix: String,
    values: Mutex<HashMap<String, CacheEntry>>,
}

impl PrefixBackend {
    fn key(&self, key: &str) -> String {
        format!("{}:{key}", self.prefix)
    }
}

#[async_trait]
impl CacheBackend for PrefixBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut values = self.values.lock().await;
        // 惰性清理所有过期条目，避免一次性业务键在后续访问时持续占用内存。
        let now = Instant::now();
        values.retain(|_, entry| entry.expires_at.is_none_or(|expires| expires > now));
        Ok(values.get(&self.key(key)).map(|entry| entry.value.clone()))
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        let expires_at = ttl
            .map(|duration| {
                Instant::now().checked_add(duration).ok_or_else(|| {
                    Error::custom("example_cache_ttl_invalid", "缓存 TTL 超出时钟范围")
                })
            })
            .transpose()?;
        self.values
            .lock()
            .await
            .insert(self.key(key), CacheEntry { value, expires_at });
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.values.lock().await.remove(&self.key(key));
        Ok(())
    }

    fn scope(&self) -> CacheScope {
        CacheScope::Process
    }
}

/// 本地捕获结果，不实现 Debug，避免日志意外输出收件人和正文。
#[cfg(test)]
#[derive(Clone, Default)]
pub struct CapturedDeliveries {
    mail: Arc<Mutex<Vec<Message>>>,
    sms: Arc<Mutex<Vec<SmsMessage>>>,
}

#[cfg(test)]
impl CapturedDeliveries {
    /// 取走已捕获邮件；内容仅留在当前进程，不会真实发送。
    pub async fn take_mail(&self) -> Vec<Message> {
        std::mem::take(&mut *self.mail.lock().await)
    }

    /// 取走已捕获短信；内容仅留在当前进程，不会真实发送。
    pub async fn take_sms(&self) -> Vec<SmsMessage> {
        std::mem::take(&mut *self.sms.lock().await)
    }

    /// 在 `TestApp` 的 register 后覆盖阶段显式安装捕获 driver。
    ///
    /// 标准 Mailer/SMS Provider 保留各自 schema；只有测试主动调用才替换绑定。
    /// 工厂及实际发送时都校验环境，生产环境拒绝捕获。
    pub async fn install(&self, bindings: &Container) -> Result<()> {
        let captured = self.clone();
        bindings
            .singleton::<Mailer, _>(move |resolver| {
                let captured = captured.clone();
                Box::pin(async move {
                    ensure_capture_allowed(&resolver)?;
                    Ok(Arc::new(Mailer::new(CaptureMailDriver {
                        captured,
                        resolver,
                    })))
                })
            })
            .await?;
        let captured = self.clone();
        bindings
            .singleton::<SmsSender, _>(move |resolver| {
                let captured = captured.clone();
                Box::pin(async move {
                    ensure_capture_allowed(&resolver)?;
                    Ok(Arc::new(SmsSender::new(CaptureSmsDriver {
                        captured,
                        resolver,
                    })))
                })
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
fn ensure_capture_allowed(resolver: &Resolver) -> Result<()> {
    if resolver
        .section::<Environment>("app.env")?
        .enforces_production_safety()
    {
        return Err(Error::custom(
            "example_capture_production_forbidden",
            "production 禁止本地捕获通信驱动",
        ));
    }
    Ok(())
}

#[cfg(test)]
struct CaptureMailDriver {
    captured: CapturedDeliveries,
    resolver: Resolver,
}

#[cfg(test)]
impl MailDriver for CaptureMailDriver {
    fn send(&self, message: Message) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            ensure_capture_allowed(&self.resolver)?;
            self.captured.mail.lock().await.push(message);
            Ok(())
        })
    }
}

#[cfg(test)]
struct CaptureSmsDriver {
    captured: CapturedDeliveries,
    resolver: Resolver,
}

#[cfg(test)]
impl SmsDriver for CaptureSmsDriver {
    fn send(&self, message: SmsMessage) -> BoxFuture<'_, Result<SmsReceipt>> {
        Box::pin(async move {
            ensure_capture_allowed(&self.resolver)?;
            self.captured.sms.lock().await.push(message);
            Ok(SmsReceipt {
                provider: "example-capture".into(),
                message_id: None,
            })
        })
    }
}
