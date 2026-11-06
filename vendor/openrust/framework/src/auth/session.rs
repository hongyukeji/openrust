//! Auth与通用Session复用同一状态、store和撤销机制。

#[cfg(feature = "session-redis")]
pub use crate::session::RedisSessionStore;
pub(crate) use crate::session::SessionSettings;
pub use crate::session::{DatabaseSessionStore, SessionServiceProvider, SessionStoreHandle};

impl SessionStoreHandle {
    /// 比较请求快照与当前持久身份，不续期；返回截止不晚于持久列或TTL。
    /// 原生数据库按本次读取的活动时间计算期限，不提升其并发写入/删除保证。
    pub async fn authorized_until(
        &self,
        session: &crate::session::Session,
        user_id: i64,
    ) -> crate::Result<std::time::SystemTime> {
        use crate::Error;
        if session.get_typed::<i64>(crate::auth::database::AUTH_USER_ID_KEY)? != Some(user_id) {
            return Err(Error::Unauthorized);
        }
        persisted_until(
            session,
            crate::auth::database::AUTH_USER_ID_KEY,
            &serde_json::Value::from(user_id),
        )
        .await
    }

    /// 按实际具名 Guard 及当前持久记录限制连接期限；不接受仅本请求的 once 身份。
    /// 原生数据库只保证当前读取边界，不承诺与后续网络发送原子撤销。
    pub async fn authorized_until_guard(
        &self,
        session: &crate::session::Session,
        guard: &dyn crate::auth::StatefulGuard,
    ) -> crate::Result<std::time::SystemTime> {
        let id = guard.id().await?.ok_or(crate::Error::Unauthorized)?;
        let key = guard_session_key(session, guard, &id).await?;
        persisted_until(session, &key, &id).await
    }
}

pub(crate) fn require_account_provider(
    db: &crate::database::Db,
    guard: &dyn crate::auth::StatefulGuard,
) -> crate::Result<()> {
    if !guard.provider().uses_database_accounts(db, "users")? {
        return Err(crate::Error::Unauthorized);
    }
    Ok(())
}

pub(crate) async fn guard_session_key(
    session: &crate::session::Session,
    guard: &dyn crate::auth::StatefulGuard,
    user_id: &serde_json::Value,
) -> crate::Result<String> {
    session.require_authenticated_storage()?;
    let key = guard.session_name().ok_or(crate::Error::Unauthorized)?;
    let account = guard.user().await?.ok_or(crate::Error::Unauthorized)?;
    if &account.auth_identifier()? != user_id || session.get_json(&key)?.as_ref() != Some(user_id) {
        return Err(crate::Error::Unauthorized);
    }
    Ok(key)
}

async fn persisted_until(
    session: &crate::session::Session,
    key: &str,
    user_id: &serde_json::Value,
) -> crate::Result<std::time::SystemTime> {
    use crate::Error;
    let handler = session.handler()?;
    if handler.guarantees() == crate::session::SessionStoreGuarantees::ClientSideSnapshot {
        return Err(Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "session_store_guarantee_unsupported",
            "客户端Session快照无法提供服务器端当前记录读取",
        ));
    }
    let id = session.id()?;
    let record = handler
        .load_authorized(&id, crate::session::DEFAULT_MAX_BYTES)
        .await?
        .ok_or(Error::Unauthorized)?;
    if record.id != id {
        return Err(Error::Unauthorized);
    }
    record.encode(crate::session::DEFAULT_MAX_BYTES)?;
    if crate::support::data::get(&record.data, key).as_ref() != Some(user_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Result,
        session::{CookieSessionStore, MemorySessionStore, Session, SessionRequestContext},
    };
    use std::sync::Arc;

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    #[allow(
        clippy::too_many_lines,
        reason = "同一数据库事务与安全断言保留完整回归场景"
    )]
    async fn legacy_account_flows_reject_other_provider_domains_before_password_or_session_io()
    -> Result<()> {
        use crate::auth::{
            AuthFlows, DatabaseUserProvider, DatabaseVerificationCodeStore, JwtConfig, JwtTokens,
            RefreshTokens, SessionGuard, VerificationCodes,
        };
        use crate::core::RateLimiter;
        use crate::database::{DatabaseRateLimitStore, Db};
        struct NoMigrations;
        impl crate::database::sea_orm_migration::MigratorTrait for NoMigrations {
            fn migrations() -> Vec<Box<dyn crate::database::sea_orm_migration::MigrationTrait>> {
                Vec::new()
            }
        }
        let database = crate::testing::TestDatabase::sqlite_file::<NoMigrations>().await?;
        let db = Db::default();
        db.initialize(database.connection()?)?;
        let result: Result<()> = async {
            let other_db = Db::default();
            other_db.initialize(database.connection()?)?;
            let passwords = std::sync::Arc::new(crate::hashing::HashManager::new(
                crate::hashing::HashingConfig {
                    bcrypt: crate::hashing::BcryptConfig {
                        rounds: 4,
                        verify: true,
                        limit: None,
                    },
                    ..Default::default()
                },
            )?) as std::sync::Arc<dyn crate::hashing::Hasher>;
            let jwt = JwtTokens::new(JwtConfig {
                issuer: "http://scope.test".into(),
                audience: "scope-test".into(),
                active_key_id: "test".into(),
                keys: std::collections::BTreeMap::from([(
                    "test".into(),
                    "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=".into(),
                )]),
                access_ttl_secs: 900,
                max_valid_ttl_secs: 3600,
            })?;
            let flows = AuthFlows::new(
                db.clone(),
                passwords.clone(),
                jwt,
                RefreshTokens::new(db.clone()),
                VerificationCodes::new(
                    DatabaseVerificationCodeStore::new(db.clone()),
                    passwords.clone(),
                ),
                RateLimiter::new(DatabaseRateLimitStore::new(db.clone())),
            );
            // 两种常见错误配置：同数据库其他账户表、其他数据库同名表。
            for (provider_db, table) in [(db.clone(), "admins"), (other_db, "users")] {
                let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
                let guard = SessionGuard::new(
                    "web".into(),
                    Arc::new(DatabaseUserProvider::new(
                        provider_db,
                        passwords.clone(),
                        table,
                    )?),
                    session.clone(),
                    crate::http::CookieJar::new(crate::http::CookieFactory::default()),
                    None,
                    None,
                    None,
                );
                assert!(matches!(
                    flows
                        .login_session_with_guard(
                            "user@example.test",
                            "secret",
                            "source",
                            &session,
                            &guard
                        )
                        .await,
                    Err(crate::Error::Unauthorized)
                ));
                assert!(matches!(
                    flows
                        .logout_session_with_guard("source", &session, &guard)
                        .await,
                    Err(crate::Error::Unauthorized)
                ));
            }
            let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
            let guard = SessionGuard::new(
                "web".into(),
                Arc::new(DatabaseUserProvider::new(db.clone(), passwords, "users")?),
                session,
                crate::http::CookieJar::new(crate::http::CookieFactory::default()),
                None,
                None,
                None,
            );
            flows.require_account_provider(&guard)?;
            Ok(())
        }
        .await;
        drop(db);
        let closed = database.close().await;
        result?;
        closed
    }

    #[cfg(feature = "db-sqlite")]
    #[tokio::test]
    async fn missing_database_user_session_id_fallback_cannot_authorize_connections() -> Result<()>
    {
        use crate::{
            auth::{DatabaseUserProvider, Guard, SessionGuard, UserProvider},
            database::{Db, sea_orm::ConnectionTrait},
        };

        fn require(condition: bool, message: &'static str) -> Result<()> {
            if condition {
                Ok(())
            } else {
                Err(crate::Error::custom(
                    "guest_fallback_regression_failed",
                    message,
                ))
            }
        }

        struct NoMigrations;
        impl crate::database::sea_orm_migration::MigratorTrait for NoMigrations {
            fn migrations() -> Vec<Box<dyn crate::database::sea_orm_migration::MigrationTrait>> {
                Vec::new()
            }
        }

        let database = crate::testing::TestDatabase::sqlite_file::<NoMigrations>().await?;
        let db = Db::default();
        let result: Result<()> = async {
            db.initialize(database.connection()?)?;
            database
            .connection()?
            .execute_unprepared(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, password TEXT, remember_token TEXT)",
            )
            .await
            .map_err(crate::Error::internal)?;
            let identifier = serde_json::json!(42);
            let provider = Arc::new(DatabaseUserProvider::new(
                db.clone(),
                std::sync::Arc::new(crate::hashing::HashManager::new(
                    crate::hashing::HashingConfig {
                        bcrypt: crate::hashing::BcryptConfig {
                            rounds: 4,
                            verify: true,
                            limit: None,
                        },
                        ..Default::default()
                    },
                )?) as std::sync::Arc<dyn crate::hashing::Hasher>,
                "users",
            )?);
            // 表真实存在且查询正常返回缺失账户，不能以查询错误冒充 guest。
            require(
                provider.retrieve_by_id(&identifier).await?.is_none(),
                "真实 users 表应返回缺失账户",
            )?;

            let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
            session.start().await?;
            let guard = SessionGuard::new(
                "web".into(),
                provider,
                session.clone(),
                crate::http::CookieJar::new(crate::http::CookieFactory::default()),
                None,
                None,
                None,
            );
            let key = guard.session_name();
            session.put_typed(&key, &identifier)?;
            session.save().await?;

            // 持久 Session 记录、账户键和未过期截止全部有效；唯一缺失条件是账户本身。
            // persisted_until 内部实际 load，并校验记录 ID、账户键和到期时间。
            require(
                persisted_until(&session, &key, &identifier).await? > std::time::SystemTime::now(),
                "仅持久键校验应能给出有效期限",
            )?;

            // 保留 Laravel SessionGuard.id 的原行为：账户不存在时仍可返回 Session ID。
            require(
                guard.user().await?.is_none(),
                "缺失账户的 Guard 必须仍为 guest",
            )?;
            require(
                guard.id().await? == Some(identifier.clone()),
                "必须保留 SessionGuard.id 的官方 fallback",
            )?;
            require(
                matches!(
                    guard_session_key(&session, &guard, &identifier).await,
                    Err(crate::Error::Unauthorized)
                ),
                "guest 不得取得正式 Guard Session 键",
            )?;
            let handle = SessionStoreHandle::new(MemorySessionStore::default());
            require(
                matches!(
                    handle.authorized_until_guard(&session, &guard).await,
                    Err(crate::Error::Unauthorized)
                ),
                "guest 不得取得连接授权期限",
            )?;
            Ok(())
        }
        .await;
        drop(db);
        // 无论测试检查及其他 fallible 操作成功与否，都关闭并清理本测试自有数据库。
        let closed = database.close().await;
        result?;
        closed
    }

    #[tokio::test]
    async fn authorization_deadline_uses_the_sessions_actual_server_driver() -> Result<()> {
        let handle = SessionStoreHandle::new(MemorySessionStore::default());
        let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        session.start().await?;
        session.put_typed(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
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
        session.put_typed(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
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
        session.put_typed(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
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
        session.put_typed(crate::auth::database::AUTH_USER_ID_KEY, 42_i64)?;
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
        session.put_typed("preference", "compact")?;
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

#[cfg(feature = "auth-fortify")]
mod live;
#[cfg(feature = "auth-fortify")]
pub use live::LiveSessionCredential;
