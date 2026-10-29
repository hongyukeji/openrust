//! Auth与通用Session复用同一状态、store和撤销机制。

#[cfg(feature = "session-redis")]
pub use crate::session::RedisSessionStore;
pub(crate) use crate::session::SessionSettings;
pub use crate::session::{DatabaseSessionStore, SessionServiceProvider, SessionStoreHandle};

impl SessionStoreHandle {
    /// 比较请求快照与当前持久身份，不续期；返回截止不晚于持久列或TTL。
    pub async fn authorized_until(
        &self,
        session: &crate::session::Session,
        user_id: i64,
    ) -> crate::Result<std::time::SystemTime> {
        use crate::Error;
        if session.get_typed::<i64>(crate::auth::database::AUTH_USER_ID_KEY)? != Some(user_id) {
            return Err(Error::Unauthorized);
        }
        let handler = session.handler()?;
        if handler.guarantees() != crate::session::SessionStoreGuarantees::ServerSide {
            return Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "session_store_guarantee_unsupported",
                "Session 驱动无法提供服务器端撤销保证",
            ));
        }
        let id = session.id()?;
        let record = handler
            .load(
                &id,
                crate::session::DEFAULT_MAX_BYTES,
                session.expiry_date()?,
            )
            .await?
            .ok_or(Error::Unauthorized)?;
        if record.id != id {
            return Err(Error::Unauthorized);
        }
        record.encode(crate::session::DEFAULT_MAX_BYTES)?;
        if crate::support::data::get(&record.data, crate::auth::database::AUTH_USER_ID_KEY)
            .and_then(|v| v.as_i64())
            != Some(user_id)
            || record.expiry_date <= time::OffsetDateTime::now_utc()
        {
            return Err(Error::Unauthorized);
        }
        let seconds =
            u64::try_from(record.expiry_date.unix_timestamp()).map_err(|_| Error::Unauthorized)?;
        std::time::UNIX_EPOCH
            .checked_add(std::time::Duration::new(
                seconds,
                record.expiry_date.nanosecond(),
            ))
            .ok_or(Error::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Result,
        session::{CookieSessionStore, MemorySessionStore, Session, SessionRequestContext},
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn authorization_deadline_uses_the_sessions_actual_server_driver() -> Result<()> {
        let handle = SessionStoreHandle::new(MemorySessionStore::default());
        let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        session.start().await?;
        session.put(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
        session.save().await?;
        assert!(handle.authorized_until(&session, 42).await? > std::time::SystemTime::now());
        Ok(())
    }

    #[tokio::test]
    async fn untrusted_client_snapshots_cannot_authenticate_or_issue_server_deadlines() -> Result<()>
    {
        let handle = SessionStoreHandle::new(MemorySessionStore::default());
        let session = Session::new(None, Arc::new(CookieSessionStore::new(2, false)?), None)?;
        let jar = crate::http::CookieJar::new(crate::http::CookieFactory::default());
        session
            .set_request_on_handler(SessionRequestContext::new(
                serde_json::json!({}),
                jar.clone(),
            )?)
            .await?;
        session.start().await?;
        session.put(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
        let protection_error = session
            .require_authenticated_storage()
            .err()
            .ok_or(crate::Error::Unauthorized)?;
        assert_eq!(
            protection_error.code(),
            "session_cookie_protection_required"
        );
        let save_error = session
            .save()
            .await
            .err()
            .ok_or(crate::Error::Unauthorized)?;
        assert_eq!(save_error.code(), "session_cookie_protection_required");
        assert!(jar.get_queued_cookies()?.is_empty());
        let deadline_error = handle
            .authorized_until(&session, 42)
            .await
            .err()
            .ok_or(crate::Error::Unauthorized)?;
        assert_eq!(deadline_error.code(), "session_store_guarantee_unsupported");
        Ok(())
    }
    #[tokio::test]
    async fn authenticated_bags_are_rechecked_after_name_or_handler_changes() -> Result<()> {
        let session = Session::new(None, Arc::new(CookieSessionStore::new(2, false)?), None)?;
        let jar = crate::http::CookieJar::new(crate::http::CookieFactory::default());
        let context = SessionRequestContext::from_request(
            serde_json::json!({}),
            jar.clone(),
            Some(crate::http::cookie::CookieProtection {
                encrypted: true,
                except: vec!["excluded".into()],
            }),
        )?;
        session.set_request_on_handler(context).await?;
        session.start().await?;
        session.put(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
        session.save().await?;
        jar.flush_queued_cookies()?;
        session.set_name("excluded")?;
        let error = session
            .save()
            .await
            .err()
            .ok_or(crate::Error::Unauthorized)?;
        assert_eq!(error.code(), "session_cookie_protection_required");
        assert!(jar.get_queued_cookies()?.is_empty());

        let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        session.start().await?;
        session.put(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
        session.save().await?;
        session.set_handler(Arc::new(CookieSessionStore::new(2, false)?))?;
        session
            .set_request_on_handler(SessionRequestContext::new(
                serde_json::json!({}),
                jar.clone(),
            )?)
            .await?;
        let error = session
            .save()
            .await
            .err()
            .ok_or(crate::Error::Unauthorized)?;
        assert_eq!(error.code(), "session_cookie_protection_required");
        assert!(jar.get_queued_cookies()?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn ordinary_attributes_still_transfer_to_explicit_plain_cookie_storage() -> Result<()> {
        let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        let jar = crate::http::CookieJar::new(crate::http::CookieFactory::default());
        session.start().await?;
        session.put("preference", "compact")?;
        session.save().await?;
        session.set_handler(Arc::new(CookieSessionStore::new(2, false)?))?;
        session
            .set_request_on_handler(SessionRequestContext::new(
                serde_json::json!({}),
                jar.clone(),
            )?)
            .await?;
        session.save().await?;
        let record = session
            .handler()?
            .load(
                &session.id()?,
                crate::session::DEFAULT_MAX_BYTES,
                session.expiry_date()?,
            )
            .await?
            .ok_or(crate::Error::Unauthorized)?;
        assert_eq!(
            crate::support::data::get(&record.data, "preference"),
            Some(serde_json::json!("compact"))
        );
        assert_eq!(jar.get_queued_cookies()?.len(), 1);
        Ok(())
    }
}
