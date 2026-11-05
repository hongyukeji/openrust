//! 真实双库OIDC回调、当前因子与关联边界；不将测试Provider当真实IdP签名验收。
use super::*;
use openrust::auth::fortify::{
    CredentialAuthority, DatabaseFactorAuthority, DatabaseTwoFactorState,
    GoogleTwoFactorAuthenticationProvider, TwoFactorActions,
};
use openrust::auth::{ExternalLoginHandle, UserProvider as _};
use openrust::database::sea_orm::TransactionTrait as _;

struct AuthorityMigrator;
impl MigratorTrait for AuthorityMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        OidcMigrator::migrations()
    }
}

#[tokio::test]
#[ignore = "需要 OPENRUST_TEST_DATABASE_URL 指向真实 MySQL 或 PostgreSQL；集中服务验收显式执行"]
#[allow(
    clippy::too_many_lines,
    reason = "同一真实账户贯穿OIDC、Session、现代挑战和注销，避免孤立fixture替代生产流程"
)]
async fn selected_oidc_challenge_and_attachments_follow_current_authority() -> TestResult {
    // 必须实际配置真实后端，不能静默skip后声称通过。
    let url = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
    let database = TestDatabase::create::<AuthorityMigrator>(&url)
        .await
        .map_err(|error| -> Box<dyn StdError> {
            format!("OIDC selected migrations: {error}").into()
        })?;
    let result:TestResult=Box::pin(async {
        let mut options=openrust::database::sea_orm::ConnectOptions::new(database.url());
        options.max_connections(4).min_connections(0);
        let connection=openrust::database::sea_orm::Database::connect(options).await?;
        let commit_mode=Arc::new(std::sync::atomic::AtomicU8::new(0));
        let listener_mode=commit_mode.clone();
        let mut listeners=openrust::events::EventsServiceProvider::new();
        listeners.listen::<openrust::db::TransactionBeforeCommit,_>(move |_,ctx| {
            let listener_mode=listener_mode.clone();
            Box::pin(async move {
                let mode=listener_mode.load(std::sync::atomic::Ordering::SeqCst);
                if mode==0 {return Ok(());}
                let tx=ctx.transaction().ok_or(Error::Unauthorized)?;
                // prepare_session 的事务尚未写入关联，不能消耗 attach 专属hook。
                if tx.query_one_raw(Statement::from_string(tx.get_database_backend(),
                    "SELECT 1 FROM auth_external_sessions WHERE encrypted_session_id IS NOT NULL LIMIT 1"))
                    .await.map_err(Error::internal)?.is_none() {return Ok(());}
                if listener_mode.compare_exchange(mode,0,std::sync::atomic::Ordering::SeqCst,std::sync::atomic::Ordering::SeqCst).is_err() {return Ok(());}
                match mode {
                    1 => {
                        tx.execute_raw(Statement::from_string(tx.get_database_backend(),
                            "UPDATE auth_external_sessions SET revoked_at=CURRENT_TIMESTAMP"))
                            .await.map_err(Error::internal)?;
                    },
                    2 => tokio::time::sleep(std::time::Duration::from_millis(1100)).await,
                    3 => {
                        tx.execute_raw(Statement::from_string(tx.get_database_backend(),
                            "INSERT INTO auth_factor_challenges SELECT * FROM test_saved_factor_challenge"))
                            .await.map_err(Error::internal)?;
                    },
                    _ => {},
                }
                Ok(())
            })
        });
        let mut builder=openrust::core::ProviderBuilder::new();
        builder.provider(openrust::db::DatabaseServiceProvider);
        builder.provider(listeners);
        let runtime=builder.start(figment::Figment::new()).await?;
        let db=(*runtime.container().resolve::<Db>().await?).clone();
        db.initialize(connection.clone())?;
        let passwords=Passwords::new(PasswordConfig { memory_kib:19_456,iterations:1,parallelism:1 })?;
        let jwt=JwtTokens::new(JwtConfig::new("oidc-owned","oidc-owned","owned",BTreeMap::from([("owned".into(),STANDARD.encode([51_u8;32]))])))?;
        let encryptor=Encryptor::new(EncryptionConfig::new("owned",BTreeMap::from([("owned".into(),STANDARD.encode([61_u8;32]))])))?;
        let policy=CredentialAuthority::new(db.clone(),encryptor.clone(),false);
        let events=(*runtime.container().resolve::<openrust::events::Events>().await?).clone();
        let actions=TwoFactorActions::new(Arc::new(GoogleTwoFactorAuthenticationProvider::new(None)),encryptor.clone(),events);
        let auth=AuthFlows::new(db.clone(),passwords.clone(),jwt.clone(),RefreshTokens::new(db.clone()),
            openrust::auth::VerificationCodes::new(DatabaseVerificationCodeStore::new(db.clone()),passwords.clone()),
            RateLimiter::new(DatabaseRateLimitStore::new(db.clone())))
            .with_factor_authority(policy.clone())?.with_factor_actions(actions.clone());
        let provider=Arc::new(TestProvider::new());
        provider.set_identity("owned",identity("sub-1","owned-oidc@example.test",true)?)?;
        let providers:Vec<Arc<dyn ExternalIdentityProvider>>=vec![provider.clone()];
        let external=ExternalIdentityFlows::new(db.clone(),auth.clone(),encryptor.clone(),providers.clone())?;
        let other=Db::default();other.initialize(database.connection()?)?;
        assert!(ExternalIdentityFlows::new(other,auth.clone(),encryptor.clone(),providers).is_err());
        let start=external.authorize("test-idp","owned-no-factor").await?;
        let state=state_from_url(start.url())?;
        let (attempt,handle)=external.complete_with_handle("test-idp",&state,"owned",start.browser_binding_cookie(),"owned-no-factor").await?.into_parts();
        let openrust::auth::LoginAttempt::Authenticated(issued)=attempt else {return Err("disabled account unexpectedly challenged".into())};
        let user_id=issued.user_id();
        let credential=policy.authenticate_access_token(&jwt,issued.access_token()).await?;
        assert!(credential.factor_origin().is_none());
        let model_db=db.clone();let model_encrypter=encryptor.clone();
        let users=openrust::auth::DatabaseUserProvider::new(db.clone(),passwords,"users")?.with_user_factory(move |attrs| {
            let id=attrs.get("id").cloned().ok_or(Error::Unauthorized)?;
            let store=DatabaseTwoFactorState::new(model_db.clone(),"users","id",id)?;
            Ok(Arc::new(application_user::User::new(attrs).with_two_factor_state(store.clone()).with_factor_authority(DatabaseFactorAuthority::new(store,model_encrypter.clone()))) as openrust::auth::AuthUser)
        });
        let user=users.retrieve_by_id(&serde_json::json!(user_id)).await?.ok_or("missing user")?;
        let session_store=SessionStoreHandle::new(openrust::session::DatabaseSessionStore::new(db.clone()));
        let session=Session::new(None,session_store.store(),None)?; session.start().await?;
        let password_hash=Passwords::new(PasswordConfig { memory_kib:19_456,iterations:1,parallelism:1 })?.hash("owned-session-password").await?;
        db.table("users")?.where_eq("id",user_id)?.update(vec![("password_hash",password_hash.as_str().into())]).await?;
        auth.login_session("owned-oidc@example.test","owned-session-password","owned-session",&session).await?;
        session.save().await?;
        let session_id=session.id()?;
        commit_mode.store(1,std::sync::atomic::Ordering::SeqCst);
        assert!(external.attach_session(user_id,&handle,&session).await.is_err());
        let handle_hash=format!("{:x}",sha2::Sha256::digest(handle.expose()));
        let rolled_back=db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash.clone())?.first().await?.ok_or("missing association")?;
        assert!(rolled_back.try_get::<Option<chrono::DateTime<chrono::Utc>>>("", "revoked_at")?.is_none());
        assert!(rolled_back.try_get::<Option<String>>("", "encrypted_session_id")?.is_none());
        external.attach_session(user_id,&handle,&session).await?;
        let bare=Session::new(None,Arc::new(openrust::session::MemorySessionStore::default()),None)?;bare.start().await?;
        bare.put_typed("openrust.auth.user_id",user_id)?;
        assert!(external.attach_session(user_id,&handle,&bare).await.is_err());
        // 仅在测试中退回同一真实句柄的未绑定状态，单独覆盖公开refresh attach。
        let handle_hash=format!("{:x}",sha2::Sha256::digest(handle.expose()));
        db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash.clone())?
            .update(vec![("refresh_family_id",Option::<String>::None.into())]).await?;
        let rotated=auth.refresh(issued.refresh_token().expose(),"owned-rotate").await?;
        assert!(external.attach_refresh_credentials(user_id,&handle,issued.refresh_token().expose()).await.is_err());
        // 末位监听器撤销关联时必须回滚刚写入的家族与监听器本身的修改。
        commit_mode.store(1,std::sync::atomic::Ordering::SeqCst);
        assert!(external.attach_refresh_credentials(user_id,&handle,rotated.refresh_token().expose()).await.is_err());
        let rolled_back=db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash.clone())?.first().await?.ok_or("missing association")?;
        assert!(rolled_back.try_get::<Option<chrono::DateTime<chrono::Utc>>>("", "revoked_at")?.is_none());
        assert!(rolled_back.try_get::<Option<String>>("", "refresh_family_id")?.is_none());
        let original_expiry=chrono::Utc::now()+chrono::Duration::seconds(1);
        db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash.clone())?
            .update(vec![("expires_at",original_expiry.into())]).await?;
        commit_mode.store(2,std::sync::atomic::Ordering::SeqCst);
        assert!(external.attach_refresh_credentials(user_id,&handle,rotated.refresh_token().expose()).await.is_err());
        let rolled_back=db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash.clone())?.first().await?.ok_or("missing association")?;
        assert!(rolled_back.try_get::<Option<String>>("", "refresh_family_id")?.is_none());
        db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash.clone())?
            .update(vec![("expires_at",(chrono::Utc::now()+chrono::Duration::minutes(5)).into())]).await?;
        // 旧raw拒绝不能顺带撤销家族；当前raw可绑定且不被消费。
        external.attach_refresh_credentials(user_id,&handle,rotated.refresh_token().expose()).await?;
        let still_current=auth.refresh(rotated.refresh_token().expose(),"owned-rotate-again").await?;
        policy.authenticate_access_token(&jwt,still_current.access_token()).await?;
        // 持有真实raw行，attach先锁user/association后等待；原关联期限在等待中到期。
        let expires=chrono::Utc::now()+chrono::Duration::seconds(2);
        db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash.clone())?
            .update(vec![("refresh_family_id",Option::<String>::None.into()),("expires_at",expires.into())]).await?;
        let backend=connection.get_database_backend();
        let refresh_table=db.quoted_table_name("auth_refresh_tokens")?;
        let holder=connection.begin().await?;
        let token_hash=format!("{:x}",sha2::Sha256::digest(still_current.refresh_token().expose()));
        let marker=if backend==openrust::database::sea_orm::DbBackend::Postgres {"$1"} else {"?"};
        holder.query_one_raw(Statement::from_sql_and_values(backend,format!("SELECT user_id FROM {refresh_table} WHERE token_hash={marker} FOR UPDATE"),[token_hash.into()])).await?.ok_or("missing held raw")?;
        let attach_external=external.clone();let attach_handle=ExternalLoginHandle::from_exposed(handle.expose())?;let raw=still_current.refresh_token().expose().to_owned();
        let mut task=tokio::spawn(async move { attach_external.attach_refresh_credentials(user_id,&attach_handle,&raw).await });
        let users_table=db.quoted_table_name("users")?;
        let barrier=tokio::time::Instant::now()+std::time::Duration::from_secs(5);
        loop {
            let probe=connection.begin().await?;
            let attempted=probe.query_one_raw(Statement::from_sql_and_values(backend,format!("SELECT id FROM {users_table} WHERE id={marker} FOR UPDATE NOWAIT"),[user_id.into()])).await;
            probe.rollback().await?;
            let held=match attempted {
                Ok(_)=>false,
                Err(error)=>if is_nowait_conflict(&error) { true } else { task.abort();let _=task.await;holder.rollback().await?;return Err(error.into()); },
            };
            if held {break;}
            if task.is_finished() || tokio::time::Instant::now()>=barrier { task.abort();let _=task.await;holder.rollback().await?;return Err("attach did not wait at real row barrier".into()); }
            tokio::task::yield_now().await;
        }
        let delay=(expires-chrono::Utc::now()).to_std().unwrap_or_default()+std::time::Duration::from_millis(20);
        tokio::time::sleep(delay).await;
        holder.rollback().await?;
        let attached=match tokio::time::timeout(std::time::Duration::from_secs(5),&mut task).await {
            Ok(joined)=>joined?,
            Err(error)=>{task.abort();let _=task.await;return Err(error.into());}
        };
        assert!(attached.is_err());
        policy.authenticate_access_token(&jwt,still_current.access_token()).await?;
        // 本测试恢复fixture期限，后续过时代际断言不会只因handle已过期通过。
        db.table("auth_external_sessions")?.where_eq("id_hash",handle_hash)?
            .update(vec![("expires_at",(chrono::Utc::now()+chrono::Duration::minutes(5)).into())]).await?;
        actions.enable(&user,false).await?;
        assert!(external.attach_session(user_id,&handle,&session).await.is_err());
        assert!(policy.authenticate_access_token(&jwt,issued.access_token()).await.is_err());
        let recovery=actions.recovery_codes(&user).await?;
        let start=external.authorize("test-idp","owned-factor").await?;
        let state=state_from_url(start.url())?;
        let (attempt,challenge_handle)=external.complete_with_handle("test-idp",&state,"owned",start.browser_binding_cookie(),"owned-factor").await?.into_parts();
        let openrust::auth::LoginAttempt::TwoFactorRequired(challenge)=attempt else {return Err("IdP first factor bypassed local challenge".into())};
        let signed=auth.complete_two_factor_login(challenge.expose(),None,Some(&recovery[0]),"owned-factor").await?;
        let credential=policy.authenticate_access_token(&jwt,signed.access_token()).await?;
        assert!(matches!(credential.factor_origin(),Some(openrust::auth::FactorOrigin::DatabaseFlow{purpose,..}) if purpose=="external:test-idp"));
        assert!(auth.complete_two_factor_login(challenge.expose(),None,Some(&recovery[0]),"owned-factor").await.is_err());
        assert!(external.attach_refresh_credentials(user_id,&challenge_handle,signed.refresh_token().expose()).await.is_err());
        // 模拟提交前扩展恢复被attach删除的真实挑战；不得失去外部注销关联后继续消费。
        let start=external.authorize("test-idp","owned-revive").await?;
        let state=state_from_url(start.url())?;
        let (attempt,revive_handle)=external.complete_with_handle("test-idp",&state,"owned",start.browser_binding_cookie(),"owned-revive").await?.into_parts();
        let openrust::auth::LoginAttempt::TwoFactorRequired(revive)=attempt else {return Err("missing revive challenge".into())};
        let revive_hash=format!("{:x}",sha2::Sha256::digest(revive.expose()));
        let revive_handle_hash=format!("{:x}",sha2::Sha256::digest(revive_handle.expose()));
        let backend=connection.get_database_backend();
        let marker=if backend==openrust::db::sea_orm::DbBackend::Postgres {"$1"} else {"?"};
        connection.execute_raw(Statement::from_sql_and_values(backend,
            format!("CREATE TABLE test_saved_factor_challenge AS SELECT * FROM auth_factor_challenges WHERE token_hash={marker}"),
            [revive_hash.clone().into()])).await?;
        commit_mode.store(3,std::sync::atomic::Ordering::SeqCst);
        assert!(external.attach_refresh_credentials(user_id,&revive_handle,signed.refresh_token().expose()).await.is_err());
        let rollback=db.table("auth_external_sessions")?.where_eq("id_hash",revive_handle_hash)?.first().await?.ok_or("missing revive association")?;
        assert_eq!(rollback.try_get::<Option<String>>("", "challenge_hash")?.as_deref(),Some(revive_hash.as_str()));
        assert!(rollback.try_get::<Option<String>>("", "refresh_family_id")?.is_none());
        assert_eq!(db.table("auth_factor_challenges")?.where_eq("token_hash",revive_hash)?.count().await?,1);
        external.attach_refresh_credentials(user_id,&revive_handle,signed.refresh_token().expose()).await?;
        assert!(auth.complete_two_factor_login(revive.expose(),None,Some(&recovery[1]),"owned-revive").await.is_err());
        // 第二个待完成挑战在后通道注销后不可继续；同一注销同时撤销已签家族。
        let start=external.authorize("test-idp","owned-revoke").await?;
        let state=state_from_url(start.url())?;
        let (attempt,_)=external.complete_with_handle("test-idp",&state,"owned",start.browser_binding_cookie(),"owned-revoke").await?.into_parts();
        let openrust::auth::LoginAttempt::TwoFactorRequired(pending)=attempt else {return Err("missing pending challenge".into())};
        external.process_backchannel_logout("test-idp","test-provider-subject-token",&session_store).await?;
        assert!(auth.complete_two_factor_login(pending.expose(),None,Some(&recovery[1]),"owned-revoke").await.is_err());
        assert!(policy.authenticate_access_token(&jwt,signed.access_token()).await.is_err());
        assert!(session_store.store().load_authorized(&session_id,openrust::session::DEFAULT_MAX_BYTES).await?.is_none());
        drop(runtime);
        Ok(())
    }).await;
    let cleanup = database.close().await;
    result.map_err(|error| -> Box<dyn StdError> {
        format!("OIDC selected integration: {error}").into()
    })?;
    cleanup?;
    Ok(())
}

use super::web::application_user;

fn is_nowait_conflict(error: &openrust::database::sea_orm::DbErr) -> bool {
    use openrust::database::sea_orm::{DbErr, RuntimeErr};
    let (DbErr::Query(RuntimeErr::SqlxError(error)) | DbErr::Exec(RuntimeErr::SqlxError(error))) =
        error
    else {
        return false;
    };
    let Some(error) = error.as_database_error() else {
        return false;
    };
    if error.code().as_deref() == Some("55P03") {
        return true;
    }
    #[cfg(feature = "db-mysql")]
    if let Some(error) =
        error.try_downcast_ref::<openrust::database::sea_orm::sqlx::mysql::MySqlDatabaseError>()
    {
        return error.number() == 3572;
    }
    false
}
