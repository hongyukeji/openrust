//! 真实SQLite账户/密码/家族的管理grant生命周期；不伪造因子收据。
use super::*;
use crate::{
    auth::fortify::{
        DatabaseFactorAuthority, DatabaseTwoFactorState, FortifyCallbacks, FortifyConfig,
        FortifyJwtApi, GoogleTwoFactorAuthenticationProvider, TwoFactorAuthenticatable,
        TwoFactorOptions, TwoFactorState,
    },
    auth::{
        AuthFlows, Authenticatable, DatabaseUserProvider, DatabaseVerificationCodeStore,
        GenericUser, LoginAttempt, Passwords, VerificationCodes,
        credential::{CredentialGuardBinding, JwtCredentialGrant},
    },
    config::{EncryptionConfig, JwtConfig, PasswordConfig},
    core::{BoxFuture, ProviderBuilder, RateLimiter},
    db::{
        DatabaseRateLimitMigration, DatabaseRateLimitStore,
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    events::{Events, EventsServiceProvider},
    http::{
        RequestInfo,
        axum::{
            body::Body,
            extract::{ConnectInfo, FromRequestParts},
            http::{Request, StatusCode},
        },
    },
    testing::TestDatabase,
};
use std::collections::BTreeMap;

const PASSWORD: &str = "api-management-real-password";
const KEY: &str = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(crate::auth::AuthMigration),
            #[cfg(feature = "grpc")]
            Box::new(crate::auth::AuthPolicyMigration),
            Box::new(crate::session::GuardedSessionMigration),
            Box::new(crate::auth::AuthRefreshTokenMigration),
            Box::new(DatabaseRateLimitMigration),
            Box::new(crate::audit::DatabaseAuditMigration),
            Box::new(super::super::FortifyTwoFactorMigration::new()),
            Box::new(super::super::FactorAuthorityMigration::new()),
            Box::new(super::super::CredentialFactorMigration),
        ]
    }
}
struct Account {
    attributes: GenericUser,
    authority: DatabaseFactorAuthority,
}
impl Authenticatable for Account {
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        Some(self)
    }
    fn auth_attribute(&self, name: &str) -> Result<Option<Value>> {
        self.attributes.auth_attribute(name)
    }
    fn auth_identifier_name(&self) -> &'static str {
        "id"
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &'static str {
        "password_hash"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        Ok(self
            .attributes
            .auth_attribute("password_hash")?
            .and_then(|value| value.as_str().map(str::to_owned)))
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn set_remember_token(&self, value: Option<String>) -> Result<()> {
        self.attributes.set_remember_token(value)
    }
    fn remember_token_name(&self) -> &'static str {
        "remember_token"
    }
}
impl TwoFactorAuthenticatable for Account {
    fn two_factor_authority(&self) -> Option<&DatabaseFactorAuthority> {
        Some(&self.authority)
    }
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        Box::pin(async move { Ok(self.authority.load().await?.state) })
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        _: &'a TwoFactorState,
        _: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Err(Error::Unauthorized) })
    }
    fn two_factor_account_name(&self, _: &str) -> Result<Option<String>> {
        Ok(Some("api-management-account".into()))
    }
}
struct Fixture {
    db: Db,
    manager: JwtApiFactorManagement,
    passwords: Passwords,
}
async fn owned<F>(confirm: bool, callback: F) -> Result<()>
where
    F: FnOnce(Fixture) -> BoxFuture<'static, Result<()>>,
{
    owned_with_events(confirm, EventsServiceProvider::new(), callback).await
}

async fn owned_with_events<F>(
    confirm: bool,
    events: EventsServiceProvider,
    callback: F,
) -> Result<()>
where
    F: FnOnce(Fixture) -> BoxFuture<'static, Result<()>>,
{
    let database = TestDatabase::sqlite_file::<Migrator>().await?;
    owned_database(database, confirm, events, ProviderBuilder::new(), callback).await
}

async fn owned_database<F>(
    database: TestDatabase,
    confirm: bool,
    events: EventsServiceProvider,
    mut builder: ProviderBuilder,
    callback: F,
) -> Result<()>
where
    F: FnOnce(Fixture) -> BoxFuture<'static, Result<()>>,
{
    builder.provider(crate::db::DatabaseServiceProvider);
    builder.provider(events);
    let runtime = builder.start(figment::Figment::new()).await?;
    let outcome: Result<()> = async {
        let db = (*runtime.container().resolve::<Db>().await?).clone();
        db.initialize(database.connection()?)?;
        let passwords = Passwords::new(PasswordConfig::default())?;
        let password = passwords.hash(PASSWORD).await?;
        let now = crate::db::dialect::utc_value(db.backend()?, Utc::now());
        db.table("users")?
            .insert(vec![
                ("id", 42_i64.into()),
                ("email", "management@example.test".into()),
                ("password_hash", password.as_str().into()),
                ("email_verified_at", now.clone()),
                ("created_at", now.clone()),
                ("updated_at", now),
            ])
            .await?;
        let encrypter = Encryptor::new(EncryptionConfig::new(
            "test",
            BTreeMap::from([("test".into(), KEY.into())]),
        ))?;
        let factory_db = db.clone();
        let factory_crypto = encrypter.clone();
        let provider: Arc<dyn UserProvider> = Arc::new(
            DatabaseUserProvider::new(db.clone(), passwords.clone(), "users")?.with_user_factory(
                move |attributes| {
                    let id = attributes.get("id").cloned().ok_or(Error::Unauthorized)?;
                    let authority = DatabaseFactorAuthority::new(
                        DatabaseTwoFactorState::new(factory_db.clone(), "users", "id", id)?,
                        factory_crypto.clone(),
                    );
                    Ok(Arc::new(Account {
                        attributes: GenericUser::new(attributes),
                        authority,
                    }) as AuthUser)
                },
            ),
        );
        let events = (*runtime.container().resolve::<Events>().await?).clone();
        let actions = TwoFactorActions::new(
            Arc::new(
                GoogleTwoFactorAuthenticationProvider::new(None).with_clock(Arc::new(|| Ok(3_000))),
            ),
            encrypter.clone(),
            events,
        )
        .with_options(TwoFactorOptions {
            confirm,
            ..TwoFactorOptions::default()
        });
        let tokens = JwtTokens::new(JwtConfig::new(
            "https://management.test",
            "management-test",
            "test",
            BTreeMap::from([("test".into(), KEY.into())]),
        ))?;
        let manager = JwtApiFactorManagement::new(
            CredentialAuthority::new(db.clone(), encrypter.clone(), confirm),
            actions,
            tokens,
            provider,
            JwtApiFactorManagementOptions {
                encrypter,
                guard: "api".into(),
                provider_name: Some("users".into()),
                confirm_passwords: None,
            },
        )?;
        callback(Fixture {
            db: db.clone(),
            manager,
            passwords,
        })
        .await
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await;
    drop(runtime);
    let closed = database.close().await;
    outcome?;
    shutdown?;
    closed
}
async fn access(fixture: &Fixture, scope: Option<CredentialGuardBinding>) -> Result<String> {
    let authority = fixture.manager.authority.clone();
    let db = fixture.db.clone();
    let tokens = fixture.manager.tokens.clone();
    // 构造真实持久家族与当前Disabled上下文的已签名JWT；管理入口仍必须真实核验密码。
    fixture
        .db
        .transaction_for_update(move |tx| {
            Box::pin(async move {
                let mut context = authority
                    .context_without_factor_in_transaction(tx, 42)
                    .await?;
                context.scope = scope;
                let issued = RefreshTokens::new(db)
                    .issue_context_in_transaction(
                        tx,
                        42,
                        Utc::now() + chrono::Duration::seconds(120),
                        &context,
                    )
                    .await?;
                tokens.issue_access_token_with_grant(
                    42,
                    JwtCredentialGrant {
                        family_id: issued.family_id,
                        context,
                    },
                )
            })
        })
        .await
}
fn scope(tokens: &JwtTokens) -> CredentialGuardBinding {
    let (issuer, audience) = tokens.credential_domain();
    CredentialGuardBinding {
        issuer: issuer.to_owned(),
        audience: audience.to_owned(),
        guard: "api".into(),
        provider: "users".into(),
    }
}
fn encode(
    manager: &JwtApiFactorManagement,
    authorization: &ApiManagementAuthorization,
) -> Result<String> {
    Ok(format!(
        "{PREFIX}{}",
        manager.encrypter.encrypt_with_context(
            &serde_json::to_vec(authorization).map_err(Error::internal)?,
            PREFIX.as_bytes()
        )?
    ))
}
#[tokio::test]
async fn real_password_management_grant_enables_reads_and_confirmation_retires_binding()
-> Result<()> {
    owned(false, |fixture| {
        Box::pin(async move {
            let token = access(&fixture, Some(scope(&fixture.manager.tokens))).await?;
            assert!(
                fixture
                    .manager
                    .prepare(&token, "incorrect-password")
                    .await
                    .is_err()
            );
            let grant = fixture.manager.prepare(&token, PASSWORD).await?;
            let other_grant = fixture.manager.prepare(&token, PASSWORD).await?;
            let authorization = fixture.manager.decode(&grant)?;
            assert!(authorization.expires_at - authorization.issued_at <= 120);
            assert!(authorization.context.factor.is_none());
            assert!(fixture.manager.tokens.verify_access_token(&grant).is_err());
            let (user, actions) = fixture.manager.actions(&grant).await?;
            actions.enable(&user, false).await?;
            assert!(fixture.manager.actions(&other_grant).await.is_err());
            let (user, actions) = fixture.manager.actions(&grant).await?;
            let secret = actions.secret_key(&user).await?;
            assert_eq!(secret.len(), 16);
            let original = actions.recovery_codes(&user).await?;
            assert_eq!(original.len(), 8);
            actions.generate_new_recovery_codes(&user).await?;
            let (user, actions) = fixture.manager.actions(&grant).await?;
            let replacement = actions.recovery_codes(&user).await?;
            assert_eq!(replacement.len(), 8);
            assert!(original.iter().all(|code| !replacement.contains(code)));
            assert!(
                fixture
                    .manager
                    .authority
                    .authenticate_access_token(&fixture.manager.tokens, &token)
                    .await
                    .is_err()
            );
            let code = crate::auth::totp::totp_code(
                &super::super::engine::decode_secret(&secret)?,
                100,
                6,
            )?;
            actions.confirm(&user, &code).await?;
            assert!(fixture.manager.actions(&grant).await.is_err());
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn grants_reject_wrong_scope_prefix_unknown_fields_future_and_expiry() -> Result<()> {
    owned(false, |fixture| {
        Box::pin(async move {
            assert!(
                fixture
                    .manager
                    .prepare(&access(&fixture, None).await?, PASSWORD)
                    .await
                    .is_err()
            );
            let other = Some(CredentialGuardBinding {
                guard: "other".into(),
                ..scope(&fixture.manager.tokens)
            });
            assert!(
                fixture
                    .manager
                    .prepare(&access(&fixture, other).await?, PASSWORD)
                    .await
                    .is_err()
            );
            let grant = fixture
                .manager
                .prepare(
                    &access(&fixture, Some(scope(&fixture.manager.tokens))).await?,
                    PASSWORD,
                )
                .await?;
            assert!(
                fixture
                    .manager
                    .actions(&grant.replacen(PREFIX, "other.", 1))
                    .await
                    .is_err()
            );
            let original = fixture.manager.decode(&grant)?;
            for change in 0..8 {
                let mut invalid = original.clone();
                match change {
                    0 => invalid.issued_at = Utc::now().timestamp() + 60,
                    1 => invalid.expires_at = Utc::now().timestamp() - 1,
                    2 => invalid.expires_at = invalid.issued_at + 301,
                    3 => invalid.provider = Some("other".into()),
                    4 => invalid.context.scope = None,
                    5 => invalid.version = 2,
                    6 => invalid.user_id = 7,
                    _ => invalid.family_id = "f".repeat(32),
                }
                assert!(
                    fixture
                        .manager
                        .actions(&encode(&fixture.manager, &invalid)?)
                        .await
                        .is_err()
                );
            }
            let mut fields = serde_json::to_value(&original).map_err(Error::internal)?;
            fields
                .as_object_mut()
                .ok_or(Error::Unauthorized)?
                .insert("extra".into(), Value::Bool(true));
            let bad = format!(
                "{PREFIX}{}",
                fixture.manager.encrypter.encrypt_with_context(
                    &serde_json::to_vec(&fields).map_err(Error::internal)?,
                    PREFIX.as_bytes()
                )?
            );
            assert!(fixture.manager.actions(&bad).await.is_err());
            Ok(())
        })
    })
    .await
}
#[tokio::test]
async fn password_changes_and_family_revocation_reject_existing_management_grant() -> Result<()> {
    owned(false, |fixture| {
        Box::pin(async move {
            let grant = fixture
                .manager
                .prepare(
                    &access(&fixture, Some(scope(&fixture.manager.tokens))).await?,
                    PASSWORD,
                )
                .await?;
            let different = fixture.passwords.hash("new-password").await?;
            fixture
                .db
                .table("users")?
                .where_eq("id", 42_i64)?
                .update(vec![("password_hash", different.as_str().into())])
                .await?;
            assert!(fixture.manager.actions(&grant).await.is_err());
            let restored = fixture.passwords.hash(PASSWORD).await?;
            fixture
                .db
                .table("users")?
                .where_eq("id", 42_i64)?
                .update(vec![("password_hash", restored.as_str().into())])
                .await?;
            let grant = fixture
                .manager
                .prepare(
                    &access(&fixture, Some(scope(&fixture.manager.tokens))).await?,
                    PASSWORD,
                )
                .await?;
            RefreshTokens::new(fixture.db.clone())
                .revoke_all_for_user(42)
                .await?;
            assert!(fixture.manager.actions(&grant).await.is_err());
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn password_hash_change_during_real_confirmation_callback_prevents_grant() -> Result<()> {
    owned(false, |fixture| {
        Box::pin(async move {
            let access = access(&fixture, Some(scope(&fixture.manager.tokens))).await?;
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let mut manager = fixture.manager.clone();
            let provider = manager.provider.clone();
            let verified = entered.clone();
            let resumed = release.clone();
            manager.confirm_passwords = Some(Arc::new(move |user, password| {
                let provider = provider.clone();
                let verified = verified.clone();
                let resumed = resumed.clone();
                Box::pin(async move {
                    let credentials = Credentials::new(Map::from_iter([(
                        "password".into(),
                        password.map_or(Value::Null, Value::String),
                    )]));
                    let valid = provider
                        .validate_credentials(user.as_ref(), &credentials)
                        .await?;
                    verified.notify_one();
                    resumed.notified().await;
                    Ok(valid)
                })
            }));
            let worker = tokio::spawn(async move { manager.prepare(&access, PASSWORD).await });
            let outcome: Result<()> = async {
                tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                    .await
                    .map_err(Error::internal)?;
                let password = fixture.passwords.hash("new-concurrent-password").await?;
                fixture
                    .db
                    .table("users")?
                    .where_eq("id", 42_i64)?
                    .update(vec![("password_hash", password.as_str().into())])
                    .await?;
                Ok(())
            }
            .await;
            release.notify_one();
            let finished = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
                .await
                .map_err(Error::internal)?
                .map_err(Error::internal)?;
            outcome?;
            assert!(finished.is_err());
            Ok(())
        })
    })
    .await
}

fn auth_flows(fixture: &Fixture) -> Result<AuthFlows> {
    let manager = &fixture.manager;
    Ok(AuthFlows::new(
        fixture.db.clone(),
        fixture.passwords.clone(),
        manager.tokens.clone(),
        RefreshTokens::new(fixture.db.clone()),
        VerificationCodes::new(
            DatabaseVerificationCodeStore::new(fixture.db.clone()),
            fixture.passwords.clone(),
        ),
        RateLimiter::new(DatabaseRateLimitStore::new(fixture.db.clone())),
    )
    .with_factor_authority(manager.authority.clone())?
    .with_factor_actions(manager.actions.clone()))
}

fn profile_api(fixture: &Fixture) -> Result<FortifyJwtApi> {
    let manager = &fixture.manager;
    Ok(FortifyJwtApi {
        scope: scope(&manager.tokens),
        flows: Arc::new(auth_flows(fixture)?),
        authority: Arc::new(manager.authority.clone()),
        provider: manager.provider.clone(),
        tokens: Arc::new(manager.tokens.clone()),
        callbacks: Arc::new(FortifyCallbacks::default()),
        config: FortifyConfig::default(),
        app_name: "测试".into(),
        rehash: false,
        timebox: std::time::Duration::ZERO,
    })
}

async fn password_login(fixture: &Fixture, endpoint: [u8; 4]) -> Result<String> {
    let api = profile_api(fixture)?;
    login_with_api(&api, endpoint).await
}

async fn login_with_api(api: &FortifyJwtApi, endpoint: [u8; 4]) -> Result<String> {
    let request = Request::post("/api/auth/login")
        .header("host", "localhost")
        .extension(ConnectInfo(std::net::SocketAddr::from((endpoint, 443))))
        .body(Body::empty())
        .map_err(Error::internal)?;
    let (mut parts, _) = request.into_parts();
    let request = RequestInfo::from_request_parts(&mut parts, &()).await?;
    let input = Credentials::new(Map::from_iter([
        ("email".into(), Value::from("management@example.test")),
        ("password".into(), Value::from(PASSWORD)),
    ]));
    match api.begin_login(&request, &input).await? {
        LoginAttempt::Authenticated(credentials) => Ok(credentials.access_token().to_owned()),
        LoginAttempt::TwoFactorRequired(_) => Err(Error::custom(
            "unexpected_pending_challenge",
            "待确认账户在启用确认选项时应能以正确密码登录",
        )),
    }
}

// 断言失败只报告比较结果，不把种子、恢复码或持有者绑定写进测试日志。
fn same_secret(left: &str, right: &str) -> bool {
    bool::from(subtle::ConstantTimeEq::ct_eq(
        left.as_bytes(),
        right.as_bytes(),
    ))
}

fn assert_denied<T>(result: Result<T>) {
    assert!(
        result.is_err_and(|error| matches!(
            error.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        )),
        "应因管理授权被拒绝"
    );
}

#[cfg(feature = "grpc")]
struct RpcAccountProvider {
    db: Db,
    manager: JwtApiFactorManagement,
    resolutions: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(feature = "grpc")]
impl crate::core::ServiceProvider for RpcAccountProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::core::RegisterContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(self.db.clone())).await?;
            app.instance(Arc::new(self.manager.encrypter.clone()))
                .await?;
            app.instance(Arc::new(self.manager.actions.clone())).await?;
            app.instance(Arc::new(RateLimiter::new(DatabaseRateLimitStore::new(
                self.db.clone(),
            ))))
            .await?;
            let provider = self.manager.provider.clone();
            let count = self.resolutions.clone();
            app.resolve::<crate::auth::AuthDrivers>().await?.provider(
                "rpc-accounts",
                move |_, _| {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let provider = provider.clone();
                    Box::pin(async move { Ok(provider) })
                },
            )
        })
    }
}

#[cfg(feature = "grpc")]
async fn rpc_profile_app(
    fixture: &Fixture,
    resolutions: Arc<std::sync::atomic::AtomicUsize>,
) -> Result<crate::App> {
    use figment::{Figment, providers::Serialized};
    let (issuer, audience) = fixture.manager.tokens.credential_domain();
    let settings = serde_json::json!({
        "auth": {
            "defaults": {"guard":"api", "passwords":"users"},
            "guards": {"api":{"driver":"jwt-api","provider":"users"}},
            "providers": {"users":{"driver":"rpc-accounts"}},
            "jwt": {"issuer":issuer,"audience":audience,"active_key_id":"test","keys":{"test":KEY}}
        },
        "fortify-api": {"guard":"api","prefix":"/auth"},
        "fortify-options": {"two-factor-authentication":{"confirm":true}}
    });
    let config = Figment::from(Serialized::defaults(
        crate::config::FrameworkConfig::default(),
    ))
    .merge(Serialized::defaults(settings));
    let mut builder = ProviderBuilder::new();
    builder
        .provider(crate::auth::AuthServiceProvider::new())
        .provider(RpcAccountProvider {
            db: fixture.db.clone(),
            manager: fixture.manager.clone(),
            resolutions,
        })
        .provider(crate::auth::JwtServiceProvider)
        .provider(crate::auth::AuthFlowsServiceProvider::new())
        .provider(crate::auth::fortify::FortifyJwtApiServiceProvider::new());
    crate::App::from_provider_runtime(builder.start(config).await?)
}

#[cfg(feature = "grpc")]
fn foreign_domain_access(
    issuer: &str,
    audience: &str,
    grant: JwtCredentialGrant,
) -> Result<String> {
    JwtTokens::new(JwtConfig::new(
        issuer,
        audience,
        "test",
        BTreeMap::from([("test".into(), KEY.into())]),
    ))?
    .issue_access_token_with_grant(42, grant)
}

#[cfg(feature = "grpc")]
#[tokio::test]
async fn tls_rpc_accepts_only_current_profile_credentials_and_projects_the_named_policy()
-> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    owned(true, |fixture| {
        Box::pin(async move {
            let resolutions = Arc::new(AtomicUsize::new(0));
            let app = rpc_profile_app(&fixture, resolutions.clone()).await?;
            let login_scope = app.state().container().scope()?;
            let api = login_scope.resolve::<FortifyJwtApi>().await?;
            let revoked = login_with_api(&api, [192, 0, 2, 40]).await?;
            assert_eq!(
                RefreshTokens::new(fixture.db.clone())
                    .revoke_all_for_user(42)
                    .await?,
                1
            );
            let valid = login_with_api(&api, [192, 0, 2, 41]).await?;
            let grant = api
                .tokens
                .verify_credential(&valid)?
                .grant
                .ok_or(Error::Unauthorized)?;
            let (issuer, audience) = api.tokens.credential_domain();
            let mut other_guard = api.scope.clone();
            other_guard.guard = "another-api".into();
            let mut other_provider = api.scope.clone();
            other_provider.provider = "another-users".into();
            let denied = [
                revoked,
                access(&fixture, Some(other_guard)).await?,
                access(&fixture, Some(other_provider)).await?,
                access(&fixture, None).await?,
                api.tokens.issue_access_token(42)?,
                foreign_domain_access("https://other-management.test", audience, grant.clone())?,
                foreign_domain_access(issuer, "other-management", grant)?,
            ];
            let policy = crate::auth::Policy::new(fixture.db.clone())
                .with_guard_scope("api", Some("users".into()));
            let role = policy.ensure_role("rpc-reader").await?;
            policy.grant_permission(role, "rpc:read").await?;
            policy.assign_role(42, role).await?;
            drop(api);
            drop(login_scope);
            let before_rpcs = resolutions.load(Ordering::SeqCst);
            let result = crate::grpc::jwt_profile_tests::verify_over_tls(
                app.state().clone(),
                policy,
                &valid,
                &denied,
            )
            .await;
            app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
                .await?;
            result?;
            assert_eq!(
                resolutions.load(Ordering::SeqCst) - before_rpcs,
                denied.len() + 1
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn pending_enrollment_rejects_other_families_before_and_after_enable() -> Result<()> {
    owned(true, |fixture| {
        Box::pin(async move {
            let manager = &fixture.manager;
            let device_a = [192, 0, 2, 1];
            let device_b = [192, 0, 2, 2];
            let owner_grant = manager
                .prepare(&password_login(&fixture, device_a).await?, PASSWORD)
                .await?;
            let old_grant = manager
                .prepare(&password_login(&fixture, device_b).await?, PASSWORD)
                .await?;
            assert!(!same_secret(
                &manager.decode(&owner_grant)?.family_id,
                &manager.decode(&old_grant)?.family_id
            ));

            let (owner, owner_actions) = manager.actions(&owner_grant).await?;
            let (other, old_actions) = manager.actions(&old_grant).await?;
            owner_actions.enable(&owner, false).await?;
            let secret = owner_actions.secret_key(&owner).await?;
            let codes = owner_actions.recovery_codes(&owner).await?;
            assert_eq!(codes.len(), 8);

            // B 即使在 A 改变代际前已取得动作，每次实际读写也要在当前用户锁下拒绝。
            assert_denied(old_actions.secret_key(&other).await);
            assert_denied(old_actions.recovery_codes(&other).await);
            assert_denied(old_actions.disable(&other).await);
            assert_denied(manager.actions(&old_grant).await);

            // B 使用真实密码重新登录，获得当前 Pending 代际的独立家族。
            let fresh_access = password_login(&fixture, device_b).await?;
            let verified = manager
                .authority
                .authenticate_access_token(&manager.tokens, &fresh_access)
                .await?;
            let CredentialDomain::Jwt { family_id, .. } = verified.domain() else {
                return Err(Error::Unauthorized);
            };
            let previous = manager.decode(&old_grant)?;
            assert!(!same_secret(family_id, &previous.family_id));
            let fresh_context = verified.context.as_ref().ok_or(Error::Unauthorized)?;
            assert!(!same_secret(
                &fresh_context.generation,
                &previous.context.generation
            ));
            match manager.prepare(&fresh_access, PASSWORD).await {
                Ok(grant) => assert_denied(manager.actions(&grant).await),
                Err(error) => assert_denied::<()>(Err(error)),
            }

            let (owner, owner_actions) = manager.actions(&owner_grant).await?;
            assert!(same_secret(
                owner_actions.secret_key(&owner).await?.as_str(),
                &secret
            ));
            assert!(
                owner_actions
                    .recovery_codes(&owner)
                    .await?
                    .iter()
                    .eq(codes.iter())
            );
            let code = crate::auth::totp::totp_code(
                &super::super::engine::decode_secret(&secret)?,
                100,
                6,
            )?;
            owner_actions.confirm(&owner, &code).await?;
            assert_denied(manager.actions(&owner_grant).await);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn pending_restart_replaces_secret_without_disclosing_previous_enrollment() -> Result<()> {
    owned(true, |fixture| {
        Box::pin(async move {
            let manager = &fixture.manager;
            let first_access = password_login(&fixture, [192, 0, 2, 1]).await?;
            let first_grant = manager.prepare(&first_access, PASSWORD).await?;
            let (first_user, first_actions) = manager.actions(&first_grant).await?;
            first_actions.enable(&first_user, false).await?;
            let first_secret = first_actions.secret_key(&first_user).await?;

            let replacement_access = password_login(&fixture, [192, 0, 2, 2]).await?;
            assert_denied(
                manager
                    .restart_pending(&replacement_access, "wrong-password")
                    .await,
            );
            assert!(same_secret(
                first_actions.secret_key(&first_user).await?.as_str(),
                &first_secret
            ));
            let replacement_grant = manager
                .restart_pending(&replacement_access, PASSWORD)
                .await?;
            let (user, actions) = manager.actions(&replacement_grant).await?;
            let secret = actions.secret_key(&user).await?;
            assert!(!same_secret(&secret, &first_secret));
            assert_eq!(actions.recovery_codes(&user).await?.len(), 8);
            assert_denied(manager.actions(&first_grant).await);
            assert_denied(first_actions.secret_key(&first_user).await);
            let code = crate::auth::totp::totp_code(
                &super::super::engine::decode_secret(&secret)?,
                100,
                6,
            )?;
            actions.confirm(&user, &code).await?;
            // 已确认的因子不能通过只证明密码的重开入口重置。
            assert_denied(manager.restart_pending(&replacement_access, PASSWORD).await);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn disabling_and_reenabling_do_not_restore_previous_management_grants() -> Result<()> {
    owned(true, |fixture| {
        Box::pin(async move {
            let manager = &fixture.manager;
            let endpoint = [192, 0, 2, 1];
            let original = manager
                .prepare(&password_login(&fixture, endpoint).await?, PASSWORD)
                .await?;
            let (user, original_actions) = manager.actions(&original).await?;
            original_actions.enable(&user, false).await?;
            let old_secret = original_actions.secret_key(&user).await?;
            original_actions.disable(&user).await?;
            assert_denied(manager.actions(&original).await);
            assert_denied(original_actions.enable(&user, false).await);

            let replacement = manager
                .prepare(&password_login(&fixture, endpoint).await?, PASSWORD)
                .await?;
            let (user, current_actions) = manager.actions(&replacement).await?;
            current_actions.enable(&user, false).await?;
            let new_secret = current_actions.secret_key(&user).await?;
            assert!(!same_secret(&old_secret, &new_secret));
            assert_denied(manager.actions(&original).await);
            assert_denied(original_actions.secret_key(&user).await);
            assert_denied(original_actions.disable(&user).await);
            assert!(same_secret(
                current_actions.secret_key(&user).await?.as_str(),
                &new_secret
            ));
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn management_grant_rejects_other_issuer_or_audience_with_shared_keys_and_database()
-> Result<()> {
    owned(true, |fixture| {
        Box::pin(async move {
            let manager = &fixture.manager;
            let grant = manager
                .prepare(&password_login(&fixture, [192, 0, 2, 1]).await?, PASSWORD)
                .await?;
            let (issuer, audience) = manager.tokens.credential_domain();
            for (foreign_issuer, foreign_audience) in [
                ("https://other-issuer.test", audience),
                (issuer, "other-audience"),
            ] {
                let mut other = manager.clone();
                other.tokens = JwtTokens::new(JwtConfig::new(
                    foreign_issuer,
                    foreign_audience,
                    "test",
                    BTreeMap::from([("test".into(), KEY.into())]),
                ))?;
                assert_denied(other.actions(&grant).await);
            }
            let (user, actions) = manager.actions(&grant).await?;
            actions.enable(&user, false).await?;
            assert_eq!(actions.recovery_codes(&user).await?.len(), 8);
            Ok(())
        })
    })
    .await
}

async fn session_parent_guard(
    fixture: &Fixture,
    store: Arc<dyn crate::session::SessionStore>,
) -> Result<(crate::session::Session, crate::auth::SessionGuard)> {
    use crate::auth::{SessionGuard, StatefulGuard};
    use crate::http::{CookieFactory, CookieJar};
    let session = crate::session::Session::new(None, store, None)?;
    session.start().await?;
    let guard = SessionGuard::new(
        "web".into(),
        fixture.manager.provider.clone(),
        session.clone(),
        CookieJar::new(CookieFactory::default()),
        None,
        None,
        None,
    )
    .with_provider_name(Some("users".into()));
    let input = Credentials::new(Map::from_iter([
        ("email".into(), Value::from("management@example.test")),
        ("password".into(), Value::from(PASSWORD)),
    ]));
    assert!(guard.attempt(&input, false).await?);
    Ok((session, guard))
}

#[tokio::test]
async fn session_exchange_rechecks_new_family_at_the_last_database_hook() -> Result<()> {
    use crate::db::{TransactionBeforeCommit, sea_orm::ConnectionTrait as _};
    use crate::session::MemorySessionStore;
    use std::sync::atomic::{AtomicBool, Ordering};

    let reject = Arc::new(AtomicBool::new(false));
    let listener = reject.clone();
    let mut events = EventsServiceProvider::new();
    events.listen::<TransactionBeforeCommit,_>(move |_, ctx| {
        let listener = listener.clone();
        Box::pin(async move {
            if listener.load(Ordering::SeqCst) {
                let tx = ctx.transaction().ok_or(Error::Unauthorized)?;
                let changed = tx.execute_unprepared("UPDATE auth_refresh_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE revoked_at IS NULL")
                    .await.map_err(Error::internal)?;
                if changed.rows_affected() == 1 { listener.store(false, Ordering::SeqCst); }
            }
            Ok(())
        })
    });
    owned_with_events(true, events, move |fixture| {
        Box::pin(async move {
            let flows = auth_flows(&fixture)?;
            let (_, guard) =
                session_parent_guard(&fixture, Arc::new(MemorySessionStore::default())).await?;
            let parent = fixture
                .manager
                .authority
                .prepare_session(&guard, "web")
                .await?;
            reject.store(true, Ordering::SeqCst);
            assert!(matches!(
                flows.issue_credentials_from(parent).await,
                Err(Error::Unauthorized)
            ));
            assert!(!reject.load(Ordering::SeqCst));
            assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
            let issued = flows.issue_credentials_from_session(&guard, "web").await?;
            assert!(
                fixture
                    .manager
                    .authority
                    .authenticate_access_token(&fixture.manager.tokens, issued.access_token())
                    .await
                    .is_ok()
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn session_exchange_rejects_request_rotation_during_login_notification() -> Result<()> {
    use crate::session::{MemorySessionStore, Session};
    let pending = Arc::new(std::sync::Mutex::new(None::<Session>));
    let pending_listener = pending.clone();
    let mut events = EventsServiceProvider::new();
    events.listen::<crate::auth::LoginSucceeded, _>(move |_, _| {
        let session = pending_listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        Box::pin(async move {
            if let Some(session) = session {
                session.invalidate().await?;
            }
            Ok(())
        })
    });
    owned_with_events(true, events, move |fixture| {
        Box::pin(async move {
            let (session, guard) =
                session_parent_guard(&fixture, Arc::new(MemorySessionStore::default())).await?;
            let original = session.id()?;
            let parent = fixture
                .manager
                .authority
                .prepare_session(&guard, "web")
                .await?;
            *pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(session.clone());
            assert!(matches!(
                auth_flows(&fixture)?.issue_credentials_from(parent).await,
                Err(Error::Unauthorized)
            ));
            assert_ne!(session.id()?, original);
            assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn another_request_deletes_saved_parent_without_changing_the_original_session_object()
-> Result<()> {
    use crate::session::{
        DatabaseSessionStore, GuardedDatabaseSessionStore, MemorySessionStore, Session,
        SessionStore,
    };
    owned(true, |fixture| {
        Box::pin(async move {
            let flows = auth_flows(&fixture)?;
            let stores: [Arc<dyn SessionStore>; 3] = [
                Arc::new(MemorySessionStore::default()),
                Arc::new(DatabaseSessionStore::new(fixture.db.clone())),
                Arc::new(GuardedDatabaseSessionStore::new(fixture.db.clone())),
            ];
            for store in stores {
                let (original, guard) = session_parent_guard(&fixture, store.clone()).await?;
                original.save().await?;
                let parent = fixture
                    .manager
                    .authority
                    .prepare_session(&guard, "web")
                    .await?;
                let id = original.id()?;
                let other = Session::new(Some(id.clone()), store, None)?;
                other.start().await?;
                assert_eq!(other.get_typed::<i64>(&guard.session_name())?, Some(42));
                other.invalidate().await?;
                assert_eq!(original.get_typed::<i64>(&guard.session_name())?, Some(42));
                assert_eq!(id, original.id()?);
                assert!(matches!(
                    flows.issue_credentials_from(parent).await,
                    Err(Error::Unauthorized)
                ));
                // 普通 ID 轮换不能把已撤销记录伪装为新验证的同请求登录。
                original.migrate(false).await?;
                assert!(matches!(
                    fixture
                        .manager
                        .authority
                        .prepare_session(&guard, "web")
                        .await,
                    Err(Error::Unauthorized)
                ));
            }
            assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn session_exchange_observes_database_session_deletion_inside_the_issuance_transaction()
-> Result<()> {
    use crate::db::{TransactionBeforeCommit, sea_orm::ConnectionTrait as _};
    use crate::session::{DatabaseSessionStore, GuardedDatabaseSessionStore, SessionStore};
    use std::sync::atomic::{AtomicBool, Ordering};
    let armed = Arc::new(AtomicBool::new(false));
    let gate = armed.clone();
    let mut events = EventsServiceProvider::new();
    events.listen::<TransactionBeforeCommit, _>(move |_, ctx| {
        let gate = gate.clone();
        Box::pin(async move {
            if gate.swap(false, Ordering::SeqCst) {
                let changed = ctx
                    .transaction()
                    .ok_or(Error::Unauthorized)?
                    .execute_unprepared("DELETE FROM sessions")
                    .await
                    .map_err(Error::internal)?;
                assert!(changed.rows_affected() > 0);
            }
            Ok(())
        })
    });
    owned_with_events(true, events, move |fixture| {
        Box::pin(async move {
            let flows = auth_flows(&fixture)?;
            let stores: [Arc<dyn SessionStore>; 2] = [
                Arc::new(DatabaseSessionStore::new(fixture.db.clone())),
                Arc::new(GuardedDatabaseSessionStore::new(fixture.db.clone())),
            ];
            for store in stores {
                let (session, guard) = session_parent_guard(&fixture, store).await?;
                session.save().await?;
                let parent = fixture
                    .manager
                    .authority
                    .prepare_session(&guard, "web")
                    .await?;
                let before = fixture.db.table("auth_refresh_tokens")?.count().await?;
                armed.store(true, Ordering::SeqCst);
                assert!(matches!(
                    flows.issue_credentials_from(parent).await,
                    Err(Error::Unauthorized)
                ));
                assert!(!armed.load(Ordering::SeqCst));
                assert_eq!(
                    fixture.db.table("auth_refresh_tokens")?.count().await?,
                    before
                );
                assert!(
                    flows
                        .issue_credentials_from_session(&guard, "web")
                        .await
                        .is_ok()
                );
            }
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn oidc_attachment_rechecks_saved_direct_login_session_at_commit() -> Result<()> {
    use crate::session::{MemorySessionStore, Session};
    owned(true, |fixture| {
        Box::pin(async move {
            let store = Arc::new(MemorySessionStore::default());
            let session = Session::new(None, store.clone(), None)?;
            session.start().await?;
            auth_flows(&fixture)?
                .login_session("management@example.test", PASSWORD, "attachment", &session)
                .await?;
            session.save().await?;
            let policy = fixture.manager.authority.clone();
            let prepared = policy
                .prepare_session_context_attachment(42, &session)
                .await?;
            let other = Session::new(Some(session.id()?), store, None)?;
            other.start().await?;
            other.invalidate().await?;
            let checked = fixture
                .db
                .transaction_for_update(move |tx| {
                    Box::pin(async move {
                        policy
                            .validate_attachment_in_transaction(tx, &prepared)
                            .await
                    })
                })
                .await;
            assert!(matches!(checked, Err(Error::Unauthorized)));
            Ok(())
        })
    })
    .await
}

#[path = "event_tests.rs"]
mod event_tests;

mod custom_parent_contract {
    use super::{Fixture, Migrator, PASSWORD, auth_flows, owned, session_parent_guard};
    use crate::{
        Error, Result,
        auth::{AuthUser, Credentials, Guard, StatefulGuard, UserProvider},
        core::BoxFuture,
        db::Db,
        session::{
            DatabaseSessionStore, GuardedDatabaseSessionStore, MemorySessionStore, Session,
            SessionId, SessionRecord, SessionStore, SessionWriteMode, SessionWriteOutcome,
        },
        testing::TestDatabase,
    };
    use serde_json::{Map, Value};
    use std::sync::Arc;
    use time::OffsetDateTime;

    // 这个驱动只实现外部契约的普通 load/write/delete，没有选择凭据派生 reader。
    #[derive(Default)]
    struct OrdinaryStore(MemorySessionStore);

    impl SessionStore for OrdinaryStore {
        fn load<'a>(
            &'a self,
            id: &'a SessionId,
            maximum: usize,
            empty: OffsetDateTime,
        ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
            self.0.load(id, maximum, empty)
        }
        fn write<'a>(
            &'a self,
            record: &'a SessionRecord,
            mode: SessionWriteMode,
            maximum: usize,
        ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
            self.0.write(record, mode, maximum)
        }
        fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
            self.0.delete(id)
        }
    }

    async fn rejected_before_and_after_save(
        fixture: &Fixture,
        store: Arc<dyn SessionStore>,
        expected: &str,
    ) -> Result<()> {
        let (session, guard) = session_parent_guard(fixture, store).await?;
        let policy = &fixture.manager.authority;
        assert!(
            policy
                .prepare_session(&guard, "web")
                .await
                .is_err_and(|e| e.code() == expected)
        );
        session.save().await?;
        assert!(
            policy
                .prepare_session(&guard, "web")
                .await
                .is_err_and(|e| e.code() == expected)
        );
        Ok(())
    }

    #[tokio::test]
    async fn parent_reader_support_and_database_identity_are_required_even_before_the_first_save()
    -> Result<()> {
        owned(true, |fixture| {
            Box::pin(async move {
                rejected_before_and_after_save(
                    &fixture,
                    Arc::new(OrdinaryStore::default()),
                    "session_credential_parent_reader_unsupported",
                )
                .await?;
                let foreign = TestDatabase::sqlite_file::<Migrator>().await?;
                let result: Result<()> = async {
                    let db = Db::default();
                    db.initialize(foreign.connection()?)?;
                    rejected_before_and_after_save(
                        &fixture,
                        Arc::new(DatabaseSessionStore::new(db.clone())),
                        "session_store_guarantee_unsupported",
                    )
                    .await?;
                    rejected_before_and_after_save(
                        &fixture,
                        Arc::new(GuardedDatabaseSessionStore::new(db)),
                        "session_store_guarantee_unsupported",
                    )
                    .await?;
                    Ok(())
                }
                .await;
                let closed = foreign.close().await;
                result?;
                closed?;
                assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 0);
                Ok(())
            })
        })
        .await
    }

    // 全部身份读写走公开 Session/Provider/Guard 接口，自定义实现没有框架私有转换 owner。
    struct PersistedGuard {
        session: Session,
        provider: Arc<dyn UserProvider>,
    }
    impl PersistedGuard {
        const KEY: &'static str = "member_login";
    }
    impl Guard for PersistedGuard {
        fn provider_name(&self) -> Option<&str> {
            Some("users")
        }
        fn user(&self) -> BoxFuture<'_, Result<Option<AuthUser>>> {
            Box::pin(async move {
                let Some(id) = self.session.get_json(Self::KEY)? else {
                    return Ok(None);
                };
                self.provider.retrieve_by_id(&id).await
            })
        }
        fn validate<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                let Some(user) = self.provider.retrieve_by_credentials(credentials).await? else {
                    return Ok(false);
                };
                self.provider
                    .validate_credentials(user.as_ref(), credentials)
                    .await
            })
        }
        fn has_user(&self) -> Result<bool> {
            self.session.has(&[Self::KEY])
        }
        fn set_user(&self, _: AuthUser) -> BoxFuture<'_, Result<()>> {
            Box::pin(async { Err(Error::Unauthorized) })
        }
        fn as_stateful(&self) -> Option<&dyn StatefulGuard> {
            Some(self)
        }
    }
    impl StatefulGuard for PersistedGuard {
        fn credential_parent_session(&self) -> Option<(&str, Session)> {
            Some(("members", self.session.clone()))
        }
        fn session_name(&self) -> Option<String> {
            Some(Self::KEY.into())
        }
        fn provider(&self) -> Arc<dyn UserProvider> {
            self.provider.clone()
        }
        fn attempt<'a>(
            &'a self,
            credentials: &'a Credentials,
            _: bool,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                let Some(user) = self.provider.retrieve_by_credentials(credentials).await? else {
                    return Ok(false);
                };
                if !self
                    .provider
                    .validate_credentials(user.as_ref(), credentials)
                    .await?
                {
                    return Ok(false);
                }
                self.login(user, false).await?;
                Ok(true)
            })
        }
        fn login(&self, user: AuthUser, _: bool) -> BoxFuture<'_, Result<()>> {
            Box::pin(async move {
                self.session.migrate(true).await?;
                self.session.put_typed(Self::KEY, user.auth_identifier()?)?;
                Ok(())
            })
        }
        fn once<'a>(&'a self, _: &'a Credentials) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }
        fn login_using_id<'a>(
            &'a self,
            _: &'a Value,
            _: bool,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Err(Error::Unauthorized) })
        }
        fn once_using_id<'a>(&'a self, _: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Err(Error::Unauthorized) })
        }
        fn via_remember(&self) -> Result<bool> {
            Ok(false)
        }
        fn logout(&self) -> BoxFuture<'_, Result<()>> {
            Box::pin(async move { self.session.invalidate().await.map(|_| ()) })
        }
    }

    #[tokio::test]
    async fn external_stateful_guard_can_exchange_only_its_actual_saved_identity() -> Result<()> {
        owned(true, |fixture| {
            Box::pin(async move {
                let store = Arc::new(MemorySessionStore::default());
                let session = Session::new(None, store.clone(), None)?;
                session.start().await?;
                let guard = PersistedGuard {
                    session: session.clone(),
                    provider: fixture.manager.provider.clone(),
                };
                let input = Credentials::new(Map::from_iter([
                    ("email".into(), Value::from("management@example.test")),
                    ("password".into(), Value::from(PASSWORD)),
                ]));
                assert!(guard.attempt(&input, false).await?);
                let policy = &fixture.manager.authority;
                assert!(matches!(
                    policy.prepare_session(&guard, "members").await,
                    Err(Error::Unauthorized)
                ));
                session.save().await?;
                assert!(matches!(
                    policy.prepare_session(&guard, "other").await,
                    Err(Error::Unauthorized)
                ));
                let flows = auth_flows(&fixture)?;
                let issued = flows
                    .issue_credentials_from_session(&guard, "members")
                    .await?;
                assert_eq!(
                    policy
                        .authenticate_access_token(&fixture.manager.tokens, issued.access_token())
                        .await?
                        .user_id(),
                    42
                );
                let prepared = policy.prepare_session(&guard, "members").await?;
                let other = Session::new(Some(session.id()?), store, None)?;
                other.start().await?;
                other.put_typed(PersistedGuard::KEY, "42")?;
                other.save().await?;
                assert!(matches!(
                    flows.issue_credentials_from(prepared).await,
                    Err(Error::Unauthorized)
                ));
                assert!(matches!(
                    policy.prepare_session(&guard, "members").await,
                    Err(Error::Unauthorized)
                ));
                assert_eq!(fixture.db.table("auth_refresh_tokens")?.count().await?, 1);
                Ok(())
            })
        })
        .await
    }
}
