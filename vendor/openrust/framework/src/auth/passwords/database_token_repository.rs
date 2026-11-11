use std::{sync::Arc, time::Duration};

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::{
    auth::{AuthError, AuthFuture, AuthResult},
    contracts::{auth::CanResetPassword, hashing::Hasher},
    database::{
        Connection, Value,
        query::{Builder, Record},
    },
};

use super::{
    TokenClaim, TokenRepository,
    token_policy::{TokenPolicy, now},
};

/// Stores a single hashed password reset token per email address.
pub struct DatabaseTokenRepository {
    database: Connection,
    table: String,
    policy: TokenPolicy,
}

impl DatabaseTokenRepository {
    pub fn new(
        database: Connection,
        table: impl Into<String>,
        app_key: &str,
        hasher: Arc<dyn Hasher>,
        expires: Duration,
        throttle: Duration,
    ) -> AuthResult<Self> {
        Ok(Self {
            database,
            table: table.into(),
            policy: TokenPolicy::new(app_key, hasher, expires, throttle)?,
        })
    }

    fn query(&self, email: &str) -> Builder<'static> {
        self.database
            .table(&self.table)
            .useWritePdo()
            .r#where("email", "=", email)
    }

    async fn record(&self, email: &str) -> AuthResult<Option<(String, DateTime<Utc>)>> {
        let Some(row) = self
            .query(email)
            .first()
            .await
            .map_err(AuthError::backend)?
        else {
            return Ok(None);
        };
        let (Some(Value::Text(hash)), Some(Value::Text(created))) =
            (row.get("token"), row.get("created_at"))
        else {
            return Ok(None);
        };
        let created = NaiveDateTime::parse_from_str(created, "%Y-%m-%d %H:%M:%S%.f")
            .map_err(AuthError::backend)?;
        Ok(Some((hash.clone(), created.and_utc())))
    }
}

impl TokenRepository for DatabaseTokenRepository {
    fn create<'a>(&'a self, user: &'a dyn CanResetPassword) -> AuthFuture<'a, AuthResult<String>> {
        Box::pin(async move {
            let (token, hash) = self.policy.generate().await?;
            let record: Record = [
                ("email".into(), user.getEmailForPasswordReset().into()),
                ("token".into(), hash.into()),
                ("created_at".into(), timestamp(now())),
            ]
            .into();
            self.database
                .table(&self.table)
                .upsert(&[record], &["email"], Some(&["token", "created_at"]))
                .await
                .map_err(AuthError::backend)?;
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
            let Some(_) = self.policy.throttle_cutoff(now()) else {
                return self.create(user).await.map(Some);
            };
            let (token, hash) = self.policy.generate().await?;
            let now = now();
            let cutoff = self.policy.throttle_cutoff(now).expect("throttle enabled");
            let email = user.getEmailForPasswordReset();
            let values: Record = [
                ("token".into(), hash.into()),
                ("created_at".into(), timestamp(now)),
            ]
            .into();
            let updated = self
                .query(email)
                .whereNested(|query| {
                    query
                        .r#where("created_at", "<=", timestamp(cutoff))
                        .orWhereNull("created_at")
                })
                .update(values.clone())
                .await
                .map_err(AuthError::backend)?;
            if updated != 0 {
                return Ok(Some(token));
            }
            let mut values = values;
            values.insert("email".into(), email.into());
            let inserted = self
                .database
                .table(&self.table)
                .insertOrIgnore(&[values])
                .await
                .map_err(AuthError::backend)?;
            Ok((inserted == 1).then_some(token))
        })
    }

    fn claim<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<TokenClaim>>> {
        Box::pin(async move {
            let email = user.getEmailForPasswordReset();
            let Some((hash, created)) = self.record(email).await? else {
                return Ok(None);
            };
            if hash.starts_with('!')
                || created <= self.policy.expiry_cutoff(now())
                || !self.policy.matches(token, &hash).await?
            {
                return Ok(None);
            }
            let changed = self
                .query(email)
                .r#where("token", "=", hash.as_str())
                .r#where(
                    "created_at",
                    ">",
                    timestamp(self.policy.expiry_cutoff(now())),
                )
                .update([("token".into(), format!("!{hash}").into())].into())
                .await
                .map_err(AuthError::backend)?;
            Ok((changed == 1).then_some(TokenClaim { hash }))
        })
    }

    fn finish<'a>(
        &'a self,
        email: &'a str,
        claim: TokenClaim,
        success: bool,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            let query = self
                .query(email)
                .r#where("token", "=", format!("!{}", claim.hash));
            if success {
                query.delete().await.map_err(AuthError::backend)?;
            } else {
                query
                    .update([("token".into(), claim.hash.into())].into())
                    .await
                    .map_err(AuthError::backend)?;
            }
            Ok(())
        })
    }

    fn exists<'a>(
        &'a self,
        user: &'a dyn CanResetPassword,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async move {
            let Some((hash, created)) = self.record(user.getEmailForPasswordReset()).await? else {
                return Ok(false);
            };
            if hash.starts_with('!') || created <= self.policy.expiry_cutoff(now()) {
                return Ok(false);
            }
            self.policy.matches(token, &hash).await
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
                .record(user.getEmailForPasswordReset())
                .await?
                .is_some_and(|(_, created)| created > cutoff))
        })
    }

    fn delete<'a>(&'a self, user: &'a dyn CanResetPassword) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            self.query(user.getEmailForPasswordReset())
                .delete()
                .await
                .map_err(AuthError::backend)?;
            Ok(())
        })
    }

    fn deleteExpired(&self) -> AuthFuture<'_, AuthResult<()>> {
        Box::pin(async move {
            self.database
                .table(&self.table)
                .r#where(
                    "created_at",
                    "<",
                    timestamp(self.policy.expiry_cutoff(now())),
                )
                .delete()
                .await
                .map_err(AuthError::backend)?;
            Ok(())
        })
    }
}

fn timestamp(value: DateTime<Utc>) -> Value {
    value.format("%Y-%m-%d %H:%M:%S").to_string().into()
}
