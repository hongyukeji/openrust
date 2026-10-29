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
        let id = session.id()?;
        let record = self
            .0
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
