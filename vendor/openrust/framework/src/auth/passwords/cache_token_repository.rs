use std::{fmt::Write as _, sync::Arc, time::Duration};

use aws_lc_rs::digest;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AuthError, AuthFuture, AuthResult},
    cache::{Repository, Ttl, Value},
    contracts::{
        auth::CanResetPassword,
        cache::{LockProvider, Repository as _},
        hashing::Hasher,
    },
};

use super::{
    TokenClaim, TokenRepository,
    token_policy::{TokenPolicy, now},
};

/// Stores hashed reset tokens in an application cache with a bounded lifetime.
pub struct CacheTokenRepository {
    store: Arc<Repository>,
    policy: TokenPolicy,
}

#[derive(Deserialize, Serialize)]
struct Record {
    hash: String,
    created: DateTime<Utc>,
}

impl CacheTokenRepository {
    pub fn new(
        store: Arc<Repository>,
        app_key: &str,
        hasher: Arc<dyn Hasher>,
        expires: Duration,
        throttle: Duration,
    ) -> AuthResult<Self> {
        Ok(Self {
            store,
            policy: TokenPolicy::new(app_key, hasher, expires, throttle)?,
        })
    }

    async fn locked<T>(
        &self,
        key: &str,
        operation: impl Future<Output = AuthResult<T>>,
    ) -> AuthResult<T> {
        let mut lock = self
            .store
            .lock(&format!("{key}:lock"), Some(Duration::from_secs(30)), None)
            .map_err(AuthError::backend)?;
        lock.block(Duration::from_secs(3))
            .await
            .map_err(AuthError::backend)?;
        let result = operation.await;
        let released = lock.release().await.map_err(AuthError::backend);
        let value = result?;
        released?;
        Ok(value)
    }

    async fn put(&self, key: &str, record: Record) -> AuthResult<()> {
        let expiry = record.created + chrono::TimeDelta::seconds(self.policy.lifetime());
        let value = serde_json::to_value(record).map_err(AuthError::backend)?;
        let stored = self
            .store
            .put(key, &Value::Json(value), Some(Ttl::Until(expiry.into())))
            .await
            .map_err(AuthError::backend)?;
        if !stored {
            return Err(AuthError::Configuration("auth.passwords.cache"));
        }
        Ok(())
    }

    async fn record(&self, key: &str) -> AuthResult<Option<Record>> {
        match self.store.get(key).await.map_err(AuthError::backend)? {
            Some(Value::Json(value)) => serde_json::from_value(value)
                .map(Some)
                .map_err(AuthError::backend),
            _ => Ok(None),
        }
    }
}

impl TokenRepository for CacheTokenRepository {
    fn create<'a>(&'a self, user: &'a dyn CanResetPassword) -> AuthFuture<'a, AuthResult<String>> {
        Box::pin(async move {
            let (token, hash) = self.policy.generate().await?;
            let key = cache_key(user);
            self.locked(
                &key,
                self.put(
                    &key,
                    Record {
                        hash,
                        created: now(),
                    },
                ),
            )
            .await?;
            Ok(token)
        })
    }

    fn issue<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
    ) -> AuthFuture<'a, AuthResult<Option<String>>> {
        Box::pin(async move {
            if self.recentlyCreatedToken(user).await? {
                return Ok(None);
            }
            let (token, hash) = self.policy.generate().await?;
            let key = cache_key(user);
            self.locked(&key, async {
                if self.recentlyCreatedToken(user).await? {
                    return Ok(None);
                }
                self.put(
                    &key,
                    Record {
                        hash,
                        created: now(),
                    },
                )
                .await?;
                Ok(Some(token))
            })
            .await
        })
    }

    fn claim<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<TokenClaim>>> {
        Box::pin(async move {
            let key = cache_key(user);
            let Some(record) = self.record(&key).await? else {
                return Ok(None);
            };
            if record.hash.starts_with('!')
                || record.created <= self.policy.expiry_cutoff(now())
                || !self.policy.matches(token, &record.hash).await?
            {
                return Ok(None);
            }
            self.locked(&key, async {
                let Some(current) = self.record(&key).await? else {
                    return Ok(None);
                };
                if current.hash != record.hash
                    || current.created <= self.policy.expiry_cutoff(now())
                {
                    return Ok(None);
                }
                self.put(
                    &key,
                    Record {
                        hash: format!("!{}", record.hash),
                        created: current.created,
                    },
                )
                .await?;
                Ok(Some(TokenClaim { hash: record.hash }))
            })
            .await
        })
    }

    fn finish<'a>(
        &'a self,
        email: &'a str,
        claim: TokenClaim,
        success: bool,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            let key = cache_key_for(email);
            self.locked(&key, async {
                let Some(record) = self.record(&key).await? else {
                    return Ok(());
                };
                if record.hash.strip_prefix('!') != Some(claim.hash.as_str()) {
                    return Ok(());
                }
                if success || record.created <= self.policy.expiry_cutoff(now()) {
                    self.store.forget(&key).await.map_err(AuthError::backend)?;
                } else {
                    self.put(
                        &key,
                        Record {
                            hash: claim.hash,
                            created: record.created,
                        },
                    )
                    .await?;
                }
                Ok(())
            })
            .await
        })
    }

    fn exists<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async move {
            let Some(record) = self.record(&cache_key(user)).await? else {
                return Ok(false);
            };
            if record.hash.starts_with('!') || record.created <= self.policy.expiry_cutoff(now()) {
                return Ok(false);
            }
            self.policy.matches(token, &record.hash).await
        })
    }

    fn recentlyCreatedToken<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async move {
            let Some(cutoff) = self.policy.throttle_cutoff(now()) else {
                return Ok(false);
            };
            Ok(self
                .record(&cache_key(user))
                .await?
                .is_some_and(|record| record.created > cutoff))
        })
    }

    fn delete<'a>(&'a self, user: &'a dyn CanResetPassword) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            let key = cache_key(user);
            self.locked(&key, async {
                self.store.forget(&key).await.map_err(AuthError::backend)?;
                Ok(())
            })
            .await
        })
    }

    fn deleteExpired(&self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn cache_key(user: &dyn CanResetPassword) -> String {
    cache_key_for(user.getEmailForPasswordReset())
}

fn cache_key_for(email: &str) -> String {
    let hash = digest::digest(&digest::SHA256, email.as_bytes());
    let mut key = String::from("password-reset:");
    for byte in hash.as_ref() {
        write!(key, "{byte:02x}").expect("writing to a String");
    }
    key
}
