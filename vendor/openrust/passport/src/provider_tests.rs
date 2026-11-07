//! Provider 预集成测试草稿；仅容器与配置，不启动数据库或 HTTP 服务。

use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use figment::{Figment, providers::Serialized};
use openrust::{database::TransactionContext, foundation::ProviderBuilder};
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    ClientRecord, GrantAuthority,
    test_keys::{OTHER_PUBLIC, PRIVATE, PUBLIC},
};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// 仅供旧配对/错误顺序测试使用，不作为默认生产能力或兼容API。
struct TestPair {
    signer: OAuthAccessSigner,
    verifier: OAuthAccessVerifier,
}

impl TestPair {
    fn sign_at(
        &self,
        client: &str,
        token: &str,
        user: Option<&str>,
        scopes: &[String],
        now: u64,
        expires: u64,
    ) -> std::result::Result<String, crate::AccessSigningError> {
        self.signer
            .sign_at(client, token, user, scopes, now, expires)
    }

    fn verify_at(
        &self,
        token: &str,
        client: &str,
        now: u64,
    ) -> std::result::Result<crate::PassportAccessClaims, crate::AccessSigningError> {
        self.verifier.verify_at(token, client, now)
    }
}

async fn load_pair(
    directory: &Path,
    config: &PassportConfig,
    permissions_check: bool,
) -> Result<TestPair> {
    // 先走唯一生产显式pair预检，保留材料→mode→下一key→配对的旧错误优先级。
    validate_pair(directory, config, permissions_check).await?;
    Ok(TestPair {
        signer: load_signer(directory, config, permissions_check).await?,
        verifier: load_verifier(directory, config, permissions_check).await?,
    })
}

fn directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "passport-provider-candidate-{}",
        uuid::Uuid::new_v4()
    ))
}
fn source() -> TestResult<Figment> {
    Ok(Figment::from(Serialized::defaults(json!({
        "security": {"encryption": {"active_key_id": "provider-fixture", "keys": {"provider-fixture": STANDARD.encode([31; 32])}}},
        "passport": {
            "private_key": std::str::from_utf8(PRIVATE)?.replace('\n', "\\n"),
            "public_key": std::str::from_utf8(PUBLIC)?.replace('\n', "\\n")
        }
    }))))
}
fn builder() -> ProviderBuilder {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(DatabaseServiceProvider)
        .provider(HashServiceProvider::new())
        .provider(EncryptionServiceProvider);
    builder
}
fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + std::time::Duration::from_secs(1)
}

/// 仅供作用域解析验证，实际调用一律拒绝，不能作为生产权威。
struct RejectingAuthority;
impl<'tx> GrantAuthority<TransactionContext<'tx>> for RejectingAuthority {
    fn validate<'a>(
        &'a self,
        _db_context: &'a TransactionContext<'tx>,
        _context: &'a crate::GrantValidation<'a>,
        _binding: &'a crate::GrantAuthorityBinding,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            Err(Error::custom(
                "provider_fixture_authority_rejected",
                "测试权威禁止签发",
            ))
        })
    }
}
struct FixtureAuthorityProvider;
impl ServiceProvider for FixtureAuthorityProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.scoped::<DatabaseGrantAuthority, _>(|_| {
                Box::pin(async { Ok(Arc::new(RejectingAuthority) as Arc<DatabaseGrantAuthority>) })
            })
            .await
        })
    }
}

#[test]
fn official_defaults_and_sensitive_schema_do_not_disclose_pem() -> TestResult {
    let mut config = PassportConfig::default();
    assert_eq!(config.guard, "web");
    assert_eq!(config.middleware, [] as [String; 0]);
    assert_eq!(config.private_key, None);
    assert_eq!(config.public_key, None);
    assert_eq!(config.connection, None);
    config.private_key = Some("private-pem-secret-sentinel".into());
    config.public_key = Some("public-pem-sentinel".into());
    let debug = format!("{config:?}");
    assert!(!debug.contains("private-pem-secret-sentinel"));
    assert!(!debug.contains("public-pem-sentinel"));
    let SectionSchema::Object { fields, .. } = schema() else {
        return Err(Error::custom("provider_fixture_schema_invalid", "配置结构应为对象").into());
    };
    assert!(matches!(
        fields.get("private_key"),
        Some(SectionSchema::Sensitive(_))
    ));
    Ok(())
}

#[tokio::test]
async fn inline_keys_accept_official_literal_newline_conversion() -> TestResult {
    let config = PassportConfig {
        private_key: Some(std::str::from_utf8(PRIVATE)?.replace('\n', "\\n")),
        public_key: Some(std::str::from_utf8(PUBLIC)?.replace('\n', "\\n")),
        ..Default::default()
    };
    let signer = load_pair(&directory(), &config, true).await?;
    let token = signer.sign_at("client", "token", Some("user"), &[], 100, 200)?;
    assert_eq!(signer.verify_at(&token, "client", 100)?.sub, "user");
    Ok(())
}

#[tokio::test]
async fn key_read_failures_are_classified_without_paths_or_pem() -> TestResult {
    let path = directory().join("path-secret-sentinel");
    for inline in [None, Some(""), Some("0")] {
        let result = load_key(&path, inline, KeyKind::Private, true).await;
        let Err(error) = result else {
            return Err(
                Error::custom("provider_fixture_expected_rejection", "缺失密钥未拒绝").into(),
            );
        };
        assert_eq!(error.code(), "passport_private_key_missing");
        assert!(!format!("{error:?}").contains("path-secret-sentinel"));
        assert!(!error.to_string().contains("path-secret-sentinel"));
    }
    let error = load_key(&path, None, KeyKind::Public, true)
        .await
        .err()
        .ok_or_else(|| Error::custom("provider_fixture_expected_rejection", "缺失公钥未拒绝"))?;
    assert_eq!(error.code(), "passport_public_key_missing");
    let invalid = PassportConfig {
        private_key: Some("pem-secret-sentinel".into()),
        public_key: Some("invalid".into()),
        ..Default::default()
    };
    let error = load_pair(&path, &invalid, true)
        .await
        .err()
        .ok_or_else(|| Error::custom("provider_fixture_expected_rejection", "非法PEM未拒绝"))?;
    assert_eq!(error.code(), "passport_signing_key_pair_invalid");
    assert!(!format!("{error:?}").contains("pem-secret-sentinel"));
    assert!(
        load_pair(Path::new("relative-storage"), &invalid, true)
            .await
            .is_err()
    );
    Ok(())
}

fn assert_service_initialization_cause(error: &Error, expected: &str) {
    assert_eq!(error.code(), "service_initialization_failed");
    let mut cause = error;
    while let Error::ServiceInitialization { source, .. } = cause {
        cause = source.as_ref();
    }
    assert_eq!(cause.code(), expected);
}

#[tokio::test]
async fn missing_authority_and_dependency_are_not_silently_defaulted() -> TestResult {
    let mut missing_dependencies = ProviderBuilder::new();
    missing_dependencies.provider(PassportServiceProvider::new(directory()));
    assert!(missing_dependencies.start(source()?).await.is_err());
    let mut builder = builder();
    builder.provider(PassportServiceProvider::new(directory()));
    let runtime = builder.start(source()?).await?;
    let scope = runtime.container().scope()?;
    let result: TestResult = async {
        let error = scope
            .resolve::<DatabaseGrantRepository>()
            .await
            .err()
            .ok_or_else(|| {
                Error::custom(
                    "provider_fixture_expected_rejection",
                    "缺失权威未阻断真实授权仓储解析",
                )
            })?;
        assert_service_initialization_cause(&error, "passport_grant_authority_missing");
        let storage_error = scope
            .resolve::<dyn GrantStorage>()
            .await
            .err()
            .ok_or_else(|| {
                Error::custom(
                    "provider_fixture_expected_rejection",
                    "默认存储alias不应绕过权威",
                )
            })?;
        assert_service_initialization_cause(&storage_error, "passport_grant_authority_missing");
        Ok(())
    }
    .await;
    scope.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    result
}

#[tokio::test]
async fn config_merges_with_defaults_and_invalid_secret_types_remain_redacted() -> TestResult {
    let mut builder = builder();
    builder
        .provider(FixtureAuthorityProvider)
        .provider(PassportServiceProvider::new(directory()));
    let config = source()?.merge(Serialized::defaults(
        json!({"passport": {"guard": "staff", "middleware": ["auth:staff"]}}),
    ));
    let runtime = builder.start(config).await?;
    let config = runtime.config().section::<PassportConfig>("passport")?;
    assert_eq!(config.guard, "staff");
    assert_eq!(config.middleware, ["auth:staff"]);
    assert_eq!(config.connection, None);
    assert!(config.private_key.is_some());
    assert!(!format!("{:?}", runtime.config()).contains("BEGIN RSA PRIVATE KEY"));
    runtime.shutdown(deadline()).await?;
    for value in [json!(true), json!({"pem-secret-sentinel": "sensitive"})] {
        let mut builder = self::builder();
        builder
            .provider(FixtureAuthorityProvider)
            .provider(PassportServiceProvider::new(directory()));
        let config = source()?.merge(Serialized::defaults(
            json!({"passport": {"private_key": value}}),
        ));
        let error = match builder.start(config).await {
            Ok(runtime) => {
                runtime.shutdown(deadline()).await?;
                return Err(Error::custom(
                    "provider_fixture_expected_rejection",
                    "非法密钥配置类型未拒绝",
                )
                .into());
            }
            Err(error) => error,
        };
        assert!(!format!("{error:?}").contains("pem-secret-sentinel"));
        assert!(!error.to_string().contains("sensitive"));
    }
    Ok(())
}

#[tokio::test]
async fn aliases_share_client_repository_but_authority_and_server_are_scoped() -> TestResult {
    let mut builder = builder();
    builder.provider(FixtureAuthorityProvider).provider(
        PassportServiceProvider::new(directory())
            .with_grants(GrantConfig {
                revoke_refresh_tokens: false,
                ..Default::default()
            })
            .with_user_key(GrantUserKey::String)
            .with_scopes(ScopeRepository::new([crate::Scope {
                id: "read".into(),
                description: "读取".into(),
            }])),
    );
    let runtime = builder.start(source()?).await?;
    let concrete = runtime.container().resolve::<ClientRepository>().await?;
    let alias = runtime.container().resolve::<dyn ClientLookup>().await?;
    let expected: Arc<dyn ClientLookup> = concrete;
    assert!(Arc::ptr_eq(&expected, &alias));
    assert!(
        !runtime
            .container()
            .resolve::<GrantConfig>()
            .await?
            .revoke_refresh_tokens
    );
    assert_eq!(
        *runtime.container().resolve::<GrantUserKey>().await?,
        GrantUserKey::String
    );
    assert!(
        runtime
            .container()
            .resolve::<ScopeRepository>()
            .await?
            .has_scope("read")
    );
    let left = runtime.container().scope()?;
    let right = runtime.container().scope()?;
    let left_authority = left.resolve::<DatabaseGrantAuthority>().await?;
    assert!(Arc::ptr_eq(
        &left_authority,
        &left.resolve::<DatabaseGrantAuthority>().await?
    ));
    assert!(!Arc::ptr_eq(
        &left_authority,
        &right.resolve::<DatabaseGrantAuthority>().await?
    ));
    let left_repository = left.resolve::<DatabaseGrantRepository>().await?;
    assert!(Arc::ptr_eq(
        &left_repository,
        &left.resolve::<DatabaseGrantRepository>().await?
    ));
    assert!(!Arc::ptr_eq(
        &left_repository,
        &right.resolve::<DatabaseGrantRepository>().await?
    ));
    let left_storage = left.resolve::<dyn GrantStorage>().await?;
    let expected_left: Arc<dyn GrantStorage> = left_repository.clone();
    assert!(Arc::ptr_eq(&expected_left, &left_storage));
    let right_storage = right.resolve::<dyn GrantStorage>().await?;
    let expected_right: Arc<dyn GrantStorage> = right.resolve::<DatabaseGrantRepository>().await?;
    assert!(Arc::ptr_eq(&expected_right, &right_storage));
    assert!(!Arc::ptr_eq(&left_storage, &right_storage));
    let server = left.resolve::<AuthorizationServer>().await?;
    assert!(Arc::ptr_eq(
        &server,
        &left.resolve::<AuthorizationServer>().await?
    ));
    assert!(!Arc::ptr_eq(
        &server,
        &right.resolve::<AuthorizationServer>().await?
    ));
    left.shutdown(deadline()).await?;
    right.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    Ok(())
}

struct CustomLookup;
impl ClientLookup for CustomLookup {
    fn find<'a>(
        &'a self,
        _scope: &'a crate::ClientLookupContext<'a>,
        _identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async { Ok(None) })
    }
}
struct ApplicationBindings {
    factory: Arc<dyn ClientModelFactory>,
    lookup: Arc<dyn ClientLookup>,
    signer: Arc<OAuthAccessSigner>,
    verifier: Arc<OAuthAccessVerifier>,
}
impl ServiceProvider for ApplicationBindings {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance::<dyn ClientModelFactory>(self.factory.clone())
                .await?;
            app.instance::<dyn ClientLookup>(self.lookup.clone())
                .await?;
            app.instance(self.signer.clone()).await?;
            app.instance(self.verifier.clone()).await
        })
    }
}

#[tokio::test]
async fn application_trait_and_signer_overrides_are_preserved_without_file_reads() -> TestResult {
    let factory: Arc<dyn ClientModelFactory> = Arc::new(ClientFactory::default());
    let lookup: Arc<dyn ClientLookup> = Arc::new(CustomLookup);
    let signer = Arc::new(OAuthAccessSigner::from_pem(PRIVATE)?);
    let verifier = Arc::new(OAuthAccessVerifier::from_pem(PUBLIC)?);
    let mut builder = builder();
    builder
        .provider(ApplicationBindings {
            factory: factory.clone(),
            lookup: lookup.clone(),
            signer: signer.clone(),
            verifier: verifier.clone(),
        })
        .provider(FixtureAuthorityProvider)
        .provider(PassportServiceProvider::new(directory()));
    let config = Figment::from(Serialized::defaults(json!({
        "security": {"encryption": {"active_key_id": "provider-fixture", "keys": {"provider-fixture": STANDARD.encode([31; 32])}}}
    })));
    let runtime = builder.start(config).await?;
    assert!(Arc::ptr_eq(
        &factory,
        &runtime
            .container()
            .resolve::<dyn ClientModelFactory>()
            .await?
    ));
    assert!(Arc::ptr_eq(
        &lookup,
        &runtime.container().resolve::<dyn ClientLookup>().await?
    ));
    assert!(Arc::ptr_eq(
        &signer,
        &runtime.container().resolve::<OAuthAccessSigner>().await?
    ));
    assert!(Arc::ptr_eq(
        &verifier,
        &runtime.container().resolve::<OAuthAccessVerifier>().await?
    ));
    runtime.shutdown(deadline()).await?;
    Ok(())
}

#[tokio::test]
async fn default_file_keys_and_inline_key_can_be_combined() -> TestResult {
    let directory = directory();
    tokio::fs::create_dir(&directory).await?;
    let result: TestResult = async {
        tokio::fs::write(directory.join("oauth-private.key"), PRIVATE).await?;
        tokio::fs::write(directory.join("oauth-public.key"), PUBLIC).await?;
        #[cfg(unix)]
        set_key_modes(&directory, 0o600, 0o600).await?;
        let config = PassportConfig::default();
        let signer = load_pair(&directory, &config, true).await?;
        let token = signer.sign_at("client", "token", None, &[], 100, 200)?;
        assert!(signer.verify_at(&token, "client", 100).is_ok());
        let config = PassportConfig {
            private_key: Some(std::str::from_utf8(PRIVATE)?.to_owned()),
            ..Default::default()
        };
        let signer = load_pair(&directory, &config, true).await?;
        assert!(signer.verify_at(&token, "client", 100).is_ok());
        Ok(())
    }
    .await;
    // 只回收本测试显式创建的两个密钥文件与空目录，不按前缀扫描或递归删除。
    let private_cleanup = tokio::fs::remove_file(directory.join("oauth-private.key")).await;
    let public_cleanup = tokio::fs::remove_file(directory.join("oauth-public.key")).await;
    let directory_cleanup = tokio::fs::remove_dir(&directory).await;
    result?;
    private_cleanup?;
    public_cleanup?;
    directory_cleanup?;
    Ok(())
}

#[tokio::test]
async fn default_client_chain_is_shared_and_consumes_only_request_free_factory() -> TestResult {
    let mut builder = builder();
    builder
        .provider(FixtureAuthorityProvider)
        .provider(PassportServiceProvider::new(directory()));
    let runtime = builder.start(source()?).await?;
    let left = runtime.container().scope()?;
    let right = runtime.container().scope()?;
    assert!(Arc::ptr_eq(
        &left.resolve::<dyn ClientModelFactory>().await?,
        &right.resolve::<dyn ClientModelFactory>().await?
    ));
    assert!(Arc::ptr_eq(
        &left.resolve::<ClientRepository>().await?,
        &right.resolve::<ClientRepository>().await?
    ));
    assert!(Arc::ptr_eq(
        &left.resolve::<dyn ClientLookup>().await?,
        &right.resolve::<dyn ClientLookup>().await?
    ));
    let bridge = left.resolve::<BridgeClientRepository>().await?;
    assert!(Arc::ptr_eq(
        &bridge,
        &right.resolve::<BridgeClientRepository>().await?
    ));
    // 真实消费Bridge→Lookup→Repository→默认Factory；非法UUID在Factory层拒绝，
    // 因此此容器测试不读取数据库，也不把它宣称为实际查询缓存隔离证据。
    assert!(
        bridge
            .get_client_entity(&left, "not-a-client-uuid")
            .await?
            .is_none()
    );
    assert!(
        bridge
            .get_client_entity(&right, "not-a-client-uuid")
            .await?
            .is_none()
    );
    left.shutdown(deadline()).await?;
    right.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    Ok(())
}

struct ObservedFactory {
    base: ClientFactory,
    checks: std::sync::atomic::AtomicUsize,
}
impl ClientModelFactory for ObservedFactory {
    fn metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
        self.base.metadata()
    }
    fn is_valid_identifier(&self, _identifier: &Value) -> bool {
        self.checks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // 只验证真实消费链的租户工厂身份与状态隔离，不启动SQL。
        false
    }
    fn new_from_builder(
        &self,
        attributes: serde_json::Map<String, Value>,
    ) -> Result<Arc<dyn crate::ClientModel>> {
        self.base.new_from_builder(attributes)
    }
}
struct ScopedClientModels;
impl ServiceProvider for ScopedClientModels {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.scoped::<ObservedFactory, _>(|_| {
                Box::pin(async {
                    Ok(Arc::new(ObservedFactory {
                        base: ClientFactory::default(),
                        checks: std::sync::atomic::AtomicUsize::new(0),
                    }))
                })
            })
            .await?;
            app.scoped::<dyn ClientModelFactory, _>(|resolver| {
                Box::pin(async move {
                    let factory: Arc<dyn ClientModelFactory> =
                        resolver.resolve::<ObservedFactory>().await?;
                    Ok(factory)
                })
            })
            .await
        })
    }
}

#[tokio::test]
async fn default_client_chain_preserves_captive_rejection_for_scoped_factory() -> TestResult {
    let mut builder = builder();
    builder
        .provider(FixtureAuthorityProvider)
        .provider(ScopedClientModels)
        .provider(PassportServiceProvider::new(directory()));
    let runtime = builder.start(source()?).await?;
    let scope = runtime.container().scope()?;
    let resolution = scope.resolve::<BridgeClientRepository>().await;
    let Err(error) = resolution else {
        return Err(Error::custom(
            "provider_fixture_expected_rejection",
            "共享仓储错误捕获了请求级Factory",
        )
        .into());
    };
    assert_eq!(error.code(), "captive_scope_dependency");
    scope.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    Ok(())
}

#[tokio::test]
async fn explicit_scoped_client_chain_consumes_its_own_tenant_factory() -> TestResult {
    let mut builder = builder();
    builder
        .provider(FixtureAuthorityProvider)
        .provider(ScopedClientModels)
        .provider(PassportServiceProvider::new(directory()).with_scoped_client_models());
    let runtime = builder.start(source()?).await?;
    let left = runtime.container().scope()?;
    let right = runtime.container().scope()?;
    let left_factory = left.resolve::<ObservedFactory>().await?;
    let right_factory = right.resolve::<ObservedFactory>().await?;
    assert!(!Arc::ptr_eq(&left_factory, &right_factory));
    assert!(!Arc::ptr_eq(
        &left.resolve::<ClientRepository>().await?,
        &right.resolve::<ClientRepository>().await?
    ));
    assert!(!Arc::ptr_eq(
        &left.resolve::<dyn ClientLookup>().await?,
        &right.resolve::<dyn ClientLookup>().await?
    ));
    let left_bridge = left.resolve::<BridgeClientRepository>().await?;
    let right_bridge = right.resolve::<BridgeClientRepository>().await?;
    assert!(!Arc::ptr_eq(&left_bridge, &right_bridge));
    assert!(Arc::ptr_eq(
        &left_bridge,
        &left.resolve::<BridgeClientRepository>().await?
    ));
    assert!(
        left_bridge
            .get_client_entity(&left, "tenant-owned-client")
            .await?
            .is_none()
    );
    assert_eq!(
        left_factory
            .checks
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        right_factory
            .checks
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(
        right_bridge
            .get_client_entity(&right, "tenant-owned-client")
            .await?
            .is_none()
    );
    assert_eq!(
        left_factory
            .checks
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        right_factory
            .checks
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    // 同一 scoped Factory 也可供Grant仓储解析；无默认权威，仍来自显式拒绝夹具。
    assert!(left.resolve::<DatabaseGrantRepository>().await.is_ok());
    assert!(left.resolve::<AuthorizationServer>().await.is_ok());
    left.shutdown(deadline()).await?;
    right.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    Ok(())
}

struct PreservedValues {
    repository: Arc<ClientRepository>,
    bridge: Arc<BridgeClientRepository>,
}
struct PreservedRepositories {
    values: Arc<std::sync::OnceLock<PreservedValues>>,
}
impl ServiceProvider for PreservedRepositories {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let databases = app.resolve::<Databases>().await?;
            let hashing = app.resolve::<HashManager>().await?;
            let factory: Arc<dyn ClientModelFactory> = Arc::new(ClientFactory::default());
            let repository = Arc::new(ClientRepository::new(&databases, None, factory)?);
            let lookup: Arc<dyn ClientLookup> = repository.clone();
            let bridge = Arc::new(BridgeClientRepository::new(lookup, hashing));
            app.instance(repository.clone()).await?;
            app.instance(bridge.clone()).await?;
            // 提前实际解析；后续显式scoped选项也不得悄悄替换已绑定对象。
            assert!(Arc::ptr_eq(
                &repository,
                &app.resolve::<ClientRepository>().await?
            ));
            assert!(Arc::ptr_eq(
                &bridge,
                &app.resolve::<BridgeClientRepository>().await?
            ));
            self.values
                .set(PreservedValues { repository, bridge })
                .map_err(|_| {
                    Error::custom("provider_fixture_already_registered", "测试仓储已登记")
                })?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn scoped_option_preserves_application_repository_and_bridge_instances() -> TestResult {
    let values = Arc::new(std::sync::OnceLock::new());
    let mut builder = builder();
    builder
        .provider(FixtureAuthorityProvider)
        .provider(PreservedRepositories {
            values: values.clone(),
        })
        .provider(PassportServiceProvider::new(directory()).with_scoped_client_models());
    let runtime = builder.start(source()?).await?;
    let PreservedValues { repository, bridge } = values
        .get()
        .ok_or_else(|| Error::custom("provider_fixture_binding_missing", "测试仓储未登记"))?;
    let left = runtime.container().scope()?;
    let right = runtime.container().scope()?;
    assert!(Arc::ptr_eq(
        repository,
        &left.resolve::<ClientRepository>().await?
    ));
    assert!(Arc::ptr_eq(
        repository,
        &right.resolve::<ClientRepository>().await?
    ));
    assert!(Arc::ptr_eq(
        bridge,
        &left.resolve::<BridgeClientRepository>().await?
    ));
    assert!(Arc::ptr_eq(
        bridge,
        &right.resolve::<BridgeClientRepository>().await?
    ));
    assert!(
        bridge
            .get_client_entity(&left, "not-a-client-uuid")
            .await?
            .is_none()
    );
    assert!(
        bridge
            .get_client_entity(&right, "not-a-client-uuid")
            .await?
            .is_none()
    );
    left.shutdown(deadline()).await?;
    right.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    Ok(())
}

async fn permission_fixture() -> TestResult<PathBuf> {
    let path = directory();
    tokio::fs::create_dir(&path).await?;
    tokio::fs::write(path.join("oauth-private.key"), PRIVATE).await?;
    tokio::fs::write(path.join("oauth-public.key"), PUBLIC).await?;
    #[cfg(unix)]
    set_key_modes(&path, 0o600, 0o600).await?;
    Ok(path)
}
#[cfg(unix)]
async fn set_key_modes(path: &Path, private: u32, public: u32) -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;
    for (name, mode) in [("oauth-private.key", private), ("oauth-public.key", public)] {
        tokio::fs::set_permissions(path.join(name), std::fs::Permissions::from_mode(mode)).await?;
    }
    Ok(())
}
async fn remove_permission_fixture(path: &Path, extra: &[&str]) -> TestResult {
    let mut first_error = None;
    for name in ["oauth-private.key", "oauth-public.key"]
        .into_iter()
        .chain(extra.iter().copied())
    {
        if let Err(error) = tokio::fs::remove_file(path.join(name)).await
            && error.kind() != ErrorKind::NotFound
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    if let Err(error) = tokio::fs::remove_dir(path).await
        && first_error.is_none()
    {
        first_error = Some(error);
    }
    if let Some(error) = first_error {
        return Err(error.into());
    }
    Ok(())
}
fn signing_failure<T>(result: Result<T>) -> TestResult<Error> {
    result.err().ok_or_else(|| {
        Error::custom("provider_fixture_expected_rejection", "密钥反例未拒绝").into()
    })
}

#[test]
fn key_permission_switch_is_compiled_default_true_without_new_config_field() -> TestResult {
    let provider = PassportServiceProvider::new(directory());
    assert!(provider.key_permissions_check);
    assert!(
        !provider
            .with_key_permissions_check(false)
            .key_permissions_check
    );
    assert!(
        serde_json::to_value(PassportConfig::default())?
            .get("key_permissions_check")
            .is_none()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn unix_key_mode_mask_matches_exact_fixed_allowed_set() {
    for mode in [0o400, 0o440, 0o600, 0o640, 0o660] {
        assert!(key_mode_allowed(mode));
        assert!(key_mode_allowed(mode | 0o4000));
    }
    for mode in [0o000, 0o200, 0o444, 0o604, 0o644, 0o666, 0o700, 0o777] {
        assert!(!key_mode_allowed(mode));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn unix_all_five_allowed_modes_load_real_private_and_public_key_files() -> TestResult {
    let path = permission_fixture().await?;
    let result: TestResult = async {
        for mode in [0o400, 0o440, 0o600, 0o640, 0o660] {
            set_key_modes(&path, mode, mode).await?;
            let signer = load_pair(&path, &PassportConfig::default(), true).await?;
            let token = signer.sign_at("client", "token", None, &[], 100, 200)?;
            assert!(signer.verify_at(&token, "client", 100).is_ok());
        }
        Ok(())
    }
    .await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    result?;
    cleanup
}

#[cfg(unix)]
#[tokio::test]
async fn unix_wrong_private_or_public_mode_is_default_sanitized_500() -> TestResult {
    let path = permission_fixture().await?;
    let result: TestResult = async {
        for (private, public) in [
            (0o644, 0o600),
            (0o600, 0o644),
            (0o700, 0o600),
            (0o600, 0o666),
        ] {
            set_key_modes(&path, private, public).await?;
            let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
            assert_eq!(error.code(), "passport_key_permissions_invalid");
            assert_eq!(
                error.status(),
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
            assert!(!format!("{error:?}").contains(path.to_string_lossy().as_ref()));
            assert!(!error.to_string().contains("BEGIN"));
        }
        Ok(())
    }
    .await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    result?;
    cleanup
}

#[cfg(unix)]
#[tokio::test]
async fn disabled_mode_check_still_rejects_invalid_pem_before_permissions() -> TestResult {
    let path = permission_fixture().await?;
    let result: TestResult = async {
        set_key_modes(&path, 0o644, 0o644).await?;
        assert!(
            load_pair(&path, &PassportConfig::default(), false)
                .await
                .is_ok()
        );
        tokio::fs::write(path.join("oauth-private.key"), b"invalid-pem-sentinel").await?;
        for enabled in [true, false] {
            let error =
                signing_failure(load_pair(&path, &PassportConfig::default(), enabled).await)?;
            assert_eq!(error.code(), "passport_signing_key_pair_invalid");
            assert!(!format!("{error:?}").contains("invalid-pem-sentinel"));
        }
        Ok(())
    }
    .await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    result?;
    cleanup
}

#[cfg(unix)]
#[tokio::test]
async fn private_pem_then_its_permissions_precede_public_read_or_pem_failures() -> TestResult {
    let path = permission_fixture().await?;
    let result: TestResult = async {
        set_key_modes(&path, 0o644, 0o600).await?;
        tokio::fs::write(path.join("oauth-public.key"), b"invalid-public-pem").await?;
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
        assert_eq!(error.code(), "passport_key_permissions_invalid");
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), false).await)?;
        assert_eq!(error.code(), "passport_signing_key_pair_invalid");
        set_key_modes(&path, 0o600, 0o644).await?;
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
        assert_eq!(error.code(), "passport_signing_key_pair_invalid");
        set_key_modes(&path, 0o644, 0o600).await?;
        tokio::fs::remove_file(path.join("oauth-public.key")).await?;
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
        assert_eq!(error.code(), "passport_key_permissions_invalid");
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), false).await)?;
        assert_eq!(error.code(), "passport_public_key_missing");
        tokio::fs::write(path.join("oauth-private.key"), b"invalid-private-pem").await?;
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
        assert_eq!(error.code(), "passport_signing_key_pair_invalid");
        Ok(())
    }
    .await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    result?;
    cleanup
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_permissions_follow_opened_key_target_without_new_acl_policy() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;
    let path = permission_fixture().await?;
    let result: TestResult = async {
        tokio::fs::rename(
            path.join("oauth-private.key"),
            path.join("private-target.key"),
        )
        .await?;
        tokio::fs::rename(
            path.join("oauth-public.key"),
            path.join("public-target.key"),
        )
        .await?;
        std::os::unix::fs::symlink("private-target.key", path.join("oauth-private.key"))?;
        std::os::unix::fs::symlink("public-target.key", path.join("oauth-public.key"))?;
        assert!(
            load_pair(&path, &PassportConfig::default(), true)
                .await
                .is_ok()
        );
        tokio::fs::set_permissions(
            path.join("public-target.key"),
            std::fs::Permissions::from_mode(0o644),
        )
        .await?;
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
        assert_eq!(error.code(), "passport_key_permissions_invalid");
        Ok(())
    }
    .await;
    let cleanup =
        remove_permission_fixture(&path, &["private-target.key", "public-target.key"]).await;
    result?;
    cleanup
}

#[tokio::test]
async fn inline_key_pair_never_stats_missing_key_directory_when_check_enabled() -> TestResult {
    let path = directory();
    let config = PassportConfig {
        private_key: Some(std::str::from_utf8(PRIVATE)?.to_owned()),
        public_key: Some(std::str::from_utf8(PUBLIC)?.to_owned()),
        ..Default::default()
    };
    assert!(load_pair(&path, &config, true).await.is_ok());
    assert!(tokio::fs::metadata(&path).await.is_err());
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn windows_file_keys_compile_and_skip_posix_mode_branch() -> TestResult {
    let path = permission_fixture().await?;
    let result = load_pair(&path, &PassportConfig::default(), true).await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    assert!(result.is_ok());
    cleanup
}

// DER 为只含一个 INTEGER 的非空 SEQUENCE，低层可解析但缺失合法 RSA 公钥字段。
const ASN1_NON_RSA_PUBLIC: &[u8] =
    b"-----BEGIN RSA PUBLIC KEY-----\nMAMCAQE=\n-----END RSA PUBLIC KEY-----\n";

#[test]
fn mature_jwk_projection_rejects_asn1_only_public_key_before_permission_stage() -> TestResult {
    use jsonwebtoken::{Algorithm, DecodingKey, errors::ErrorKind as JwtErrorKind, jwk::Jwk};
    // 保留能触发旧bug的精确前提：PEM/ASN.1层接收，但成熟RSA解析层拒绝。
    let key = DecodingKey::from_rsa_pem(ASN1_NON_RSA_PUBLIC)?;
    let error = Jwk::from_decoding_key(&key, Some(Algorithm::RS256))
        .err()
        .ok_or_else(|| {
            Error::custom(
                "provider_fixture_expected_rejection",
                "ASN.1结构不得冒充合法RSA公钥",
            )
        })?;
    assert!(matches!(error.kind(), JwtErrorKind::InvalidRsaKey(_)));
    assert!(validate_key_material(ASN1_NON_RSA_PUBLIC, KeyKind::Public).is_err());
    assert!(validate_key_material(OTHER_PUBLIC, KeyKind::Public).is_ok());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn invalid_rsa_public_key_with_valid_asn1_is_rejected_before_its_bad_mode() -> TestResult {
    let path = permission_fixture().await?;
    let result: TestResult = async {
        tokio::fs::write(path.join("oauth-public.key"), ASN1_NON_RSA_PUBLIC).await?;
        set_key_modes(&path, 0o600, 0o644).await?;
        for enabled in [true, false] {
            let error =
                signing_failure(load_pair(&path, &PassportConfig::default(), enabled).await)?;
            assert_eq!(error.code(), "passport_signing_key_pair_invalid");
            assert_eq!(
                error.status(),
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
        }
        Ok(())
    }
    .await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    result?;
    cleanup
}

#[cfg(unix)]
#[tokio::test]
async fn valid_mismatched_public_key_checks_mode_then_fails_only_at_final_pair_proof() -> TestResult
{
    let path = permission_fixture().await?;
    let result: TestResult = async {
        tokio::fs::write(path.join("oauth-public.key"), OTHER_PUBLIC).await?;
        set_key_modes(&path, 0o600, 0o644).await?;
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
        assert_eq!(error.code(), "passport_key_permissions_invalid");
        set_key_modes(&path, 0o600, 0o600).await?;
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), true).await)?;
        assert_eq!(error.code(), "passport_signing_key_pair_invalid");
        let error = signing_failure(load_pair(&path, &PassportConfig::default(), false).await)?;
        assert_eq!(error.code(), "passport_signing_key_pair_invalid");
        Ok(())
    }
    .await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    result?;
    cleanup
}

#[cfg(unix)]
const EXTRA_PATH_FILES: &[(&str, &[u8])] = &[
    ("plain.key", PRIVATE),
    ("space key.pem", PUBLIC),
    ("space%20key.pem", PRIVATE),
    ("encoded%2Fsegment.pem", PRIVATE),
    ("plus+key.pem", PRIVATE),
    ("plain.key?query", PUBLIC),
    ("plain.key#fragment", PUBLIC),
    ("literal\\nkey.pem", PRIVATE),
];
#[cfg(unix)]
async fn path_fixture() -> TestResult<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    let path = permission_fixture().await?;
    for (name, contents) in EXTRA_PATH_FILES {
        tokio::fs::write(path.join(name), contents).await?;
        tokio::fs::set_permissions(path.join(name), std::fs::Permissions::from_mode(0o600)).await?;
    }
    tokio::fs::create_dir(path.join("encoded")).await?;
    tokio::fs::write(path.join("encoded/segment.pem"), PUBLIC).await?;
    tokio::fs::set_permissions(
        path.join("encoded/segment.pem"),
        std::fs::Permissions::from_mode(0o600),
    )
    .await?;
    Ok(path)
}
#[cfg(unix)]
async fn remove_path_fixture(path: &Path) -> TestResult {
    let file_cleanup = tokio::fs::remove_file(path.join("encoded/segment.pem")).await;
    let child_cleanup = tokio::fs::remove_dir(path.join("encoded")).await;
    let names = EXTRA_PATH_FILES
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    let base_cleanup = remove_permission_fixture(path, &names).await;
    file_cleanup?;
    child_cleanup?;
    base_cleanup
}

#[cfg(unix)]
struct ObservedPathCase {
    label: &'static str,
    configured: Option<String>,
}
#[cfg(unix)]
fn path_case(label: &'static str, configured: impl Into<String>) -> ObservedPathCase {
    ObservedPathCase {
        label,
        configured: Some(configured.into()),
    }
}
#[cfg(unix)]
fn observed_defaults_and_inline() -> TestResult<Vec<ObservedPathCase>> {
    let private = std::str::from_utf8(PRIVATE)?;
    Ok(vec![
        ObservedPathCase {
            label: "default-null",
            configured: None,
        },
        path_case("default-empty", ""),
        path_case("default-string-zero", "0"),
        path_case("inline-private", private),
        path_case("inline-public", std::str::from_utf8(PUBLIC)?),
        path_case(
            "inline-private-escaped-newline",
            private.replace('\n', "\\n"),
        ),
    ])
}
#[cfg(unix)]
fn observed_literal_success_paths(path: &Path) -> Vec<ObservedPathCase> {
    let absolute = |name: &str| path.join(name).to_string_lossy().into_owned();
    let uri = |name: &str| format!("file://{}", path.join(name).display());
    vec![
        path_case("plain-absolute", absolute("plain.key")),
        path_case("file-absolute", uri("plain.key")),
        path_case(
            "file-host-localhost",
            format!("file://localhost{}", path.join("plain.key").display()),
        ),
        path_case("plain-literal-space", absolute("space key.pem")),
        path_case("plain-literal-percent20", absolute("space%20key.pem")),
        path_case("file-literal-space", uri("space key.pem")),
        path_case("file-percent20", uri("space%20key.pem")),
        path_case("plain-percent2f", absolute("encoded%2Fsegment.pem")),
        path_case("file-percent2f", uri("encoded%2Fsegment.pem")),
        path_case("file-plus", uri("plus+key.pem")),
        path_case("file-query-literal", uri("plain.key?query")),
        path_case("file-fragment-literal", uri("plain.key#fragment")),
        path_case("plain-query-literal", absolute("plain.key?query")),
        path_case("plain-fragment-literal", absolute("plain.key#fragment")),
    ]
}
#[cfg(unix)]
fn observed_rejected_paths(path: &Path) -> TestResult<Vec<ObservedPathCase>> {
    // 只为构造显式relative测试值读取cwd，不改变全局cwd、不拿cwd猜默认应用目录。
    let depth = std::env::current_dir()?
        .components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count();
    let prefix = "../".repeat(depth);
    let relative = format!("{prefix}{}", path.strip_prefix("/")?.display());
    Ok(vec![
        path_case("plain-relative", format!("{relative}/plain.key")),
        path_case("plain-relative-dot", format!("./{relative}/plain.key")),
        path_case(
            "plain-relative-parent",
            format!("{relative}/encoded/../plain.key"),
        ),
        path_case("file-relative", "file://plain.key"),
        path_case("file-relative-dot", "file://./plain.key"),
        path_case(
            "scheme-uppercase",
            format!("FILE://{}", path.join("plain.key").display()),
        ),
        path_case(
            "scheme-mixed-case",
            format!("FiLe://{}", path.join("plain.key").display()),
        ),
        path_case(
            "file-one-slash",
            format!("file:{}", path.join("plain.key").display()),
        ),
        path_case(
            "path-literal-backslash-n",
            path.join("literal\\nkey.pem").to_string_lossy(),
        ),
        path_case(
            "path-actual-newline-missing",
            format!("{}/literal\nkey.pem", path.display()),
        ),
        path_case("missing-relative", format!("{relative}/absent.key")),
        path_case(
            "missing-absolute",
            path.join("absent.key").to_string_lossy(),
        ),
        path_case(
            "file-missing",
            format!("file://{}", path.join("absent.key").display()),
        ),
        path_case("existing-directory", path.join("encoded").to_string_lossy()),
        path_case("invalid-pem-string", "not-a-key"),
        path_case("whitespace-string", " "),
        path_case("path-with-nul", format!("{}/plain\0.key", path.display())),
        path_case(
            "file-host-nonlocal",
            format!("file://remote.invalid{}", path.join("plain.key").display()),
        ),
    ])
}

// 固定实际oracle SHA48bc10e8...：38输入在四个host/check组合中结果逐值相同。
#[cfg(unix)]
const PHP_LINUX_PATH_OBSERVATIONS: &[(&str, Option<&str>)] = &[
    ("default-null", Some("private-test")),
    ("default-empty", Some("private-test")),
    ("default-string-zero", Some("private-test")),
    ("inline-private", Some("private-test")),
    ("inline-public", Some("public-test")),
    ("inline-private-escaped-newline", Some("private-test")),
    ("plain-absolute", Some("private-test")),
    ("plain-relative", None),
    ("plain-relative-dot", None),
    ("plain-relative-parent", None),
    ("file-absolute", Some("private-test")),
    ("file-relative", None),
    ("file-relative-dot", None),
    ("file-host-localhost", Some("private-test")),
    ("scheme-uppercase", None),
    ("scheme-mixed-case", None),
    ("file-one-slash", None),
    ("plain-literal-space", Some("public-test")),
    ("plain-literal-percent20", Some("private-test")),
    ("file-literal-space", Some("public-test")),
    ("file-percent20", Some("private-test")),
    ("plain-percent2f", Some("private-test")),
    ("file-percent2f", Some("private-test")),
    ("file-plus", Some("private-test")),
    ("file-query-literal", Some("public-test")),
    ("file-fragment-literal", Some("public-test")),
    ("plain-query-literal", Some("public-test")),
    ("plain-fragment-literal", Some("public-test")),
    ("path-literal-backslash-n", None),
    ("path-actual-newline-missing", None),
    ("missing-relative", None),
    ("missing-absolute", None),
    ("file-missing", None),
    ("existing-directory", None),
    ("invalid-pem-string", None),
    ("whitespace-string", None),
    ("path-with-nul", None),
    ("file-host-nonlocal", None),
];

#[cfg(unix)]
async fn assert_observed_path_case(path: &Path, case: &ObservedPathCase) -> TestResult {
    let expected = PHP_LINUX_PATH_OBSERVATIONS
        .iter()
        .find(|(label, _)| *label == case.label)
        .ok_or_else(|| Error::custom("provider_fixture_case_unknown", "缺少固定PHP路径case映射"))?
        .1;
    let kind = if expected == Some("public-test") {
        KeyKind::Public
    } else {
        KeyKind::Private
    };
    for enabled in [true, false] {
        let result = load_key(path, case.configured.as_deref(), kind, enabled).await;
        if let Some(material) = expected {
            let contents = result?;
            let bytes = if material == "private-test" {
                PRIVATE
            } else {
                PUBLIC
            };
            // 不用assert_eq打印公开测试private PEM；只核字节是否相同。
            let matches = contents.as_slice() == bytes;
            assert!(matches);
        } else {
            let error = result.err().ok_or_else(|| {
                Error::custom("provider_fixture_expected_rejection", "固定PHP反例未拒绝")
            })?;
            assert_eq!(
                error.status(),
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
            assert!(!format!("{error:?}").contains(path.to_string_lossy().as_ref()));
        }
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn unix_configured_key_sources_match_all_38_actual_linux_php_cases() -> TestResult {
    let path = path_fixture().await?;
    let result: TestResult = async {
        let mut cases = observed_defaults_and_inline()?;
        cases.extend(observed_literal_success_paths(&path));
        cases.extend(observed_rejected_paths(&path)?);
        assert_eq!(cases.len(), PHP_LINUX_PATH_OBSERVATIONS.len());
        for case in &cases {
            assert_observed_path_case(&path, case).await?;
        }
        Ok(())
    }
    .await;
    let cleanup = remove_path_fixture(&path).await;
    result?;
    cleanup
}

#[test]
fn valid_inline_material_is_not_reclassified_as_path_when_role_is_wrong() -> TestResult {
    assert!(inline_material(PUBLIC, KeyKind::Public)?);
    assert!(inline_material(PRIVATE, KeyKind::Private)?);
    assert!(inline_material(PUBLIC, KeyKind::Private).is_err());
    assert!(inline_material(PRIVATE, KeyKind::Public).is_err());
    assert!(!inline_material(b"not-a-key", KeyKind::Private)?);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn explicit_paths_keep_rsa_permissions_and_final_pair_failure_order() -> TestResult {
    let path = permission_fixture().await?;
    let result: TestResult = async {
        let config = PassportConfig {
            private_key: Some(
                path.join("oauth-private.key")
                    .to_string_lossy()
                    .into_owned(),
            ),
            public_key: Some(format!(
                "file://localhost{}",
                path.join("oauth-public.key").display()
            )),
            ..Default::default()
        };
        assert!(load_pair(&path, &config, true).await.is_ok());
        tokio::fs::write(path.join("oauth-public.key"), ASN1_NON_RSA_PUBLIC).await?;
        set_key_modes(&path, 0o600, 0o644).await?;
        for enabled in [true, false] {
            assert_eq!(
                signing_failure(load_pair(&path, &config, enabled).await)?.code(),
                "passport_signing_key_pair_invalid"
            );
        }
        tokio::fs::write(path.join("oauth-public.key"), OTHER_PUBLIC).await?;
        assert_eq!(
            signing_failure(load_pair(&path, &config, true).await)?.code(),
            "passport_key_permissions_invalid"
        );
        set_key_modes(&path, 0o600, 0o600).await?;
        assert_eq!(
            signing_failure(load_pair(&path, &config, true).await)?.code(),
            "passport_signing_key_pair_invalid"
        );
        assert_eq!(
            signing_failure(load_pair(&path, &config, false).await)?.code(),
            "passport_signing_key_pair_invalid"
        );
        Ok(())
    }
    .await;
    let cleanup = remove_permission_fixture(&path, &[]).await;
    result?;
    cleanup
}

#[cfg(unix)]
#[tokio::test]
async fn relative_existing_file_is_checked_then_rejected_without_default_root_guess() -> TestResult
{
    let path = path_fixture().await?;
    let result: TestResult = async {
        let cases = observed_rejected_paths(&path)?;
        let relative = cases
            .iter()
            .find(|case| case.label == "plain-relative")
            .and_then(|case| case.configured.as_deref())
            .ok_or_else(|| Error::custom("provider_fixture_case_missing", "缺少relative夹具"))?;
        assert!(tokio::fs::metadata(relative).await?.is_file());
        let error = configured_file_path(relative, KeyKind::Private)
            .await
            .err()
            .ok_or_else(|| {
                Error::custom(
                    "provider_fixture_expected_rejection",
                    "relative加file前缀后应不可读",
                )
            })?;
        assert_eq!(error.code(), "passport_private_key_unreadable");
        Ok(())
    }
    .await;
    let cleanup = remove_path_fixture(&path).await;
    result?;
    cleanup
}

#[cfg(not(unix))]
#[tokio::test]
async fn unobserved_platform_explicit_path_is_named_gap_not_claimed_php_equivalence() -> TestResult
{
    let error = load_key(
        &directory(),
        Some("developer-explicit.key"),
        KeyKind::Private,
        true,
    )
    .await
    .err()
    .ok_or_else(|| {
        Error::custom(
            "provider_fixture_expected_rejection",
            "未核验的平台路径必须显式标明边界",
        )
    })?;
    assert_eq!(error.code(), "passport_key_path_platform_unverified");
    Ok(())
}

#[tokio::test]
async fn unused_oauth_does_not_read_missing_keys_or_require_authority_at_boot() -> TestResult {
    let path = directory();
    let mut builder = builder();
    builder.provider(PassportServiceProvider::new(path.clone()));
    let config = Figment::from(Serialized::defaults(json!({
        "security": {"encryption": {"active_key_id": "provider-fixture", "keys": {"provider-fixture": STANDARD.encode([31; 32])}}}
    })));
    let runtime = builder.start(config).await?;
    let result: TestResult = async {
        assert!(tokio::fs::metadata(&path).await.is_err());
        let config = runtime.config().section::<PassportConfig>("passport")?;
        assert!(config.private_key.is_none());
        assert!(config.public_key.is_none());
        let error = runtime
            .container()
            .resolve::<OAuthAccessSigner>()
            .await
            .err()
            .ok_or_else(|| {
                Error::custom(
                    "provider_fixture_expected_rejection",
                    "缺失私钥在首次实际解析时未拒绝",
                )
            })?;
        assert_service_initialization_cause(&error, "passport_private_key_missing");
        Ok(())
    }
    .await;
    runtime.shutdown(deadline()).await?;
    result
}

fn capability_source(private: Option<&[u8]>, public: Option<&[u8]>) -> TestResult<Figment> {
    let private = private.map(std::str::from_utf8).transpose()?;
    let public = public.map(std::str::from_utf8).transpose()?;
    Ok(Figment::from(Serialized::defaults(json!({
        "security": {"encryption": {"active_key_id": "provider-fixture", "keys": {"provider-fixture": STANDARD.encode([31; 32])}}},
        "passport": {"private_key": private, "public_key": public}
    }))))
}

#[tokio::test]
async fn private_only_singleton_signer_ignores_missing_or_invalid_public_key() -> TestResult {
    for public in [None, Some(b"invalid-public-sentinel".as_slice())] {
        let path = directory();
        let mut builder = builder();
        builder.provider(PassportServiceProvider::new(path.clone()));
        let runtime = builder
            .start(capability_source(Some(PRIVATE), public)?)
            .await?;
        let result: TestResult = async {
            let signer = runtime.container().resolve::<OAuthAccessSigner>().await?;
            let again = runtime.container().resolve::<OAuthAccessSigner>().await?;
            assert!(Arc::ptr_eq(&signer, &again));
            let scope = runtime.container().scope()?;
            assert!(Arc::ptr_eq(
                &signer,
                &scope.resolve::<OAuthAccessSigner>().await?
            ));
            let token = signer.sign_at("client", "private-only", Some("user"), &[], 100, 200)?;
            assert_eq!(
                OAuthAccessVerifier::from_pem(PUBLIC)?
                    .verify_at(&token, "client", 100)?
                    .sub,
                "user"
            );
            // 缺失/非法另一用途key不参与Signer解析，也没有创建默认key目录。
            assert!(tokio::fs::metadata(&path).await.is_err());
            assert!(
                runtime
                    .container()
                    .resolve::<OAuthAccessVerifier>()
                    .await
                    .is_err()
            );
            assert!(Arc::ptr_eq(
                &signer,
                &runtime.container().resolve::<OAuthAccessSigner>().await?
            ));
            Ok(())
        }
        .await;
        runtime.shutdown(deadline()).await?;
        result?;
    }
    Ok(())
}

#[tokio::test]
async fn public_only_singleton_verifier_ignores_missing_or_invalid_private_key() -> TestResult {
    for private in [None, Some(b"invalid-private-sentinel".as_slice())] {
        let path = directory();
        let mut builder = builder();
        builder.provider(PassportServiceProvider::new(path.clone()));
        let runtime = builder
            .start(capability_source(private, Some(PUBLIC))?)
            .await?;
        let result: TestResult = async {
            let verifier = runtime.container().resolve::<OAuthAccessVerifier>().await?;
            let again = runtime.container().resolve::<OAuthAccessVerifier>().await?;
            assert!(Arc::ptr_eq(&verifier, &again));
            let scope = runtime.container().scope()?;
            assert!(Arc::ptr_eq(
                &verifier,
                &scope.resolve::<OAuthAccessVerifier>().await?
            ));
            let token = OAuthAccessSigner::from_pem(PRIVATE)?.sign_at(
                "client",
                "public-only",
                Some("user"),
                &[],
                100,
                200,
            )?;
            assert_eq!(verifier.verify_at(&token, "client", 100)?.sub, "user");
            assert!(tokio::fs::metadata(&path).await.is_err());
            assert!(
                runtime
                    .container()
                    .resolve::<OAuthAccessSigner>()
                    .await
                    .is_err()
            );
            assert!(Arc::ptr_eq(
                &verifier,
                &runtime.container().resolve::<OAuthAccessVerifier>().await?
            ));
            Ok(())
        }
        .await;
        runtime.shutdown(deadline()).await?;
        result?;
    }
    Ok(())
}

#[tokio::test]
async fn mismatched_capabilities_resolve_independently_but_signature_and_explicit_pair_fail()
-> TestResult {
    let path = directory();
    let mut builder = builder();
    builder.provider(PassportServiceProvider::new(path.clone()));
    let runtime = builder
        .start(capability_source(Some(PRIVATE), Some(OTHER_PUBLIC))?)
        .await?;
    let result: TestResult = async {
        let signer = runtime.container().resolve::<OAuthAccessSigner>().await?;
        let verifier = runtime.container().resolve::<OAuthAccessVerifier>().await?;
        let token = signer.sign_at("client", "mismatched", None, &[], 100, 200)?;
        assert!(verifier.verify_at(&token, "client", 100).is_err());
        let config = runtime.config().section::<PassportConfig>("passport")?;
        let error = signing_failure(validate_pair(&path, &config, true).await)?;
        assert_eq!(error.code(), "passport_signing_key_pair_invalid");
        assert!(tokio::fs::metadata(&path).await.is_err());
        Ok(())
    }
    .await;
    runtime.shutdown(deadline()).await?;
    result
}

/// 只用于DI消费链：所有签发操作拒绝，不伪造DB事务或生产认证权威。
#[derive(Default)]
struct RejectingStorage {
    lookups: AtomicUsize,
}

fn rejecting_storage_unavailable() -> crate::GrantStorageError {
    crate::GrantStorageError::Unavailable(Error::custom(
        "fixture_custom_store_reached",
        "受控自定义存储拒绝所有实际业务",
    ))
}

impl GrantStorage for RejectingStorage {
    fn current_time(&self) -> crate::GrantStorageResult<i64> {
        Err(rejecting_storage_unavailable())
    }
    fn client_by_protocol<'a>(
        &'a self,
        _identifier: &'a str,
    ) -> BoxFuture<'a, crate::GrantStorageResult<Option<ClientRecord>>> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(rejecting_storage_unavailable()) })
    }
    fn persist_code<'a>(
        &'a self,
        _code: &'a crate::CodeRecord,
        _client: &'a crate::ClientSnapshot,
    ) -> BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async { Err(crate::GrantStorageError::AuthorityDenied) })
    }
    fn exchange_code<'a>(
        &'a self,
        _code: &'a crate::CodeRecord,
        _client: &'a crate::ClientSnapshot,
        _tokens: &'a crate::TokenRecords,
    ) -> BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async { Err(crate::GrantStorageError::AuthorityDenied) })
    }
    fn rotate_refresh<'a>(
        &'a self,
        _refresh: &'a crate::RefreshRedemption,
        _client: &'a crate::ClientSnapshot,
        _tokens: &'a crate::TokenRecords,
        _revoke_old: bool,
    ) -> BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async { Err(crate::GrantStorageError::AuthorityDenied) })
    }
    fn is_code_revoked<'a>(
        &'a self,
        _id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async {
            Err(crate::GrantStorageError::Unavailable(
                openrust::Error::custom(
                    "fixture_revocation_read_unsupported",
                    "夹具不提供已提交撤销读取",
                ),
            ))
        })
    }
    fn is_refresh_revoked<'a>(
        &'a self,
        _id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async {
            Err(crate::GrantStorageError::Unavailable(
                openrust::Error::custom(
                    "fixture_revocation_read_unsupported",
                    "夹具不提供已提交撤销读取",
                ),
            ))
        })
    }
    fn is_access_revoked<'a>(
        &'a self,
        _id: &'a str,
    ) -> BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async { Err(rejecting_storage_unavailable()) })
    }
    fn revoke_access<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async { Err(rejecting_storage_unavailable()) })
    }
}

struct ApplicationStorageProvider {
    shared: Option<Arc<RejectingStorage>>,
    database_attempts: Arc<AtomicUsize>,
}

impl ServiceProvider for ApplicationStorageProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if let Some(storage) = &self.shared {
                let storage: Arc<dyn GrantStorage> = storage.clone();
                app.instance::<dyn GrantStorage>(storage).await?;
            } else {
                app.scoped::<RejectingStorage, _>(|_| {
                    Box::pin(async { Ok(Arc::new(RejectingStorage::default())) })
                })
                .await?;
                app.scoped::<dyn GrantStorage, _>(|resolver| {
                    Box::pin(async move {
                        let storage: Arc<dyn GrantStorage> =
                            resolver.resolve::<RejectingStorage>().await?;
                        Ok(storage)
                    })
                })
                .await?;
            }
            // 只作未触达证据；不是Db替代实现，不选择/连接任何数据库。
            let attempts = self.database_attempts.clone();
            app.scoped::<DatabaseGrantRepository, _>(move |_| {
                let attempts = attempts.clone();
                Box::pin(async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Err(Error::custom(
                        "fixture_default_db_must_not_resolve",
                        "自定义存储路径不应解析DB仓储",
                    ))
                })
            })
            .await
        })
    }
}

async fn prepare_reaches_rejecting_store(server: &AuthorizationServer) -> TestResult {
    let parameters = crate::OAuthParameters::new(Map::from_iter([
        ("response_type".into(), json!("code")),
        ("client_id".into(), json!("fixture-store-client")),
    ]));
    let error = server
        .prepare_authorization(&parameters)
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "provider_fixture_expected_rejection",
                "拒绝型存储不应准备成功",
            )
        })?;
    let crate::AuthorizationFailure::Local(error) = error else {
        return Err(Error::custom("provider_fixture_wrong_failure", "存储失败不应建立回调").into());
    };
    let serialized = serde_json::to_value(error)?;
    assert_eq!(serialized.get("error"), Some(&json!("server_error")));
    assert!(
        !serialized
            .to_string()
            .contains("fixture_custom_store_reached")
    );
    Ok(())
}

#[tokio::test]
async fn application_shared_storage_resolves_same_server_without_db_authority_or_db_factory()
-> TestResult {
    let store = Arc::new(RejectingStorage::default());
    let expected: Arc<dyn GrantStorage> = store.clone();
    let database_attempts = Arc::new(AtomicUsize::new(0));
    let mut builder = builder();
    builder
        .provider(ApplicationStorageProvider {
            shared: Some(store.clone()),
            database_attempts: database_attempts.clone(),
        })
        .provider(PassportServiceProvider::new(directory()));
    let runtime = builder.start(source()?).await?;
    let left = runtime.container().scope()?;
    let right = runtime.container().scope()?;
    let result: TestResult = async {
        assert!(!runtime.container().contains::<DatabaseGrantAuthority>());
        assert!(Arc::ptr_eq(
            &expected,
            &runtime.container().resolve::<dyn GrantStorage>().await?
        ));
        assert!(Arc::ptr_eq(
            &expected,
            &left.resolve::<dyn GrantStorage>().await?
        ));
        assert!(Arc::ptr_eq(
            &expected,
            &right.resolve::<dyn GrantStorage>().await?
        ));
        let server = left.resolve::<AuthorizationServer>().await?;
        let other_server = right.resolve::<AuthorizationServer>().await?;
        assert!(Arc::ptr_eq(
            &server,
            &left.resolve::<AuthorizationServer>().await?
        ));
        assert!(!Arc::ptr_eq(&server, &other_server));
        assert_eq!(store.lookups.load(Ordering::SeqCst), 0);
        prepare_reaches_rejecting_store(&server).await?;
        prepare_reaches_rejecting_store(&other_server).await?;
        assert_eq!(store.lookups.load(Ordering::SeqCst), 2);
        assert_eq!(database_attempts.load(Ordering::SeqCst), 0);
        Ok(())
    }
    .await;
    left.shutdown(deadline()).await?;
    right.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    result
}

#[tokio::test]
async fn application_scoped_storage_preserves_two_scope_instances_and_actual_prepare_dispatch()
-> TestResult {
    let database_attempts = Arc::new(AtomicUsize::new(0));
    let mut builder = builder();
    builder
        .provider(ApplicationStorageProvider {
            shared: None,
            database_attempts: database_attempts.clone(),
        })
        .provider(PassportServiceProvider::new(directory()));
    let runtime = builder.start(source()?).await?;
    let left = runtime.container().scope()?;
    let right = runtime.container().scope()?;
    let result: TestResult = async {
        assert!(!runtime.container().contains::<DatabaseGrantAuthority>());
        let left_store = left.resolve::<RejectingStorage>().await?;
        let right_store = right.resolve::<RejectingStorage>().await?;
        assert!(!Arc::ptr_eq(&left_store, &right_store));
        let left_expected: Arc<dyn GrantStorage> = left_store.clone();
        let right_expected: Arc<dyn GrantStorage> = right_store.clone();
        assert!(Arc::ptr_eq(
            &left_expected,
            &left.resolve::<dyn GrantStorage>().await?
        ));
        assert!(Arc::ptr_eq(
            &right_expected,
            &right.resolve::<dyn GrantStorage>().await?
        ));
        let left_server = left.resolve::<AuthorizationServer>().await?;
        let right_server = right.resolve::<AuthorizationServer>().await?;
        assert!(Arc::ptr_eq(
            &left_server,
            &left.resolve::<AuthorizationServer>().await?
        ));
        assert!(!Arc::ptr_eq(&left_server, &right_server));
        prepare_reaches_rejecting_store(&left_server).await?;
        assert_eq!(left_store.lookups.load(Ordering::SeqCst), 1);
        assert_eq!(right_store.lookups.load(Ordering::SeqCst), 0);
        prepare_reaches_rejecting_store(&right_server).await?;
        assert_eq!(right_store.lookups.load(Ordering::SeqCst), 1);
        assert_eq!(database_attempts.load(Ordering::SeqCst), 0);
        Ok(())
    }
    .await;
    left.shutdown(deadline()).await?;
    right.shutdown(deadline()).await?;
    runtime.shutdown(deadline()).await?;
    result
}

struct ActiveOverrideLookup {
    record: ClientRecord,
    mode: AtomicUsize,
    find_calls: AtomicUsize,
    active_calls: AtomicUsize,
}
impl ClientLookup for ActiveOverrideLookup {
    fn find<'a>(
        &'a self,
        _context: &'a crate::ClientLookupContext<'a>,
        _identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(self.record.clone()))
        })
    }
    fn find_active<'a>(
        &'a self,
        _context: &'a crate::ClientLookupContext<'a>,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move {
            assert_eq!(identifier, &json!("application-alias-not-uuid"));
            self.active_calls.fetch_add(1, Ordering::SeqCst);
            match self.mode.load(Ordering::SeqCst) {
                1 => Ok(None),
                2 => Err(Error::custom(
                    "lookup_active_rule_failed",
                    "private-active-policy-detail",
                )),
                _ => Ok(Some(self.record.clone())),
            }
        })
    }
}

#[tokio::test]
async fn application_active_lookup_override_reaches_default_server_and_closed_scope_blocks_it()
-> TestResult {
    let factory: Arc<dyn ClientModelFactory> = Arc::new(ClientFactory::default());
    let raw = json!({"id":"019a4d4b-f21a-7191-bce9-fdc817a165aa", "name":"lookup fixture",
        "revoked":false, "secret":null, "grant_types":["authorization_code"],
        "redirect_uris":["https://lookup-fixture.test/cb"]});
    let model = factory.new_from_builder(raw.as_object().ok_or("模型非对象")?.clone())?;
    let lookup = Arc::new(ActiveOverrideLookup {
        record: model,
        mode: AtomicUsize::new(1),
        find_calls: AtomicUsize::new(0),
        active_calls: AtomicUsize::new(0),
    });
    let mut providers = builder();
    providers
        .provider(ApplicationBindings {
            factory,
            lookup: lookup.clone(),
            signer: Arc::new(OAuthAccessSigner::from_pem(PRIVATE)?),
            verifier: Arc::new(OAuthAccessVerifier::from_pem(PUBLIC)?),
        })
        .provider(FixtureAuthorityProvider)
        .provider(PassportServiceProvider::new(directory()));
    let config = Figment::from(Serialized::defaults(json!({
        "security":{"encryption":{"active_key_id":"provider-fixture","keys":{"provider-fixture":STANDARD.encode([31;32])}}}
    })));
    let runtime = providers.start(config).await?;
    let outcome: TestResult = async {
        let scope = runtime.container().scope()?;
        let context = crate::ClientLookupContext::from(&scope);
        assert!(
            lookup
                .find(&context, &json!("application-alias-not-uuid"))
                .await?
                .is_some()
        );
        let server = scope.resolve::<AuthorizationServer>().await?;
        let repository = scope.resolve::<DatabaseGrantRepository>().await?;
        let bridge = scope.resolve::<BridgeClientRepository>().await?;
        let request = crate::OAuthParameters::new(
            json!({"grant_type":"authorization_code",
            "client_id":"application-alias-not-uuid", "code":"not-yet-decrypted"})
            .as_object()
            .ok_or("参数非对象")?
            .clone(),
        );
        for (mode, expected) in [(1, "invalid_client"), (2, "server_error")] {
            lookup.mode.store(mode, Ordering::SeqCst);
            let result = Box::pin(server.token(&request, None)).await;
            let Err(error) = result else {
                return Err("active覆写拒绝时不得签发".into());
            };
            let body = serde_json::to_value(error)?;
            assert_eq!(body["error"], expected);
            assert!(!body.to_string().contains("private-active-policy-detail"));
        }
        assert_eq!(lookup.find_calls.load(Ordering::SeqCst), 1);
        assert_eq!(lookup.active_calls.load(Ordering::SeqCst), 2);
        lookup.mode.store(0, Ordering::SeqCst);
        let authorize = crate::OAuthParameters::new(
            json!({"response_type":"code", "client_id":"application-alias-not-uuid",
            "redirect_uri":"https://lookup-fixture.test/cb", "code_challenge_method":"S256",
            "code_challenge":"E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"})
            .as_object()
            .ok_or("参数非对象")?
            .clone(),
        );
        assert!(server.prepare_authorization(&authorize).await.is_ok());
        assert_eq!(lookup.active_calls.load(Ordering::SeqCst), 3);
        scope.shutdown(deadline()).await?;
        let closed = repository
            .client_by_protocol("application-alias-not-uuid")
            .await;
        assert!(matches!(closed, Err(error) if error.code() == "scope_closed"));
        assert!(
            bridge
                .get_client_entity(&scope, "application-alias-not-uuid")
                .await
                .is_err()
        );
        assert_eq!(lookup.active_calls.load(Ordering::SeqCst), 3);
        // 同步纯clock不是入口开放证明；不能把此成功当作scope租约。
        assert!(crate::GrantStorage::current_time(repository.as_ref()).is_ok());
        Ok(())
    }
    .await;
    let cleanup = runtime.shutdown(deadline()).await;
    outcome?;
    cleanup?;
    Ok(())
}
