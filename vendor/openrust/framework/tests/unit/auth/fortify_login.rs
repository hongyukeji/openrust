//! 实际 SessionGuard、注册 UserProvider、模型能力与事件组成的 Fortify 登录管线。
use super::*;
use crate::auth::AuthDrivers;
use crate::auth::Authenticatable;
use crate::auth::GenericUser;
use crate::auth::SessionGuard;
use crate::auth::UserProvider;
use crate::auth::fortify::GoogleTwoFactorAuthenticationProvider;
use crate::auth::fortify::TwoFactorAuthenticatable;
use crate::auth::fortify::TwoFactorOptions;
use crate::auth::fortify::TwoFactorState;
use crate::config::ConfigSources;
use crate::config::repository::ConfigRepository;
use crate::container::{ContainerBuilder, Scope};
use crate::encryption::ApplicationEncrypter;
use crate::events::Dispatcher;
use crate::events::EventsServiceProvider;
use crate::foundation::ProviderBuilder;
use crate::foundation::ProviderRuntime;
use crate::foundation::resources::ResourceOwner;
use crate::http::CookieFactory;
use crate::http::CookieJar;
use crate::http::axum::body::Body;
use crate::http::axum::extract::FromRequestParts;
use crate::http::axum::http::Request;
use crate::session::MemorySessionStore;
use serde_json::{Map, json};
#[cfg(feature = "db-sqlite")]
use sha2::{Digest as _, Sha256};
use std::sync::{
    Mutex, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const ID: &str = "tenant:user-42";
const RECOVERY: &str = "AAAAAAAAAA-BBBBBBBBBB";
fn fixture_error() -> Error {
    Error::custom("fortify_fixture_failed", "Fortify管线测试状态错误")
}

struct Account {
    attributes: GenericUser,
    state: RwLock<TwoFactorState>,
    write_mode: AtomicUsize,
    write_entered: tokio::sync::Notify,
    authority: Option<super::super::authority::DatabaseFactorAuthority>,
}
impl Authenticatable for Account {
    fn auth_identifier_name(&self) -> &str {
        self.attributes.auth_identifier_name()
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &str {
        self.attributes.auth_password_name()
    }
    fn auth_password(&self) -> Result<Option<String>> {
        self.attributes.auth_password()
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        self.attributes.remember_token_for_cookie()
    }
    fn set_remember_token(
        &self,
        token: Option<String>,
    ) -> crate::support::BoxFuture<'_, Result<()>> {
        self.attributes.set_remember_token(token)
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> crate::support::BoxFuture<'a, Result<Option<crate::auth::RememberTokenUpdate>>> {
        self.attributes
            .begin_remember_token_update(expected, replacement)
    }
    fn remember_token_name(&self) -> &str {
        self.attributes.remember_token_name()
    }
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        Some(self)
    }
}
impl TwoFactorAuthenticatable for Account {
    fn two_factor_authority(&self) -> Option<&super::super::authority::DatabaseFactorAuthority> {
        self.authority.as_ref()
    }
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        Box::pin(async { Ok(self.state.read().map_err(|_| fixture_error())?.clone()) })
    }
    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        expected: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            match self.write_mode.load(Ordering::SeqCst) {
                1 => return Err(Error::custom("model_write_failed", "模型保存失败")),
                2 => {
                    self.write_entered.notify_one();
                    std::future::pending::<()>().await;
                }
                _ => {}
            }
            let mut state = self.state.write().map_err(|_| fixture_error())?;
            if &*state != expected {
                return Ok(false);
            }
            *state = replacement.clone();
            Ok(true)
        })
    }
    fn two_factor_account_name(&self, _: &str) -> Result<Option<String>> {
        Ok(Some("username-only".into()))
    }
}
type ValidationPause = (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>);

struct AccountProvider {
    user: Arc<Account>,
    retrieved: AtomicUsize,
    validated: AtomicUsize,
    rehashed: AtomicUsize,
    by_id: AtomicUsize,
    error_mode: AtomicUsize,
    database: Option<crate::database::Db>,
    validation_pause: Mutex<Option<ValidationPause>>,
}
impl UserProvider for AccountProvider {
    fn uses_database_accounts(&self, db: &crate::database::Db, table: &str) -> Result<bool> {
        Ok(self
            .database
            .as_ref()
            .is_some_and(|bound| bound.same_handle(db))
            && table == "users")
    }

    fn retrieve_by_id<'a>(
        &'a self,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            self.by_id.fetch_add(1, Ordering::SeqCst);
            Ok(
                (identifier == &self.user.auth_identifier()?)
                    .then(|| self.user.clone() as AuthUser),
            )
        })
    }
    fn retrieve_by_token<'a>(
        &'a self,
        _: &'a Value,
        _: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async { Ok(None) })
    }
    fn update_remember_token<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            self.retrieved.fetch_add(1, Ordering::SeqCst);
            if self.error_mode.load(Ordering::SeqCst) == 1 {
                return Err(Error::custom("provider_read_failed", "账户读取失败"));
            }
            assert_eq!(
                credentials.attributes().len(),
                2,
                "只交给Provider已配置username和password"
            );
            Ok((credentials.get("handle") == Some(&json!("username-only")))
                .then(|| self.user.clone() as AuthUser))
        })
    }
    fn validate_credentials<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            self.validated.fetch_add(1, Ordering::SeqCst);
            let pause = self
                .validation_pause
                .lock()
                .map_err(|_| fixture_error())?
                .take();
            if let Some((entered, resume)) = pause {
                entered.notify_one();
                resume.notified().await;
            }
            Ok(credentials.get("password") == Some(&json!("correct")))
        })
    }
    fn rehash_password_if_required<'a>(
        &'a self,
        _: &'a dyn Authenticatable,
        _: &'a Credentials,
        _: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.rehashed.fetch_add(1, Ordering::SeqCst);
            if self.error_mode.load(Ordering::SeqCst) == 2 {
                return Err(Error::custom("provider_rehash_failed", "密码重哈希失败"));
            }
            Ok(())
        })
    }
}
#[derive(Default)]
struct Observed {
    events: Mutex<Vec<&'static str>>,
    failed_users: Mutex<Vec<Option<Value>>>,
    fail_listener: AtomicBool,
}
async fn events(observed: &Arc<Observed>) -> Result<(Dispatcher, ProviderRuntime)> {
    let mut provider = EventsServiceProvider::new();
    macro_rules! record {
        ($event:ty, $name:literal) => {{
            let observed = observed.clone();
            provider.listen::<$event, _>(move |_, _| {
                let observed = observed.clone();
                Box::pin(async move {
                    observed
                        .events
                        .lock()
                        .map_err(|_| fixture_error())?
                        .push($name);
                    Ok(())
                })
            });
        }};
    }
    record!(crate::auth::Attempting, "attempting");
    record!(crate::auth::Validated, "validated");
    record!(crate::auth::Login, "login");
    record!(crate::auth::Authenticated, "authenticated");
    record!(TwoFactorAuthenticationChallenged, "challenge");
    record!(RecoveryCodeReplaced, "recovery");
    record!(ValidTwoFactorAuthenticationCodeProvided, "valid-two-factor");
    record!(TwoFactorAuthenticationFailed, "failed-two-factor");
    let observed = observed.clone();
    provider.listen::<crate::auth::Failed, _>(move |event, _| {
        let observed = observed.clone();
        let user = event
            .user
            .as_ref()
            .map(|user| user.auth_identifier())
            .transpose();
        Box::pin(async move {
            observed
                .events
                .lock()
                .map_err(|_| fixture_error())?
                .push("failed");
            observed
                .failed_users
                .lock()
                .map_err(|_| fixture_error())?
                .push(user?);
            if observed.fail_listener.load(Ordering::SeqCst) {
                return Err(Error::custom(
                    "failed_listener_failed",
                    "失败事件监听器失败",
                ));
            }
            Ok(())
        })
    });
    let mut builder = ProviderBuilder::new();
    builder
        .provider(crate::events::EventServiceProvider)
        .provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    let dispatcher = (*runtime.container().resolve::<Dispatcher>().await?).clone();
    Ok((dispatcher, runtime))
}
struct Fixture {
    login: FortifyLogin,
    manager: Arc<AuthManager>,
    user: Arc<Account>,
    provider: Arc<AccountProvider>,
    session: Session,
    observed: Arc<Observed>,
    request: RequestInfo,
    cookies: CookieJar,
    encrypter: Arc<dyn ApplicationEncrypter>,
    _scope: Scope,
    _owner: ResourceOwner,
    _runtime: ProviderRuntime,
}
async fn build_manager(
    provider: Arc<AccountProvider>,
    session: Session,
    cookies: CookieJar,
    events: Dispatcher,
) -> Result<(Arc<AuthManager>, Scope, ResourceOwner)> {
    let drivers = AuthDrivers::default();
    let factory = provider;
    drivers.provider("account-fixture", move |_, _| {
        let provider = factory.clone();
        Box::pin(async move { Ok(provider as Arc<dyn UserProvider>) })
    })?;
    drivers.extend("session-fixture", |context| {
        Box::pin(async move {
            let provider_name = context.user_provider_name()?;
            let provider = context.user_provider().await?.ok_or_else(fixture_error)?;
            let session = context.resolver.resolve::<Session>().await?;
            let cookies = context.resolver.resolve::<CookieJar>().await?;
            let events = context.resolver.resolve::<Dispatcher>().await?;
            Ok(Arc::new(
                SessionGuard::new(
                    context.name,
                    provider,
                    (*session).clone(),
                    (*cookies).clone(),
                    None,
                    Some((*events).clone()),
                    None,
                )
                .with_provider_name(provider_name)
                .with_remember_cookie_protected(true),
            ) as Arc<dyn Guard>)
        })
    })?;
    let sources = ConfigSources::from_ordered_entries([(
            "auth",
            json!({"defaults":{"guard":"web","passwords":"users","provider":null},"guards":{"web":{"driver":"session-fixture","provider":"users"},"admin":{"driver":"session-fixture","provider":"admins"}},"providers":{"users":{"driver":"account-fixture"},"admins":{"driver":"account-fixture"}}}),
        )]).map_err(Error::internal)?;
    let owner = ResourceOwner::new();
    let mut builder = ContainerBuilder::new();
    builder.context(ConfigRepository::new(sources), owner.registrar());
    builder.instance(Arc::new(drivers));
    builder.instance(Arc::new(session.clone()));
    builder.instance(Arc::new(cookies.clone()));
    builder.instance(Arc::new(events.clone()));
    builder.scoped::<AuthManager, _>(|resolver| {
        Box::pin(async move {
            let drivers = resolver.resolve::<AuthDrivers>().await?;
            Ok(Arc::new(AuthManager::new(resolver, (*drivers).clone())))
        })
    });
    let container = builder.build();
    let scope = container.scope()?;
    let manager = scope.resolve::<AuthManager>().await?;
    Ok((manager, scope, owner))
}
impl Fixture {
    fn with_feature_enabled(mut self, enabled: bool) -> Self {
        self.login.actions = Arc::new((*self.login.actions).clone().with_feature_enabled(enabled));
        self
    }
    async fn new(options: TwoFactorOptions, has_secret: bool) -> Result<Self> {
        Self::with_authority(options, has_secret, None).await
    }
    async fn with_authority(
        options: TwoFactorOptions,
        has_secret: bool,
        authority: Option<(
            super::super::authority::DatabaseFactorAuthority,
            crate::database::Db,
        )>,
    ) -> Result<Self> {
        Self::with_authority_id(options, has_secret, authority, json!(ID)).await
    }
    async fn with_authority_id(
        options: TwoFactorOptions,
        has_secret: bool,
        authority: Option<(
            super::super::authority::DatabaseFactorAuthority,
            crate::database::Db,
        )>,
        identifier: Value,
    ) -> Result<Self> {
        let observed = Arc::new(Observed::default());
        let (events, runtime) = events(&observed).await?;
        let encrypter = crate::auth::fortify::test_field_encrypter(
            "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
        )?;
        let state = if has_secret {
            TwoFactorState {
                secret: Some(encrypter.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?),
                recovery_codes: Some(encrypter.encrypt_serialized_string(
                    &serde_json::to_vec(&[RECOVERY; 8]).map_err(Error::internal)?,
                )?),
                confirmed_at: None,
            }
        } else {
            TwoFactorState::default()
        };
        let user = Arc::new(Account {
            attributes: GenericUser::new(Map::from_iter([
                ("id".into(), identifier),
                ("handle".into(), json!("username-only")),
                ("password".into(), json!("stored-hash")),
            ])),
            state: RwLock::new(state),
            write_mode: AtomicUsize::new(0),
            write_entered: tokio::sync::Notify::new(),
            authority: authority.as_ref().map(|(authority, _)| authority.clone()),
        });
        let provider = Arc::new(AccountProvider {
            user: user.clone(),
            retrieved: AtomicUsize::new(0),
            validated: AtomicUsize::new(0),
            rehashed: AtomicUsize::new(0),
            by_id: AtomicUsize::new(0),
            error_mode: AtomicUsize::new(0),
            database: authority.map(|(_, db)| db),
            validation_pause: Mutex::new(None),
        });
        let session = Session::new(None, Arc::new(MemorySessionStore::default()), None)?;
        session.start().await?;
        let cookies = CookieJar::new(CookieFactory::default());
        let (manager, scope, owner) = build_manager(
            provider.clone(),
            session.clone(),
            cookies.clone(),
            events.clone(),
        )
        .await?;
        let actions = Arc::new(
            TwoFactorActions::new(
                Arc::new(
                    GoogleTwoFactorAuthenticationProvider::new(None)
                        .with_clock(Arc::new(|| Ok(3_000))),
                ),
                encrypter.clone(),
                events,
            )
            .with_options(options)
            .with_recovery_code_generator(Arc::new(|| Ok("CCCCCCCCCC-DDDDDDDDDD".into()))),
        );
        let login = FortifyLogin::new(&manager, "web", session.clone(), actions)
            .await?
            .with_username("handle");
        let request = Request::post("/login")
            .header("host", "localhost")
            .body(Body::empty())
            .map_err(Error::internal)?;
        let (mut parts, _) = request.into_parts();
        let request = RequestInfo::from_request_parts(&mut parts, &()).await?;
        Ok(Self {
            login,
            manager,
            user,
            provider,
            session,
            observed,
            request,
            cookies,
            encrypter,
            _scope: scope,
            _owner: owner,
            _runtime: runtime,
        })
    }
    fn credentials(password: &str) -> Credentials {
        Credentials::new(Map::from_iter([
            ("handle".into(), json!("username-only")),
            ("password".into(), json!(password)),
            ("ignored".into(), json!("not-a-query-condition")),
        ]))
    }
    fn trace(&self) -> Result<Vec<&'static str>> {
        Ok(self
            .observed
            .events
            .lock()
            .map_err(|_| fixture_error())?
            .clone())
    }
    async fn begin(&self, remember: bool) -> Result<FortifyLoginOutcome> {
        self.login
            .begin(&self.request, &Self::credentials("correct"), remember)
            .await
    }
}

#[tokio::test]
async fn challenged_first_factor_waits_and_rehashes_without_publishing_a_guard_identity()
-> Result<()> {
    let fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
    let old = fixture.session.id()?;
    let start = Instant::now();
    assert_eq!(
        fixture.begin(true).await?,
        FortifyLoginOutcome::TwoFactorChallenge
    );
    assert!(start.elapsed() >= Duration::from_millis(200));
    assert_eq!(fixture.session.id()?, old);
    assert_eq!(fixture.provider.retrieved.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.provider.rehashed.load(Ordering::SeqCst), 1);
    assert!(fixture.login.guard.user().await?.is_none());
    assert!(
        fixture
            .session
            .get_json(
                &fixture
                    .login
                    .guard
                    .as_stateful()
                    .ok_or_else(fixture_error)?
                    .session_name()
                    .ok_or_else(fixture_error)?
            )?
            .is_none()
    );
    assert_eq!(fixture.session.get_json("login.id")?, Some(json!(ID)));
    assert_eq!(
        fixture.session.get_json("login.remember")?,
        Some(json!(true))
    );
    assert!(fixture.cookies.get_queued_cookies()?.is_empty());
    assert_eq!(fixture.trace()?, ["challenge"]);
    Ok(())
}

#[tokio::test]
async fn non_challenged_pipeline_repeats_provider_work_and_uses_real_guard_attempt_events()
-> Result<()> {
    let fixture = Fixture::new(TwoFactorOptions::default(), false).await?;
    assert_eq!(
        fixture.begin(false).await?,
        FortifyLoginOutcome::Authenticated
    );
    assert_eq!(fixture.provider.retrieved.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.provider.validated.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.provider.rehashed.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.login.guard.id().await?, Some(json!(ID)));
    assert_eq!(
        fixture.trace()?,
        ["attempting", "validated", "login", "authenticated"]
    );
    Ok(())
}

#[tokio::test]
async fn disabled_feature_skips_redirect_even_for_an_enabled_account() -> Result<()> {
    let options = TwoFactorOptions::default();
    let fixture = Fixture::new(options, true)
        .await?
        .with_feature_enabled(false);
    assert_eq!(
        fixture.begin(false).await?,
        FortifyLoginOutcome::Authenticated
    );
    assert_eq!(fixture.provider.retrieved.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.provider.rehashed.load(Ordering::SeqCst), 1);
    assert!(fixture.session.get_json("login.id")?.is_none());
    assert_eq!(
        fixture.trace()?,
        ["attempting", "validated", "login", "authenticated"]
    );
    Ok(())
}

#[tokio::test]
async fn successful_custom_callback_runs_twice_only_when_redirect_does_not_challenge() -> Result<()>
{
    for has_secret in [false, true] {
        let mut fixture = Fixture::new(TwoFactorOptions::default(), has_secret).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let user: AuthUser = fixture.user.clone();
        fixture.login = fixture.login.with_authenticate_using(Arc::new(move |_, _| {
            let user = user.clone();
            let observed = observed.clone();
            Box::pin(async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Some(user))
            })
        }));
        let outcome = fixture.begin(false).await?;
        assert_eq!(calls.load(Ordering::SeqCst), if has_secret { 1 } else { 2 });
        assert_eq!(fixture.provider.retrieved.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.provider.rehashed.load(Ordering::SeqCst), 0);
        assert_eq!(
            outcome,
            if has_secret {
                FortifyLoginOutcome::TwoFactorChallenge
            } else {
                FortifyLoginOutcome::Authenticated
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn first_factor_errors_and_failed_listener_keep_the_default_timebox_and_real_user()
-> Result<()> {
    for mode in [0, 1, 2, 3] {
        let fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
        fixture
            .provider
            .error_mode
            .store(mode.min(2), Ordering::SeqCst);
        fixture
            .observed
            .fail_listener
            .store(mode == 3, Ordering::SeqCst);
        if mode == 3 {
            fixture.provider.error_mode.store(0, Ordering::SeqCst);
        }
        let credentials = Fixture::credentials(if mode == 0 || mode == 3 {
            "wrong"
        } else {
            "correct"
        });
        let start = Instant::now();
        let error = fixture
            .login
            .begin(&fixture.request, &credentials, false)
            .await
            .err()
            .ok_or_else(fixture_error)?;
        match mode {
            1 => assert_eq!(error.code(), "provider_read_failed"),
            2 => assert_eq!(error.code(), "provider_rehash_failed"),
            3 => assert_eq!(error.code(), "failed_listener_failed"),
            _ => {}
        }
        assert!(start.elapsed() >= Duration::from_millis(200));
        assert!(fixture.login.guard.user().await?.is_none());
        assert!(fixture.session.get_json("login.id")?.is_none());
        if mode == 0 || mode == 3 {
            assert_eq!(
                *fixture
                    .observed
                    .failed_users
                    .lock()
                    .map_err(|_| fixture_error())?,
                vec![Some(json!(ID))]
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn confirmation_option_and_php_falsey_secret_follow_redirect_source() -> Result<()> {
    for secret in ["", "0"] {
        let fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
        fixture
            .user
            .state
            .write()
            .map_err(|_| fixture_error())?
            .secret = Some(secret.into());
        assert_eq!(
            fixture.begin(false).await?,
            FortifyLoginOutcome::Authenticated
        );
    }
    let fixture = Fixture::new(
        TwoFactorOptions {
            confirm: true,
            ..TwoFactorOptions::default()
        },
        true,
    )
    .await?;
    assert_eq!(
        fixture.begin(false).await?,
        FortifyLoginOutcome::Authenticated
    );
    Ok(())
}

#[tokio::test]
async fn valid_totp_consumes_challenge_then_logs_in_same_guard_and_remembers() -> Result<()> {
    let fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
    fixture.begin(true).await?;
    let before = fixture.session.id()?;
    fixture.login.complete(Some("594318"), None).await?;
    assert!(fixture.session.get_json("login.id")?.is_none());
    assert!(fixture.session.get_json("login.remember")?.is_none());
    assert_ne!(fixture.session.id()?, before);
    assert_eq!(fixture.login.guard.id().await?, Some(json!(ID)));
    assert_eq!(
        fixture.session.get_json(
            &fixture
                .login
                .guard
                .as_stateful()
                .ok_or_else(fixture_error)?
                .session_name()
                .ok_or_else(fixture_error)?
        )?,
        Some(json!(ID))
    );
    assert!(
        fixture
            .cookies
            .get_queued_cookies()?
            .iter()
            .any(|cookie| cookie.name().starts_with("remember_web_"))
    );
    assert_eq!(
        fixture.trace()?,
        ["challenge", "valid-two-factor", "login", "authenticated"]
    );
    Ok(())
}

#[tokio::test]
async fn invalid_code_keeps_challenge_but_wrong_guard_or_provider_cannot_resolve_account()
-> Result<()> {
    let fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
    fixture.begin(false).await?;
    assert!(fixture.login.complete(Some("000000"), None).await.is_err());
    assert_eq!(fixture.session.get_json("login.id")?, Some(json!(ID)));
    assert!(fixture.login.guard.user().await?.is_none());
    let admin = FortifyLogin::new(
        &fixture.manager,
        "admin",
        fixture.session.clone(),
        fixture.login.actions.clone(),
    )
    .await?;
    assert!(admin.challenged_user().await?.is_none());
    let calls = fixture.provider.by_id.load(Ordering::SeqCst);
    fixture
        .session
        .put_typed("fortify.login.provider", "admins")?;
    assert!(fixture.login.challenged_user().await?.is_none());
    assert_eq!(fixture.provider.by_id.load(Ordering::SeqCst), calls);
    assert!(matches!(
        fixture.login.complete(Some("594318"), None).await,
        Err(Error::Unauthorized)
    ));
    Ok(())
}

#[tokio::test]
async fn recovery_has_priority_replaces_in_place_and_emits_before_login() -> Result<()> {
    let fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
    fixture.begin(false).await?;
    fixture
        .login
        .complete(Some("000000"), Some(RECOVERY))
        .await?;
    let state = fixture.user.two_factor_state().await?;
    let plaintext = fixture
        .encrypter
        .decrypt_serialized_string(state.recovery_codes.as_deref().ok_or_else(fixture_error)?)?;
    let codes: Vec<String> = serde_json::from_slice(&plaintext).map_err(Error::internal)?;
    assert_eq!(codes.len(), 8);
    assert!(codes.iter().all(|code| code == "CCCCCCCCCC-DDDDDDDDDD"));
    assert_eq!(
        fixture.trace()?,
        [
            "challenge",
            "recovery",
            "valid-two-factor",
            "login",
            "authenticated"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn recovery_write_error_or_cancellation_still_consumes_the_matched_challenge() -> Result<()> {
    for mode in [1, 2] {
        let fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
        fixture.begin(false).await?;
        fixture.user.write_mode.store(mode, Ordering::SeqCst);
        if mode == 1 {
            assert!(fixture.login.complete(None, Some(RECOVERY)).await.is_err());
        } else {
            let future = fixture.login.complete(None, Some(RECOVERY));
            tokio::pin!(future);
            tokio::time::timeout(Duration::from_secs(2), async {
                tokio::select! {
                    () = fixture.user.write_entered.notified() => Ok(()),
                    result = &mut future => { result?; Err(fixture_error()) }
                }
            })
            .await
            .map_err(Error::internal)??;
        }
        assert!(fixture.session.get_json("login.id")?.is_none());
        assert!(fixture.login.guard.user().await?.is_none());
        assert_eq!(fixture.trace()?, ["challenge"]);
    }
    Ok(())
}

#[tokio::test]
async fn callback_second_failure_cannot_reuse_first_factor_user_as_formal_identity() -> Result<()> {
    let mut fixture = Fixture::new(TwoFactorOptions::default(), false).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let user: AuthUser = fixture.user.clone();
    fixture.login = fixture.login.with_authenticate_using(Arc::new(move |_, _| {
        let user = user.clone();
        let count = count.clone();
        Box::pin(async move { Ok((count.fetch_add(1, Ordering::SeqCst) == 0).then_some(user)) })
    }));
    assert!(fixture.begin(false).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(fixture.login.guard.user().await?.is_none());
    assert_eq!(fixture.trace()?, ["failed"]);
    assert_eq!(
        *fixture
            .observed
            .failed_users
            .lock()
            .map_err(|_| fixture_error())?,
        vec![None]
    );
    Ok(())
}

#[tokio::test]
async fn rehash_can_be_disabled_for_redirect_and_confirmed_account_is_challenged() -> Result<()> {
    let mut fixture = Fixture::new(
        TwoFactorOptions {
            confirm: true,
            ..TwoFactorOptions::default()
        },
        true,
    )
    .await?;
    fixture
        .user
        .state
        .write()
        .map_err(|_| fixture_error())?
        .confirmed_at = Some(chrono::Utc::now());
    fixture.login = fixture.login.with_rehash_on_login(false);
    assert_eq!(
        fixture.begin(false).await?,
        FortifyLoginOutcome::TwoFactorChallenge
    );
    assert_eq!(fixture.provider.rehashed.load(Ordering::SeqCst), 0);
    assert!(fixture.login.guard.user().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn required_authority_cannot_be_disabled_by_feature_flag_or_custom_callback() -> Result<()> {
    for callback in [false, true] {
        let options = TwoFactorOptions::default();
        let mut fixture = Fixture::new(options, false)
            .await?
            .with_feature_enabled(false);
        fixture.login = fixture.login.with_factor_authority_required(true);
        if callback {
            let user = fixture.user.clone();
            fixture.login = fixture.login.with_authenticate_using(Arc::new(move |_, _| {
                let user = user.clone();
                Box::pin(async move { Ok(Some(user as AuthUser)) })
            }));
        }
        let error = fixture.begin(true).await.err().ok_or_else(fixture_error)?;
        assert_eq!(error.code(), "fortify_factor_authority_required");
        assert!(fixture.login.guard.user().await?.is_none());
        assert!(fixture.session.get_json("login.id")?.is_none());
        assert!(fixture.cookies.get_queued_cookies()?.is_empty());
        assert!(!fixture.trace()?.contains(&"login"));
    }
    Ok(())
}

#[cfg(feature = "db-sqlite")]
struct AuthorityEmptyMigrations;
#[cfg(feature = "db-sqlite")]
impl crate::database::sea_orm_migration::MigratorTrait for AuthorityEmptyMigrations {
    fn migrations() -> Vec<Box<dyn crate::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}
#[cfg(feature = "db-sqlite")]
async fn authority_database_case<F>(callback: F) -> Result<()>
where
    F: FnOnce(
        crate::database::Db,
        super::super::authority::DatabaseFactorAuthority,
    ) -> BoxFuture<'static, Result<()>>,
{
    authority_database_case_for(json!(ID), callback).await
}
#[cfg(feature = "db-sqlite")]
async fn authority_database_case_for<F>(identifier: Value, callback: F) -> Result<()>
where
    F: FnOnce(
        crate::database::Db,
        super::super::authority::DatabaseFactorAuthority,
    ) -> BoxFuture<'static, Result<()>>,
{
    use crate::database::sea_orm::ConnectionTrait;
    let database = crate::testing::TestDatabase::sqlite_file::<AuthorityEmptyMigrations>().await?;
    let db = crate::database::Db::default();
    let outcome = async {
        db.initialize(database.connection()?)?;
        let key_type = if identifier.is_number() { "INTEGER" } else { "TEXT" };
        db.connection()?.execute_unprepared(&format!("CREATE TABLE users (id {key_type} PRIMARY KEY, two_factor_secret TEXT, two_factor_recovery_codes TEXT, two_factor_confirmed_at datetime, two_factor_authority_source TEXT, two_factor_authority_generation TEXT, two_factor_consumed_codes TEXT, two_factor_management_session_hash TEXT, two_factor_management_generation TEXT, two_factor_management_authorized_at datetime)")).await.map_err(Error::internal)?;
        let id_value = match &identifier {
            Value::String(value) => value.clone().into(),
            Value::Number(value) => value.as_i64().ok_or_else(fixture_error)?.into(),
            _ => return Err(fixture_error()),
        };
        db.table("users")?.insert(vec![("id", id_value)]).await?;
        let authority = super::super::authority::DatabaseFactorAuthority::new(super::super::DatabaseTwoFactorState::new(db.clone(), "users", "id", identifier)?);
        callback(db.clone(), authority).await
    }.await;
    drop(db);
    let closed = database.close().await;
    outcome?;
    closed
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn database_authority_actions_challenge_consume_and_session_receipt_share_one_identity()
-> Result<()> {
    authority_database_case(|db, authority| {
        Box::pin(async move {
            let options = TwoFactorOptions::default();
            let mut fixture =
                Fixture::with_authority(options, false, Some((authority.clone(), db)))
                    .await?
                    .with_feature_enabled(false);
            fixture.login = fixture.login.with_factor_authority_required(true);
            let user: AuthUser = fixture.user.clone();
            let disabled = authority.load().await?;
            fixture.login.actions.enable(&user, false).await?;
            let enabled = authority.load().await?;
            assert_ne!(enabled.generation, disabled.generation);
            assert_eq!(
                fixture.begin(false).await?,
                FortifyLoginOutcome::TwoFactorChallenge
            );
            assert_eq!(
                fixture.session.get_json("fortify.login.authority.source")?,
                Some(json!(enabled.source))
            );
            fixture.login.actions = Arc::new(
                (*fixture.login.actions)
                    .clone()
                    .with_recovery_code_generator(Arc::new(|| Ok("EEEEEEEEEE-FFFFFFFFFF".into()))),
            );
            fixture
                .login
                .complete(None, Some("CCCCCCCCCC-DDDDDDDDDD"))
                .await?;
            let stateful = fixture
                .login
                .guard
                .as_stateful()
                .ok_or_else(fixture_error)?;
            let proof = stateful
                .session_factor_proof()
                .await?
                .ok_or_else(fixture_error)?;
            let current = authority.load().await?;
            assert!(proof.matches(
                &json!(ID),
                "web",
                Some("users"),
                &current.source,
                &current.generation
            ));
            assert_eq!(proof.method(), FactorVerificationMethod::RecoveryCode);
            fixture
                .login
                .actions
                .generate_new_recovery_codes(&user)
                .await?;
            assert_ne!(authority.load().await?.generation, current.generation);
            fixture.login.actions.disable(&user).await?;
            assert_eq!(
                authority.load().await?.status,
                super::super::authority::FactorAuthorityStatus::Disabled
            );
            Ok(())
        })
    })
    .await
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn authority_generation_change_invalidates_saved_challenge_without_login() -> Result<()> {
    authority_database_case(|db, authority| {
        Box::pin(async move {
            let mut fixture = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db)),
            )
            .await?;
            fixture.login = fixture.login.with_factor_authority_required(true);
            let user: AuthUser = fixture.user.clone();
            fixture.login.actions.enable(&user, false).await?;
            fixture.begin(false).await?;
            fixture.login.actions.enable(&user, true).await?;
            assert!(fixture.login.challenged_user().await?.is_none());
            assert!(
                fixture
                    .login
                    .complete(None, Some("CCCCCCCCCC-DDDDDDDDDD"))
                    .await
                    .is_err()
            );
            assert!(fixture.login.guard.user().await?.is_none());
            assert!(fixture.cookies.get_queued_cookies()?.is_empty());
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn confirm_receipt_uses_actual_committed_authority_snapshot() -> Result<()> {
    authority_database_case(|db, authority| {
        Box::pin(async move {
            let fixture = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db)),
            )
            .await?;
            let user: AuthUser = fixture.user.clone();
            fixture.login.actions.enable(&user, false).await?;
            let before = authority.load().await?;
            let mut pending = before.state.clone();
            pending.secret = Some(
                fixture
                    .encrypter
                    .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?,
            );
            authority
                .replace(
                    &before,
                    &pending,
                    super::super::authority::FactorAuthorityMutation::Enable,
                )
                .await?;
            let receipt = fixture
                .login
                .actions
                .confirm_with_receipt(
                    &user,
                    "594318",
                    "web",
                    Some("users"),
                    fixture.provider.as_ref(),
                )
                .await?
                .ok_or_else(fixture_error)?;
            let current = authority.load().await?;
            assert!(current.state.confirmed_at.is_some());
            let proof = receipt.into_proof();
            assert!(proof.matches(
                &json!(ID),
                "web",
                Some("users"),
                &current.source,
                &current.generation
            ));
            assert_eq!(proof.method(), FactorVerificationMethod::Totp);
            assert!(
                fixture
                    .login
                    .actions
                    .confirm(&user, "594318")
                    .await
                    .is_err()
            );
            let replayed = authority.load().await?;
            assert_eq!(replayed.state, current.state);
            assert_eq!(replayed.generation, current.generation);
            assert!(fixture.session.get_json("login.id")?.is_none());
            assert!(fixture.login.guard.user().await?.is_none());
            assert_eq!(
                fixture.begin(false).await?,
                FortifyLoginOutcome::TwoFactorChallenge
            );
            assert!(fixture.login.complete(Some("594318"), None).await.is_err());
            assert!(fixture.login.guard.user().await?.is_none());
            assert!(
                fixture
                    .login
                    .guard
                    .as_stateful()
                    .ok_or_else(fixture_error)?
                    .session_factor_proof()
                    .await?
                    .is_none()
            );
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn callback_second_selection_rechecks_authority_after_first_factor() -> Result<()> {
    authority_database_case(|db, authority| {
        Box::pin(async move {
            let options = TwoFactorOptions::default();
            let mut fixture = Fixture::with_authority(options, false, Some((authority, db)))
                .await?
                .with_feature_enabled(false);
            let user = fixture.user.clone();
            let actions = fixture.login.actions.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            fixture.login = fixture
                .login
                .with_factor_authority_required(true)
                .with_authenticate_using(Arc::new(move |_, _| {
                    let user = user.clone();
                    let actions = actions.clone();
                    let calls = calls.clone();
                    Box::pin(async move {
                        let user: AuthUser = user;
                        if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                            actions.enable(&user, false).await?;
                        }
                        Ok(Some(user))
                    })
                }));
            assert_eq!(
                fixture.begin(true).await?,
                FortifyLoginOutcome::TwoFactorChallenge
            );
            assert_eq!(observed.load(Ordering::SeqCst), 2);
            assert!(fixture.login.guard.user().await?.is_none());
            assert!(fixture.cookies.get_queued_cookies()?.is_empty());
            assert_eq!(fixture.session.get_json("login.id")?, Some(json!(ID)));
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
struct ConcurrentCodeProvider {
    inner: Arc<dyn super::super::TwoFactorAuthenticationProvider>,
    verified: Arc<AtomicUsize>,
    barrier: Arc<tokio::sync::Barrier>,
}
#[cfg(feature = "db-sqlite")]
impl super::super::TwoFactorAuthenticationProvider for ConcurrentCodeProvider {
    fn generate_secret_key(&self, length: usize) -> Result<String> {
        self.inner.generate_secret_key(length)
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String> {
        self.inner.qr_code_url(company, account, secret)
    }
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<super::super::TwoFactorCodeVerification>> {
        Box::pin(async move {
            let fact = self.inner.verify_with_context(secret, code).await?;
            assert!(fact.is_valid(), "两个无Cache验证须实际到达数据库消费前屏障");
            self.verified.fetch_add(1, Ordering::SeqCst);
            self.barrier.wait().await;
            Ok(fact)
        })
    }
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn concurrent_modern_code_without_cache_publishes_only_one_session_proof() -> Result<()> {
    authority_database_case(|db, authority| {
        Box::pin(async move {
            let mut first = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db.clone())),
            )
            .await?;
            let mut second = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db)),
            )
            .await?;
            let user: AuthUser = first.user.clone();
            first.login.actions.enable(&user, false).await?;
            let snapshot = authority.load().await?;
            let mut fixed = snapshot.state.clone();
            fixed.secret = Some(
                first
                    .encrypter
                    .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?,
            );
            authority
                .replace(
                    &snapshot,
                    &fixed,
                    super::super::authority::FactorAuthorityMutation::Enable,
                )
                .await?;
            let count = Arc::new(AtomicUsize::new(0));
            let provider: Arc<dyn super::super::TwoFactorAuthenticationProvider> =
                Arc::new(ConcurrentCodeProvider {
                    inner: first.login.actions.provider.clone(),
                    verified: count.clone(),
                    barrier: Arc::new(tokio::sync::Barrier::new(2)),
                });
            for fixture in [&mut first, &mut second] {
                let mut actions = (*fixture.login.actions).clone();
                actions.provider = provider.clone();
                fixture.login.actions = Arc::new(actions);
                assert_eq!(
                    fixture.begin(false).await?,
                    FortifyLoginOutcome::TwoFactorChallenge
                );
            }
            let (left, right) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(
                    first.login.complete(Some("594318"), None),
                    second.login.complete(Some("594318"), None)
                )
            })
            .await
            .map_err(Error::internal)?;
            assert_eq!(count.load(Ordering::SeqCst), 2);
            assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
            let left_proof = first
                .login
                .guard
                .as_stateful()
                .ok_or_else(fixture_error)?
                .session_factor_proof()
                .await?;
            let right_proof = second
                .login
                .guard
                .as_stateful()
                .ok_or_else(fixture_error)?
                .session_factor_proof()
                .await?;
            assert_eq!(
                usize::from(left_proof.is_some()) + usize::from(right_proof.is_some()),
                1
            );
            assert_eq!(left.is_ok(), left_proof.is_some());
            assert_eq!(right.is_ok(), right_proof.is_some());
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn management_context_keeps_initiator_access_without_granting_factor_proof() -> Result<()> {
    authority_database_case(|db, authority| {
        Box::pin(async move {
            let first = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db.clone())),
            )
            .await?;
            let other = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db)),
            )
            .await?;
            let user: AuthUser = first.user.clone();
            let stateful = first.login.guard.as_stateful().ok_or_else(fixture_error)?;
            stateful.login(user.clone(), false).await?;
            let context = stateful.factor_management_context().await?;
            let actions = first.login.actions.with_management_context(context, false);
            actions.enable(&user, false).await?;
            let enabled = authority.load().await?;
            assert!(!actions.secret_key(&user).await?.is_empty());
            assert_eq!(actions.recovery_codes(&user).await?.len(), 8);
            assert!(stateful.session_factor_proof().await?.is_none());
            // 同一已知Session可重取密封上下文，模拟DB已提交后HTTP任务取消、下次请求恢复管理。
            let resumed = first
                .login
                .actions
                .with_management_context(stateful.factor_management_context().await?, false);
            assert!(
                resumed
                    .qr_code_url(&user, "Company", "handle")
                    .await?
                    .is_some()
            );
            resumed.enable(&user, true).await?;
            let forced = authority.load().await?;
            assert_ne!(forced.generation, enabled.generation);
            assert!(!resumed.secret_key(&user).await?.is_empty());
            resumed.generate_new_recovery_codes(&user).await?;
            assert_ne!(authority.load().await?.generation, forced.generation);
            assert_eq!(resumed.recovery_codes(&user).await?.len(), 8);
            assert!(stateful.session_factor_proof().await?.is_none());
            let outsider = other.login.guard.as_stateful().ok_or_else(fixture_error)?;
            outsider.login(other.user.clone(), false).await?;
            let denied = other
                .login
                .actions
                .with_management_context(outsider.factor_management_context().await?, false);
            let other_user: AuthUser = other.user.clone();
            assert!(denied.secret_key(&other_user).await.is_err());
            assert!(denied.recovery_codes(&other_user).await.is_err());
            assert!(denied.enable(&other_user, true).await.is_err());
            assert!(
                denied
                    .generate_new_recovery_codes(&other_user)
                    .await
                    .is_err()
            );
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
async fn actual_provider_metadata_case(confirm: bool) -> Result<()> {
    authority_database_case(move |db, authority| {
        Box::pin(async move {
            let mut fixture = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db.clone())),
            )
            .await?;
            fixture.login = fixture.login.with_factor_authority_required(true);
            let user: AuthUser = fixture.user.clone();
            fixture.login.actions.enable(&user, false).await?;
            let snapshot = authority.load().await?;
            let encrypted = snapshot.state.secret.as_ref().ok_or_else(fixture_error)?;
            let secret =
                super::super::actions::decrypt_string(fixture.encrypter.as_ref(), encrypted)?;
            let decoded = super::super::engine::decode_secret(&secret)?;
            let code = crate::auth::totp::totp_code(&decoded, 102, 6)?;
            let mut actions = (*fixture.login.actions).clone();
            actions.options.window = Some(0);
            actions.provider = Arc::new(
                GoogleTwoFactorAuthenticationProvider::new(None)
                    .with_clock(Arc::new(|| Ok(3_000)))
                    .with_window(3)?,
            );
            fixture.login.actions = Arc::new(actions);
            let before = chrono::Utc::now().timestamp();
            if confirm {
                let receipt = fixture
                    .login
                    .actions
                    .confirm_with_receipt(
                        &user,
                        &code,
                        "web",
                        Some("users"),
                        fixture.provider.as_ref(),
                    )
                    .await?;
                assert!(receipt.is_some());
                assert!(authority.load().await?.state.confirmed_at.is_some());
            } else {
                assert_eq!(
                    fixture.begin(false).await?,
                    FortifyLoginOutcome::TwoFactorChallenge
                );
                fixture.login.complete(Some(&code), None).await?;
                assert!(
                    fixture
                        .login
                        .guard
                        .as_stateful()
                        .ok_or_else(fixture_error)?
                        .session_factor_proof()
                        .await?
                        .is_some()
                );
            }
            let row = db
                .table("users")?
                .where_eq("id", ID)?
                .first()
                .await?
                .ok_or_else(fixture_error)?;
            let encoded: String = row
                .try_get("", "two_factor_consumed_codes")
                .map_err(Error::internal)?;
            let ledger: Value = serde_json::from_str(&encoded).map_err(Error::internal)?;
            let hash: String = Sha256::digest(code.as_bytes())
                .iter()
                .flat_map(|byte| {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    [
                        char::from(HEX[usize::from(byte >> 4)]),
                        char::from(HEX[usize::from(byte & 15)]),
                    ]
                })
                .collect();
            let expires = ledger
                .get(&hash)
                .and_then(Value::as_i64)
                .ok_or_else(fixture_error)?;
            assert!(expires >= before + 270);
            assert!(expires <= chrono::Utc::now().timestamp() + 270);
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn session_login_uses_actual_provider_window_clock_and_nonce_ttl() -> Result<()> {
    actual_provider_metadata_case(false).await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn confirmation_uses_actual_provider_window_clock_and_nonce_ttl() -> Result<()> {
    actual_provider_metadata_case(true).await
}

#[cfg(feature = "db-sqlite")]
struct ConfigurationPausedCodeProvider {
    inner: Arc<dyn super::super::TwoFactorAuthenticationProvider>,
    entered: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}
#[cfg(feature = "db-sqlite")]
impl super::super::TwoFactorAuthenticationProvider for ConfigurationPausedCodeProvider {
    fn generate_secret_key(&self, length: usize) -> Result<String> {
        self.inner.generate_secret_key(length)
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> Result<String> {
        self.inner.qr_code_url(company, account, secret)
    }
    fn verify_with_context<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> BoxFuture<'a, Result<super::super::TwoFactorCodeVerification>> {
        Box::pin(async move {
            let fact = self.inner.verify_with_context(secret, code).await?;
            assert!(fact.is_valid());
            self.entered.notify_one();
            self.resume.notified().await;
            Ok(fact)
        })
    }
}
#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn pending_management_rechecks_confirmation_config_after_actual_provider_await() -> Result<()>
{
    authority_database_case(|db, authority| {
        Box::pin(async move {
            let fixture = Fixture::with_authority(
                TwoFactorOptions::default(),
                false,
                Some((authority.clone(), db.clone())),
            )
            .await?;
            let user: AuthUser = fixture.user.clone();
            let before = authority.load().await?;
            let pending = authority
                .replace(
                    &before,
                    &TwoFactorState {
                        secret: Some(
                            fixture
                                .encrypter
                                .encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?,
                        ),
                        recovery_codes: Some(fixture.encrypter.encrypt_serialized_string(b"[]")?),
                        confirmed_at: None,
                    },
                    super::super::FactorAuthorityMutation::Enable,
                )
                .await?;
            let stateful = fixture
                .login
                .guard
                .as_stateful()
                .ok_or_else(fixture_error)?;
            stateful.login(user.clone(), false).await?;
            let context = stateful.factor_management_context().await?;
            assert!(context.factor_proof().is_none());
            let repository = ConfigRepository::new(figment::Figment::new());
            repository.set("fortify.features", json!(["two-factor-authentication"]))?;
            repository.set(
                "fortify-options.two-factor-authentication.confirm",
                json!(true),
            )?;
            authority
                .clone()
                .with_confirmation_config(repository.clone())
                .authorize_management(&pending, &context, true)
                .await?;
            let entered = Arc::new(tokio::sync::Notify::new());
            let resume = Arc::new(tokio::sync::Notify::new());
            let mut actions = (*fixture.login.actions)
                .clone()
                .with_config_repository(repository.clone())
                .with_management_context(context, true);
            actions.provider = Arc::new(ConfigurationPausedCodeProvider {
                inner: actions.provider.clone(),
                entered: entered.clone(),
                resume: resume.clone(),
            });
            let confirming = Box::pin(actions.confirm(&user, "594318"));
            let change = async {
                entered.notified().await;
                repository.set(
                    "fortify-options.two-factor-authentication.confirm",
                    json!(false),
                )?;
                resume.notify_one();
                Ok::<_, Error>(())
            };
            let (result, changed) =
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    tokio::join!(confirming, change)
                })
                .await
                .map_err(Error::internal)?;
            changed?;
            let error = result.err().ok_or_else(fixture_error)?;
            assert_eq!(error.status(), http::StatusCode::FORBIDDEN);
            assert_eq!(authority.load().await?, pending);
            let row = db
                .table("users")?
                .where_eq("id", ID)?
                .first()
                .await?
                .ok_or_else(fixture_error)?;
            assert_eq!(
                row.try_get::<Option<String>>("", "two_factor_consumed_codes")
                    .map_err(Error::internal)?,
                None
            );
            Ok(())
        })
    })
    .await
}

async fn configuration_fixture(confirm: bool) -> Result<(Fixture, ConfigRepository)> {
    let mut fixture = Fixture::new(TwoFactorOptions::default(), true).await?;
    let repository = ConfigRepository::new(figment::Figment::new());
    repository.set("fortify.features", json!(["two-factor-authentication"]))?;
    repository.set(
        "fortify-options.two-factor-authentication.confirm",
        json!(confirm),
    )?;
    fixture.login.actions = Arc::new(
        (*fixture.login.actions)
            .clone()
            .with_config_repository(repository.clone()),
    );
    Ok((fixture, repository))
}
fn callback_pause(
    fixture: &mut Fixture,
    entered: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let user = fixture.user.clone();
    fixture.login.authenticate_using = Some(Arc::new(move |_, _| {
        let calls = calls.clone();
        let entered = entered.clone();
        let resume = resume.clone();
        let user = user.clone();
        Box::pin(async move {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                entered.notify_one();
                resume.notified().await;
            }
            Ok(Some(user as AuthUser))
        })
    }));
    observed
}
#[tokio::test]
async fn pending_login_reads_confirmation_after_callback_or_provider_wait_in_both_directions()
-> Result<()> {
    for callback in [true, false] {
        for initial_confirm in [true, false] {
            let (mut fixture, repository) = configuration_fixture(initial_confirm).await?;
            let entered = Arc::new(tokio::sync::Notify::new());
            let resume = Arc::new(tokio::sync::Notify::new());
            if callback {
                callback_pause(&mut fixture, entered.clone(), resume.clone());
            } else {
                *fixture
                    .provider
                    .validation_pause
                    .lock()
                    .map_err(|_| fixture_error())? = Some((entered.clone(), resume.clone()));
            }
            let running = fixture.begin(false);
            let change = async {
                entered.notified().await;
                repository.set(
                    "fortify-options.two-factor-authentication.confirm",
                    json!(!initial_confirm),
                )?;
                resume.notify_one();
                Ok::<_, Error>(())
            };
            let (outcome, changed) = tokio::time::timeout(Duration::from_secs(2), async {
                tokio::join!(running, change)
            })
            .await
            .map_err(Error::internal)?;
            changed?;
            let expected = if initial_confirm {
                FortifyLoginOutcome::TwoFactorChallenge
            } else {
                FortifyLoginOutcome::Authenticated
            };
            assert_eq!(outcome?, expected);
            assert_eq!(
                fixture.login.guard.user().await?.is_some(),
                !initial_confirm
            );
            assert_eq!(
                fixture.session.get_json("login.id")?.is_some(),
                initial_confirm
            );
        }
    }
    Ok(())
}
#[tokio::test]
async fn running_pipeline_keeps_selected_feature_stage_but_next_begin_reselects_features()
-> Result<()> {
    let (mut fixture, repository) = configuration_fixture(false).await?;
    let entered = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let calls = callback_pause(&mut fixture, entered.clone(), resume.clone());
    let running = fixture.begin(false);
    let change = async {
        entered.notified().await;
        repository.set("fortify.features", json!([]))?;
        resume.notify_one();
        Ok::<_, Error>(())
    };
    let (outcome, changed) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(running, change)
    })
    .await
    .map_err(Error::internal)?;
    changed?;
    assert_eq!(outcome?, FortifyLoginOutcome::TwoFactorChallenge);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(fixture.login.guard.user().await?.is_none());
    // 同一已建立实例，不重建Actions/Guard；下次begin不再选择Redirect阶段。
    assert_eq!(
        fixture.begin(false).await?,
        FortifyLoginOutcome::Authenticated
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}
