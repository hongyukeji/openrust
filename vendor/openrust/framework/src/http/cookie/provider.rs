//! Cookie 服务生命周期；可变队列始终归请求作用域所有。
use std::sync::Arc;

use super::{CookieFactory, CookieJar, middleware::EncryptCookies};
use crate::Result;
use crate::foundation::BootContext;
use crate::foundation::RegisterContext;
use crate::support::{BoxFuture, ServiceProvider};

/// 登记 Cookie 创建器和请求隔离队列。
///
/// `new` 要求加密服务；仅明确选择 `plain` 才会发送未保护的 Cookie。
#[derive(Clone)]
pub struct CookieServiceProvider {
    factory: Option<CookieFactory>,
    middleware: EncryptCookies,
}
impl Default for CookieServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}
impl CookieServiceProvider {
    /// 使用名称绑定保护 Cookie，首次受保护请求时解析应用密钥。
    pub fn new() -> Self {
        Self {
            factory: None,
            middleware: EncryptCookies::new(),
        }
    }
    /// 显式使用明文 Cookie，适用于允许发送未加密 Cookie 的路由。
    pub fn plain() -> Self {
        Self {
            middleware: EncryptCookies::plain(),
            ..Self::new()
        }
    }
    /// 使用上下文认证及独立密钥环保护 Cookie。
    ///
    /// 此模式读取 `security.encryption`，使用独立的上下文密文格式。
    pub fn contextual() -> Self {
        Self {
            middleware: EncryptCookies::contextual(),
            ..Self::new()
        }
    }
    /// 使用应用提供的不可变创建默认值。
    #[must_use]
    pub fn with_factory(mut self, factory: CookieFactory) -> Self {
        self.factory = Some(factory);
        self
    }
    /// 使用应用提供的 Cookie 加密中间件配置。
    ///
    /// Web 组与 `cookies` 路由别名使用同一应用实例。
    #[must_use]
    pub fn with_middleware(mut self, middleware: EncryptCookies) -> Self {
        self.middleware = middleware;
        self
    }

    /// 替换当前全部例外，入站匹配根键，出站匹配字面名称。
    #[must_use]
    pub fn except(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.middleware.replace_exceptions(names);
        self
    }
}

impl ServiceProvider for CookieServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.middleware.validate()?;
            let explicit = self.factory.clone();
            let repository = app.config_repository();
            app.singleton::<CookieFactory, _>(move |_| {
                let explicit = explicit.clone();
                let repository = repository.clone();
                Box::pin(async move {
                    let factory = match explicit {
                        Some(factory) => factory,
                        None => factory_from_repository(&repository)?,
                    };
                    Ok(Arc::new(factory))
                })
            })
            .await?;
            app.require::<CookieFactory>();
            app.scoped::<CookieJar, _>(|resolver| {
                Box::pin(async move {
                    let factory = resolver.resolve::<CookieFactory>().await?;
                    Ok(Arc::new(CookieJar::new((*factory).clone())))
                })
            })
            .await?;
            app.instance(Arc::new(self.middleware.clone())).await
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.factory.is_none() {
                let repository = app.config_repository();
                app.validate_at_finalize(move || {
                    Box::pin(async move { factory_from_repository(&repository).map(|_| ()) })
                });
            }
            Ok(())
        })
    }
}

// 与 Session 读取同一仓储，HTTP 不依赖可选 auth 模块或其服务是否登记。
fn factory_from_repository(
    repository: &crate::config::repository::ConfigRepository,
) -> Result<CookieFactory> {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct Defaults {
        path: Option<String>,
        domain: Option<String>,
        secure: Option<bool>,
        same_site: Option<String>,
    }
    let mut factory = CookieFactory::default();
    if repository.has("session")? {
        let config: Defaults = repository.section("session")?;
        let defaults = crate::config::SessionConfig {
            path: config.path.unwrap_or_else(|| "/".to_owned()),
            domain: config.domain,
            secure: config.secure,
            same_site: config.same_site,
            ..crate::config::SessionConfig::default()
        };
        let same_site = defaults.validate_cookie_defaults()?;
        factory.set_default_path_and_domain(
            defaults.path,
            defaults.domain,
            defaults.secure,
            same_site,
        )?;
    }
    Ok(factory)
}
