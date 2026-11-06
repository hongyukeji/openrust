//! Cookie 服务生命周期；可变队列始终归请求作用域所有。
use std::sync::Arc;

use super::{CookieFactory, CookieJar, crypto::Protection};
#[cfg(not(feature = "encryption"))]
use crate::Error;
use crate::Result;
use crate::core::{BootContext, BoxFuture, RegisterContext, ServiceProvider};

/// 登记 Cookie 创建器和请求隔离队列。
///
/// `new` 要求加密服务；仅明确选择 `plain` 才会发送未保护的 Cookie。
#[derive(Clone)]
pub struct CookieServiceProvider {
    factory: Option<CookieFactory>,
    plain: bool,
    except: Vec<String>,
}
impl Default for CookieServiceProvider {
    fn default() -> Self {
        Self::new()
    }
}
impl CookieServiceProvider {
    /// 默认要求已注册的 Encryptor，缺失时启动失败。
    pub fn new() -> Self {
        Self {
            factory: None,
            plain: false,
            except: Vec::new(),
        }
    }
    /// 显式使用明文 Cookie；不提供 Laravel web 组的默认保护。
    pub fn plain() -> Self {
        Self {
            plain: true,
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
#[derive(Clone, Default)]
pub(crate) struct CookieRuntime(pub(crate) Option<super::middleware::CookieLayer>);
impl ServiceProvider for CookieServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> crate::core::BoxFuture<'a, Result<()>> {
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
            Ok(())
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
            let protection = if self.plain {
                Protection::Plain
            } else {
                #[cfg(feature = "encryption")]
                {
                    Protection::Encrypted(
                        (*app.resolve::<crate::security::Encryptor>().await?).clone(),
                    )
                }
                #[cfg(not(feature = "encryption"))]
                {
                    return Err(Error::custom(
                        "cookie_encryption_unavailable",
                        "Cookie 保护要求 encryption feature 和 Encryptor",
                    ));
                }
            };
            app.extension_mut::<CookieRuntime>()?.0 = Some(super::middleware::CookieLayer {
                protection,
                except: self.except.clone(),
            });
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
