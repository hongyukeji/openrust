//! Cookie 服务生命周期；可变队列始终归请求作用域所有。
use std::sync::Arc;

use super::{CookieFactory, CookieJar, crypto::Protection, middleware::CookieLayer};
#[cfg(not(feature = "encryption"))]
use crate::Error;
use crate::Result;
use crate::container::Scope;
use crate::foundation::BootContext;
use crate::foundation::RegisterContext;
use crate::support::{BoxFuture, ServiceProvider};

#[derive(Clone, Copy)]
enum ProtectionMode {
    Plain,
    Contextual,
    NameBound,
}

/// 登记 Cookie 创建器和请求隔离队列。
///
/// `new` 要求加密服务；仅明确选择 `plain` 才会发送未保护的 Cookie。
#[derive(Clone)]
pub struct CookieServiceProvider {
    factory: Option<CookieFactory>,
    mode: ProtectionMode,
    except: Vec<String>,
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
            mode: ProtectionMode::NameBound,
            except: Vec::new(),
        }
    }
    /// 显式使用明文 Cookie，适用于允许发送未加密 Cookie 的路由。
    pub fn plain() -> Self {
        Self {
            mode: ProtectionMode::Plain,
            ..Self::new()
        }
    }
    /// 使用上下文认证及独立密钥环保护 Cookie。
    ///
    /// 此模式读取 `security.encryption`，使用独立的上下文密文格式。
    pub fn contextual() -> Self {
        Self {
            mode: ProtectionMode::Contextual,
            ..Self::new()
        }
    }
    /// 使用应用提供的不可变创建默认值。
    #[must_use]
    pub fn with_factory(mut self, factory: CookieFactory) -> Self {
        self.factory = Some(factory);
        self
    }
    /// 按各阶段的实际名称精确排除：入站为解析后的根键，出站为 Cookie 名称字面值。
    #[must_use]
    pub fn except(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.except = names.into_iter().map(Into::into).collect();
        self
    }
}
#[derive(Clone)]
pub(crate) struct CookieLayerPlan {
    mode: ProtectionMode,
    except: Vec<String>,
}

impl From<&CookieServiceProvider> for CookieLayerPlan {
    fn from(provider: &CookieServiceProvider) -> Self {
        Self {
            mode: provider.mode,
            except: provider.except.clone(),
        }
    }
}

impl CookieLayerPlan {
    #[cfg(feature = "session")]
    pub(crate) fn is_protected(&self) -> bool {
        !matches!(self.mode, ProtectionMode::Plain)
    }

    #[cfg(feature = "session")]
    pub(crate) fn protects_cookie(&self, name: &str) -> bool {
        self.is_protected() && !self.except.iter().any(|except| except == name)
    }

    #[cfg(feature = "encryption")]
    pub(crate) async fn resolve(&self, scope: &Scope) -> Result<Arc<CookieLayer>> {
        let protection = match self.mode {
            ProtectionMode::Plain => Protection::Plain,
            ProtectionMode::Contextual => {
                let encrypter = scope
                    .resolve::<crate::encryption::ContextualEncrypter>()
                    .await?;
                Protection::Encrypted((*encrypter).clone())
            }
            ProtectionMode::NameBound => Protection::NameBound(
                scope
                    .resolve::<dyn crate::encryption::ApplicationEncrypter>()
                    .await?,
            ),
        };
        Ok(self.layer(protection))
    }

    #[cfg(not(feature = "encryption"))]
    pub(crate) fn resolve(&self, _scope: &Scope) -> std::future::Ready<Result<Arc<CookieLayer>>> {
        std::future::ready(if matches!(self.mode, ProtectionMode::Plain) {
            Ok(self.layer(Protection::Plain))
        } else {
            Err(Error::custom(
                "cookie_encryption_unavailable",
                "Cookie 保护要求 encryption feature 和 Encryptor",
            ))
        })
    }

    fn layer(&self, protection: Protection) -> Arc<CookieLayer> {
        Arc::new(CookieLayer {
            protection,
            except: self.except.clone(),
        })
    }
}

impl ServiceProvider for CookieServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if self.except.len() > 1024
                || self.except.iter().map(String::len).sum::<usize>() > 64 * 1024
            {
                return Err(super::cookie_limits_error());
            }
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
            app.instance(Arc::new(CookieLayerPlan::from(self))).await
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
