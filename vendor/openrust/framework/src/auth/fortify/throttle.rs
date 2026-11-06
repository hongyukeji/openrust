//! Fortify失败计数与具名请求限流，统一使用框架RateLimiter。

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, RwLock},
    time::Duration,
};

use super::{Fortify, FortifyConfig};
use crate::{
    Error, Result,
    core::RateLimiter,
    http::{Request, axum::http::StatusCode},
};
use sha2::{Digest, Sha256};

/// 具名策略返回的单个请求限制，key调试输出脱敏。
#[derive(Clone)]
pub struct FortifyLimit {
    /// 窗口内可接受的请求数。
    pub attempts: u64,
    /// 首次命中后窗口期限。
    pub decay: Duration,
    key: String,
}
impl fmt::Debug for FortifyLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FortifyLimit")
            .field("attempts", &self.attempts)
            .field("decay", &self.decay)
            .field("key", &"[REDACTED]")
            .finish()
    }
}
impl FortifyLimit {
    /// 创建分钟窗口策略；依赖的RateLimiter负责key与期限校验。
    pub fn per_minute(attempts: u64, key: impl Into<String>) -> Self {
        Self {
            attempts,
            decay: Duration::from_secs(60),
            key: key.into(),
        }
    }
}

type LimitFactory = Arc<dyn Fn(&Fortify, &Request) -> Result<FortifyLimit> + Send + Sync>;
/// 对应安装Provider的具名限流策略登记；计数仍走唯一框架RateLimiter。
#[derive(Clone)]
pub struct FortifyRateLimiters(Arc<RwLock<BTreeMap<String, LimitFactory>>>);
impl fmt::Debug for FortifyRateLimiters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FortifyRateLimiters([REDACTED])")
    }
}
impl Default for FortifyRateLimiters {
    fn default() -> Self {
        let mut factories = BTreeMap::<String, LimitFactory>::new();
        factories.insert(
            "login".into(),
            Arc::new(|fortify, request| {
                Ok(FortifyLimit::per_minute(
                    5,
                    login_key(&fortify.config, request)?,
                ))
            }),
        );
        factories.insert(
            "two-factor".into(),
            Arc::new(|_, request| {
                let id = request.session()?.get_json("login.id")?;
                let id = match id {
                    None | Some(serde_json::Value::Null) => String::new(),
                    Some(serde_json::Value::String(id)) => id,
                    Some(serde_json::Value::Number(id)) => id.to_string(),
                    Some(_) => {
                        return Err(Error::custom(
                            "fortify_challenge_identifier_invalid",
                            "双因素挑战账户ID类型无效",
                        ));
                    }
                };
                Ok(FortifyLimit::per_minute(5, id))
            }),
        );
        Self(Arc::new(RwLock::new(factories)))
    }
}
impl FortifyRateLimiters {
    /// 创建安装默认的login与two-factor具名策略，不强制包回退启用它们。
    pub fn new() -> Self {
        Self::default()
    }
    /// 登记或替换策略；回调在请求中求值，不缓存用户身份。
    pub fn for_limiter(
        &self,
        name: impl Into<String>,
        factory: impl Fn(&Fortify, &Request) -> Result<FortifyLimit> + Send + Sync + 'static,
    ) -> Result<&Self> {
        let name = name.into();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(Error::custom(
                "fortify_limiter_name_invalid",
                "具名限流器名称无效",
            ));
        }
        self.0
            .write()
            .map_err(|_| registry_error())?
            .insert(name, Arc::new(factory));
        Ok(self)
    }
    fn resolve(&self, name: &str, fortify: &Fortify, request: &Request) -> Result<FortifyLimit> {
        let factory = self
            .0
            .read()
            .map_err(|_| registry_error())?
            .get(name)
            .cloned()
            .ok_or_else(|| {
                Error::custom_with_status(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "fortify_limiter_not_defined",
                    "Fortify具名限流器未定义",
                )
            })?;
        factory(fortify, request)
    }
}
fn registry_error() -> Error {
    Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "fortify_limiter_registry_poisoned",
        "Fortify限流策略不可用",
    )
}

pub(super) fn login_key(config: &FortifyConfig, request: &Request) -> Result<String> {
    let username = match request.input(&config.username)? {
        Some(serde_json::Value::String(value)) => value,
        Some(serde_json::Value::Number(value)) => value.to_string(),
        None | Some(serde_json::Value::Null) => String::new(),
        Some(_) => return Err(Error::BadRequest("登录账号必须为字符串".into())),
    };
    let key = format!(
        "{}|{}",
        username.to_lowercase(),
        request
            .client_ip()?
            .map(|ip| ip.to_string())
            .unwrap_or_default()
    );
    crate::support::Str::transliterate(&key)
}
async fn named(fortify: &Fortify, request: &Request, name: &str) -> Result<()> {
    let registry = fortify.resolver.resolve::<FortifyRateLimiters>().await?;
    let policy = registry.resolve(name, fortify, request)?;
    let limiter = fortify.resolver.resolve::<RateLimiter>().await?;
    let key = counter_key("fortify.named", &format!("{name}:{}", policy.key));
    if limiter.too_many_attempts(&key, policy.attempts).await? {
        return Err(Error::rate_limited(limiter.available_in(&key).await?));
    }
    limiter.hit(&key, policy.decay).await?;
    Ok(())
}

pub(super) async fn before_login(
    fortify: &Fortify,
    request: &Request,
) -> Result<Option<crate::http::axum::response::Response>> {
    if let Some(name) = &fortify.config.limiters.login {
        named(fortify, request, name).await?;
        return Ok(None);
    }
    let limiter = fortify.resolver.resolve::<RateLimiter>().await?;
    let key = counter_key("fortify.login", &login_key(&fortify.config, request)?);
    if limiter.too_many_attempts(&key, 5).await? {
        fortify
            .actions
            .events
            .dispatch(&Lockout {
                request: request.clone(),
            })
            .await?;
        return Ok(Some(
            fortify
                .resolver
                .resolve::<dyn super::LockoutResponse>()
                .await?
                .to_response(request)
                .await?,
        ));
    }
    Ok(None)
}
pub(super) async fn login_failed(fortify: &Fortify, request: &Request) -> Result<()> {
    let limiter = fortify.resolver.resolve::<RateLimiter>().await?;
    limiter
        .hit(
            &counter_key("fortify.login", &login_key(&fortify.config, request)?),
            Duration::from_secs(60),
        )
        .await?;
    Ok(())
}
pub(super) async fn login_succeeded(fortify: &Fortify, request: &Request) -> Result<()> {
    let limiter = fortify.resolver.resolve::<RateLimiter>().await?;
    limiter
        .clear(&counter_key(
            "fortify.login",
            &login_key(&fortify.config, request)?,
        ))
        .await
}
pub(super) async fn before_challenge(fortify: &Fortify, request: &Request) -> Result<()> {
    if let Some(name) = &fortify.config.limiters.two_factor {
        return named(fortify, request, name).await;
    }
    Ok(())
}

/// 认证失败次数触线事件；原始请求仅供受信监听器显式读取，Debug不泄露输入。
#[derive(Clone)]
pub struct Lockout {
    /// 触发锁定的同一规范化请求。
    pub request: Request,
}
impl fmt::Debug for Lockout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Lockout([REDACTED])")
    }
}

// 逻辑bucket身份保留，进入有界通用存储前假名化，避免长度限制改变合法用户名流程。
pub(super) fn counter_key(scope: &str, logical_key: &str) -> String {
    format!("{scope}.{:x}", Sha256::digest(logical_key.as_bytes()))
}
