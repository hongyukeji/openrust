//! 真实隔离网络库中的授权码、PKCE、刷新与重放回归。
//!
//! SQL 仅为测试夹具播种，不表示已实现生产客户端管理或 Model save 生命周期。
//! 测试权威只核对固定用户与完整绑定，不表示生产 Guard/Fortify 二次认证已接入。

#![cfg(any(feature = "db-mysql", feature = "db-postgres"))]

use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::{
    database::sea_orm::ConnectionTrait,
    encryption::{ContextualEncrypter, EncryptionConfig},
    hashing::{BcryptConfig, HashManager, HashOptions, HashingConfig},
    http::axum::{
        body::to_bytes,
        http::{StatusCode, header},
        response::IntoResponse,
    },
    testing::TestDatabase,
};
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::{
    AuthorizationServer, GrantAuthority, GrantConfig, GrantLifetime, OAuthAccessSigner,
    OAuthAccessVerifier, OAuthError, OAuthParameters, Scope, ScopeRepository, TokenResponse,
    test_keys::{PRIVATE, PUBLIC},
};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const NOW: i64 = 1_700_000_000;
const PUBLIC_CLIENT: &str = "019a4d4b-f21a-7191-bce9-fdc817a165aa";
const CONFIDENTIAL_CLIENT: &str = "019a4d4b-f21a-7191-bce9-fdc817a165ab";
const CODE_ONLY_CLIENT: &str = "019a4d4b-f21a-7191-bce9-fdc817a165ac";
const OTHER_CLIENT: &str = "019a4d4b-f21a-7191-bce9-fdc817a165ad";
const CALLBACK: &str = "https://rp.example.test/callback?registered=yes";
const USER: &str = "7";
const BINDING: &str = "engine-fixture-verified-user-7";
const SECRET: &str = "engine-fixture-client-secret";
// RFC 7636 附录 B 的 S256 向量，避免测试自行实现摘要算法。
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

struct EngineMigrator;
impl openrust::database::sea_orm_migration::MigratorTrait for EngineMigrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

struct FixedClock {
    now: AtomicI64,
    step: AtomicI64,
}
impl GrantClock for FixedClock {
    fn now(&self) -> Result<i64> {
        Ok(self
            .now
            .fetch_add(self.step.load(Ordering::SeqCst), Ordering::SeqCst))
    }
}

/// 此权威仅属于隔离测试，完整匹配固定用户、实际客户端与允许scope，拒绝非空即通过的伪证明。
struct FixtureAuthority(Db);
impl<'tx> GrantAuthority<TransactionContext<'tx>> for FixtureAuthority {
    fn validate<'a>(
        &'a self,
        db_context: &'a TransactionContext<'tx>,
        context: &'a crate::GrantValidation<'a>,
        binding: &'a GrantAuthorityBinding,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !db_context.is_from(&self.0) {
                return Err(Error::custom(
                    "fixture_transaction_origin_mismatch",
                    "事务context来源不匹配",
                ));
            }
            if context.user_id == USER
                && [
                    PUBLIC_CLIENT,
                    CONFIDENTIAL_CLIENT,
                    CODE_ONLY_CLIENT,
                    OTHER_CLIENT,
                ]
                .contains(&context.client_id)
                && binding.as_str() == format!("{BINDING}:{}", context.client_id)
                && context
                    .scopes
                    .iter()
                    .all(|scope| matches!(scope.as_str(), "read" | "write" | "*"))
            {
                Ok(())
            } else {
                Err(Error::custom(
                    "engine_fixture_authority_rejected",
                    "测试用户或授权绑定不匹配",
                ))
            }
        })
    }
}

struct Fixture {
    repository: Arc<DatabaseGrantRepository>,
    owner: super::lookup_test_support::ScopedRepository,
    server: AuthorizationServer,
    verifier: OAuthAccessVerifier,
    clock: Arc<FixedClock>,
}

fn scoped_engine(
    repository: Arc<DatabaseGrantRepository>,
    hashing: Arc<HashManager>,
) -> TestResult<AuthorizationServer> {
    let signer = Arc::new(OAuthAccessSigner::from_pem(PRIVATE)?);
    let encryption = Arc::new(ContextualEncrypter::new(EncryptionConfig::new(
        "engine-fixture",
        [("engine-fixture".to_owned(), STANDARD.encode([41; 32]))].into(),
    ))?);
    let config = GrantConfig {
        authorization_code_lifetime: GrantLifetime::Seconds(600),
        access_token_lifetime: GrantLifetime::Seconds(3600),
        refresh_token_lifetime: GrantLifetime::Seconds(7200),
        ..Default::default()
    };
    let server =
        AuthorizationServer::new(repository, hashing, signer, encryption, scopes(), config);
    Ok(server)
}

impl Fixture {
    async fn new(database: &TestDatabase) -> TestResult<Self> {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        create_schema(&db).await?;
        let factory: Arc<dyn ClientModelFactory> = Arc::new(crate::ClientFactory::default());
        let hashing = Arc::new(HashManager::new(HashingConfig {
            bcrypt: BcryptConfig {
                rounds: 4,
                verify: true,
                limit: None,
            },
            ..Default::default()
        })?);
        seed_clients(&db, factory.as_ref(), &hashing).await?;
        let clock = Arc::new(FixedClock {
            now: AtomicI64::new(NOW),
            step: AtomicI64::new(0),
        });
        let owner = super::lookup_test_support::scoped_repository(
            db.clone(),
            factory,
            GrantUserKey::Integer,
            Arc::new(FixtureAuthority(db.clone())),
            clock.clone(),
            None,
        )
        .await?;
        let repository = owner.repository.clone();
        let server = scoped_engine(repository.clone(), hashing)?;
        Ok(Self {
            repository,
            owner,
            server,
            verifier: OAuthAccessVerifier::from_pem(PUBLIC)?,
            clock,
        })
    }

    async fn fork_entry(&self) -> TestResult<Self> {
        let container = self.owner.container.clone();
        let scope = Arc::new(container.scope()?);
        let repository = scope.resolve::<DatabaseGrantRepository>().await?;
        let owner = super::lookup_test_support::ScopedRepository {
            repository: repository.clone(),
            scope,
            container,
        };
        let hashing = Arc::new(HashManager::new(HashingConfig {
            bcrypt: BcryptConfig {
                rounds: 4,
                verify: true,
                limit: None,
            },
            ..Default::default()
        })?);
        Ok(Self {
            server: scoped_engine(repository.clone(), hashing)?,
            repository,
            owner,
            verifier: OAuthAccessVerifier::from_pem(PUBLIC)?,
            clock: self.clock.clone(),
        })
    }

    fn set_time(&self, now: i64) {
        self.clock.now.store(now, Ordering::SeqCst);
    }

    async fn code(&self, client: &str, scope: &str) -> TestResult<String> {
        let request = self
            .server
            .prepare_authorization(&authorization(client, scope))
            .await
            .map_err(|_| Error::custom("engine_fixture_prepare_failed", "测试授权准备失败"))?;
        assert_eq!(request.client().get_identifier(), client);
        assert_eq!(
            request.scopes(),
            scope.split(' ').map(str::to_owned).collect::<Vec<_>>()
        );
        let redirect = self
            .server
            .approve(
                request,
                USER,
                GrantAuthorityBinding::new(format!("{BINDING}:{client}"))?,
            )
            .await?;
        let url = url::Url::parse(redirect.location())?;
        assert_eq!(url.host_str(), Some("rp.example.test"));
        assert_eq!(url.path(), "/callback");
        let query = url.query_pairs().into_owned().collect::<Vec<_>>();
        assert!(
            query
                .iter()
                .any(|(name, value)| name == "registered" && value == "yes")
        );
        assert!(
            query
                .iter()
                .any(|(name, value)| name == "state" && value == "state+空格 &")
        );
        query
            .into_iter()
            .find(|(name, _)| name == "code")
            .map(|(_, value)| value)
            .ok_or_else(|| Error::custom("engine_fixture_code_missing", "授权回调缺少code").into())
    }
}

async fn create_schema(db: &Db) -> Result<()> {
    let backend = db.backend()?;
    let id_type = match backend {
        DbBackend::MySql => "CHAR(36)",
        DbBackend::Postgres => "UUID",
        _ => {
            return Err(Error::custom(
                "engine_fixture_backend_invalid",
                "此测试需要真实MySQL或PostgreSQL",
            ));
        }
    };
    let date_type = if backend == DbBackend::MySql {
        "DATETIME"
    } else {
        "TIMESTAMP"
    };
    for sql in [
        format!(
            "CREATE TABLE oauth_clients (id {id_type} PRIMARY KEY, name VARCHAR(80), secret VARCHAR(255), revoked BOOLEAN, redirect_uris TEXT, grant_types TEXT)"
        ),
        format!(
            "CREATE TABLE oauth_auth_codes (id VARCHAR(80) PRIMARY KEY, client_id {id_type}, user_id BIGINT, scopes TEXT, revoked BOOLEAN, expires_at {date_type})"
        ),
        format!(
            "CREATE TABLE oauth_access_tokens (id VARCHAR(80) PRIMARY KEY, client_id {id_type}, user_id BIGINT, scopes TEXT, revoked BOOLEAN, created_at {date_type}, updated_at {date_type}, expires_at {date_type})"
        ),
        format!(
            "CREATE TABLE oauth_refresh_tokens (id VARCHAR(80) PRIMARY KEY, access_token_id VARCHAR(80), revoked BOOLEAN, expires_at {date_type})"
        ),
    ] {
        db.primary_connection()?
            .execute_unprepared(&sql)
            .await
            .map_err(Error::internal)?;
    }
    Ok(())
}

async fn seed_clients(
    db: &Db,
    factory: &dyn ClientModelFactory,
    hashing: &HashManager,
) -> Result<()> {
    let hash = hashing.make(SECRET, &HashOptions::default()).await?;
    assert_ne!(hash, SECRET);
    for client in [
        PUBLIC_CLIENT,
        CONFIDENTIAL_CLIENT,
        CODE_ONLY_CLIENT,
        OTHER_CLIENT,
    ] {
        let secret = if client == CONFIDENTIAL_CLIENT {
            hash.clone().into()
        } else {
            SqlValue::String(None)
        };
        let grants = if client == CODE_ONLY_CLIENT {
            "[\"authorization_code\"]"
        } else {
            "[\"authorization_code\",\"refresh_token\"]"
        };
        db.table("oauth_clients")?
            .insert(vec![
                (
                    "id",
                    factory.identifier_binding(&json!(client), db.backend()?)?,
                ),
                ("name", "engine-fixture".into()),
                ("secret", secret),
                ("revoked", false.into()),
                (
                    "redirect_uris",
                    serde_json::to_string(&[CALLBACK])
                        .map_err(Error::internal)?
                        .into(),
                ),
                ("grant_types", grants.into()),
            ])
            .await?;
    }
    Ok(())
}

fn scopes() -> ScopeRepository {
    ScopeRepository::new(["read", "write", "admin"].map(|id| Scope {
        id: id.into(),
        description: format!("测试{id}权限"),
    }))
}

fn parameters(values: &[(&str, &str)]) -> OAuthParameters {
    OAuthParameters::new(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), json!(value)))
            .collect(),
    )
}

fn authorization(client: &str, scope: &str) -> OAuthParameters {
    parameters(&[
        ("response_type", "code"),
        ("client_id", client),
        ("redirect_uri", CALLBACK),
        ("scope", scope),
        ("state", "state+空格 &"),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
    ])
}

fn exchange(
    client: &str,
    code: &str,
    redirect: &str,
    verifier: &str,
    secret: Option<&str>,
) -> OAuthParameters {
    let mut fields = vec![
        ("grant_type", "authorization_code"),
        ("client_id", client),
        ("code", code),
        ("redirect_uri", redirect),
        ("code_verifier", verifier),
    ];
    if let Some(secret) = secret {
        fields.push(("client_secret", secret));
    }
    parameters(&fields)
}

fn refresh(client: &str, token: &str, scope: Option<&str>) -> OAuthParameters {
    let mut fields = vec![
        ("grant_type", "refresh_token"),
        ("client_id", client),
        ("refresh_token", token),
    ];
    if let Some(scope) = scope {
        fields.push(("scope", scope));
    }
    parameters(&fields)
}

fn assert_oauth_error<T>(result: std::result::Result<T, OAuthError>, expected: &str) -> TestResult {
    let Err(error) = result else {
        return Err(Error::custom("engine_fixture_expected_rejection", "协议反例未拒绝").into());
    };
    assert_eq!(serde_json::to_value(error)?["error"], expected);
    Ok(())
}

async fn check_success_headers(response: TokenResponse) -> TestResult {
    let response = response.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/json; charset=UTF-8"
    );
    assert_eq!(response.headers()[header::PRAGMA], "no-cache");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "no-store, private"
    );
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let json: Value = serde_json::from_slice(&to_bytes(response.into_body(), 16_384).await?)?;
    assert_eq!(json["token_type"], "Bearer");
    assert!(json.get("data").is_none());
    assert!(json.get("id_token").is_none());
    Ok(())
}

async fn public_pkce_and_rotation(fixture: &Fixture) -> TestResult {
    let code = fixture.code(PUBLIC_CLIENT, "read write").await?;
    // 失败请求不能提前消费合法code；随后相同code必须仍可完成交换。
    for (client, redirect, verifier, error) in [
        (
            PUBLIC_CLIENT,
            "https://evil.example/callback",
            VERIFIER,
            "invalid_request",
        ),
        (
            PUBLIC_CLIENT,
            CALLBACK,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "invalid_grant",
        ),
        (OTHER_CLIENT, CALLBACK, VERIFIER, "invalid_request"),
    ] {
        assert_oauth_error(
            fixture
                .server
                .token(&exchange(client, &code, redirect, verifier, None), None)
                .await,
            error,
        )?;
    }
    let response = fixture
        .server
        .token(
            &exchange(PUBLIC_CLIENT, &code, CALLBACK, VERIFIER, None),
            None,
        )
        .await?;
    assert_eq!(response.expires_in(), 3600);
    let claims =
        fixture
            .verifier
            .verify_at(response.access_token(), PUBLIC_CLIENT, u64::try_from(NOW)?)?;
    assert_eq!(claims.sub, USER);
    assert_eq!(claims.scopes, ["read", "write"]);
    assert!(!fixture.repository.is_access_revoked(&claims.jti).await?);
    let original_refresh = response
        .refresh_token()
        .ok_or_else(|| Error::custom("engine_fixture_refresh_missing", "缺少刷新令牌"))?
        .to_owned();
    assert_oauth_error(
        fixture
            .server
            .token(
                &exchange(PUBLIC_CLIENT, &code, CALLBACK, VERIFIER, None),
                None,
            )
            .await,
        "invalid_grant",
    )?;
    check_rotation(fixture, &original_refresh, &claims.jti).await?;
    check_success_headers(response).await
}

async fn check_rotation(
    fixture: &Fixture,
    original_refresh: &str,
    original_jti: &str,
) -> TestResult {
    assert_oauth_error(
        fixture
            .server
            .token(
                &refresh(PUBLIC_CLIENT, original_refresh, Some("read write admin")),
                None,
            )
            .await,
        "invalid_scope",
    )?;
    assert_oauth_error(
        fixture
            .server
            .token(&refresh(OTHER_CLIENT, original_refresh, None), None)
            .await,
        "invalid_grant",
    )?;
    let rotated = fixture
        .server
        .token(
            &refresh(PUBLIC_CLIENT, original_refresh, Some("read")),
            None,
        )
        .await?;
    let next =
        fixture
            .verifier
            .verify_at(rotated.access_token(), PUBLIC_CLIENT, u64::try_from(NOW)?)?;
    assert_eq!(next.scopes, ["read"]);
    assert_ne!(next.jti.as_str(), original_jti);
    assert!(fixture.repository.is_access_revoked(original_jti).await?);
    assert!(!fixture.repository.is_access_revoked(&next.jti).await?);
    assert_oauth_error(
        fixture
            .server
            .token(&refresh(PUBLIC_CLIENT, original_refresh, None), None)
            .await,
        "invalid_grant",
    )?;
    let new_refresh = rotated
        .refresh_token()
        .ok_or_else(|| Error::custom("engine_fixture_refresh_missing", "缺少轮换刷新令牌"))?
        .to_owned();
    assert_ne!(new_refresh.as_str(), original_refresh);
    fixture.set_time(NOW + 7201);
    let expired = fixture
        .server
        .token(&refresh(PUBLIC_CLIENT, &new_refresh, None), None)
        .await;
    fixture.set_time(NOW);
    assert_oauth_error(expired, "invalid_grant")?;
    check_success_headers(rotated).await
}

async fn confidential_hash_and_code_only(fixture: &Fixture) -> TestResult {
    let code = fixture.code(CONFIDENTIAL_CLIENT, "read").await?;
    assert_oauth_error(
        fixture
            .server
            .token(
                &exchange(
                    CONFIDENTIAL_CLIENT,
                    &code,
                    CALLBACK,
                    VERIFIER,
                    Some("wrong-secret"),
                ),
                None,
            )
            .await,
        "invalid_client",
    )?;
    assert_oauth_error(
        fixture
            .server
            .token(
                &exchange(CONFIDENTIAL_CLIENT, &code, CALLBACK, VERIFIER, None),
                None,
            )
            .await,
        "invalid_request",
    )?;
    let response = fixture
        .server
        .token(
            &exchange(CONFIDENTIAL_CLIENT, &code, CALLBACK, VERIFIER, Some(SECRET)),
            None,
        )
        .await?;
    assert!(
        fixture
            .verifier
            .verify_at(
                response.access_token(),
                CONFIDENTIAL_CLIENT,
                u64::try_from(NOW)?
            )
            .is_ok()
    );
    assert!(response.refresh_token().is_some());
    let code = fixture.code(CODE_ONLY_CLIENT, "read").await?;
    let response = fixture
        .server
        .token(
            &exchange(CODE_ONLY_CLIENT, &code, CALLBACK, VERIFIER, None),
            None,
        )
        .await?;
    assert!(response.refresh_token().is_none());
    assert!(
        serde_json::to_value(&response)?
            .get("refresh_token")
            .is_none()
    );
    assert_oauth_error(
        fixture
            .server
            .token(&refresh(CODE_ONLY_CLIENT, "unused", None), None)
            .await,
        "unauthorized_client",
    )?;
    check_success_headers(response).await
}

async fn rejection_boundaries(fixture: &Fixture) -> TestResult {
    let count = fixture
        .repository
        .db
        .table("oauth_auth_codes")?
        .count()
        .await?;
    for (user, binding) in [("8", BINDING), (USER, "untrusted-proof")] {
        let request = fixture
            .server
            .prepare_authorization(&authorization(PUBLIC_CLIENT, "read"))
            .await
            .map_err(|_| Error::custom("engine_fixture_prepare_failed", "测试授权准备失败"))?;
        assert!(
            fixture
                .server
                .approve(request, user, GrantAuthorityBinding::new(binding.into())?)
                .await
                .is_err()
        );
    }
    assert_eq!(
        fixture
            .repository
            .db
            .table("oauth_auth_codes")?
            .count()
            .await?,
        count
    );
    let code = fixture.code(PUBLIC_CLIENT, "read").await?;
    fixture.set_time(NOW + 601);
    let expired = fixture
        .server
        .token(
            &exchange(PUBLIC_CLIENT, &code, CALLBACK, VERIFIER, None),
            None,
        )
        .await;
    fixture.set_time(NOW);
    assert_oauth_error(expired, "invalid_grant")?;
    let no_challenge = parameters(&[
        ("response_type", "code"),
        ("client_id", PUBLIC_CLIENT),
        ("redirect_uri", CALLBACK),
    ]);
    assert!(
        fixture
            .server
            .prepare_authorization(&no_challenge)
            .await
            .is_err()
    );
    let bad_redirect = parameters(&[
        ("response_type", "code"),
        ("client_id", PUBLIC_CLIENT),
        ("redirect_uri", "https://evil.example/callback"),
    ]);
    let failure = match fixture.server.prepare_authorization(&bad_redirect).await {
        Ok(_) => {
            return Err(
                Error::custom("engine_fixture_expected_rejection", "未注册回调未拒绝").into(),
            );
        }
        Err(failure) => failure.into_response(),
    };
    assert!(failure.headers().get(header::LOCATION).is_none());
    assert_eq!(failure.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

async fn delivery_uses_remaining_lifetime(fixture: &Fixture) -> TestResult {
    let code = fixture.code(PUBLIC_CLIENT, "read").await?;
    // 仅此场景让每次权威时钟读取前进一步，模拟准备、事务末检和交付耗时。
    fixture.clock.step.store(1, Ordering::SeqCst);
    let result = fixture
        .server
        .token(
            &exchange(PUBLIC_CLIENT, &code, CALLBACK, VERIFIER, None),
            None,
        )
        .await;
    let delivered_at = fixture.clock.now.load(Ordering::SeqCst) - 1;
    fixture.clock.step.store(0, Ordering::SeqCst);
    fixture.set_time(NOW);
    let response = result?;
    let claims = fixture.verifier.verify_at(
        response.access_token(),
        PUBLIC_CLIENT,
        u64::try_from(delivered_at)?,
    )?;
    assert_eq!(
        response.expires_in(),
        claims.exp - u64::try_from(delivered_at)?
    );
    assert!(response.expires_in() < 3600);
    assert!(response.expires_in() > 0);
    Ok(())
}

#[tokio::test]
async fn real_engine_authorization_pkce_refresh_and_rejection_boundaries() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!(
            "跳过Passport完整engine网络回归：未提供OPENRUST_TEST_DATABASE_URL；此跳过不算真实数据库验收通过"
        );
        return Ok(());
    };
    let database = TestDatabase::create::<EngineMigrator>(&url).await?;
    let result: TestResult = async {
        let fixture = Fixture::new(&database).await?;
        original_code_scopes_are_finalized_only_at_exchange(&fixture).await?;
        public_pkce_and_rotation(&fixture).await?;
        confidential_hash_and_code_only(&fixture).await?;
        rejection_boundaries(&fixture).await?;
        delivery_uses_remaining_lifetime(&fixture).await
    }
    .await;
    // 无论正常失败结果如何，先关闭隔离库；保留原始失败而不把清理当成功。
    let cleanup = database.close().await;
    result?;
    cleanup?;
    Ok(())
}

async fn original_code_scopes_are_finalized_only_at_exchange(fixture: &Fixture) -> TestResult {
    let code = fixture.code(PUBLIC_CLIENT, "read *").await?;
    let row = fixture
        .repository
        .db
        .table("oauth_auth_codes")?
        .first()
        .await?
        .ok_or_else(|| Error::custom("engine_fixture_code_missing", "测试授权码未持久化"))?;
    let raw = attributes_from_row(&row)?;
    assert_eq!(
        raw.get("scopes")
            .and_then(Value::as_str)
            .map(serde_json::from_str::<Vec<String>>)
            .transpose()?,
        Some(vec!["read".to_owned(), "*".to_owned()])
    );
    let response = fixture
        .server
        .token(
            &exchange(PUBLIC_CLIENT, &code, CALLBACK, VERIFIER, None),
            None,
        )
        .await?;
    let claims =
        fixture
            .verifier
            .verify_at(response.access_token(), PUBLIC_CLIENT, u64::try_from(NOW)?)?;
    assert_eq!(claims.scopes, vec!["read"]);
    Ok(())
}

fn redirect_array_request(target: Option<&str>, malformed_scope: bool) -> OAuthParameters {
    let mut values = serde_json::Map::from_iter([
        ("response_type".into(), json!("code")),
        ("client_id".into(), json!(PUBLIC_CLIENT)),
        ("scope".into(), json!("read")),
        ("code_challenge".into(), json!(CHALLENGE)),
        ("code_challenge_method".into(), json!("S256")),
    ]);
    if let Some(target) = target {
        values.insert("redirect_uri".into(), json!(target));
    }
    if malformed_scope {
        values.insert("scope".into(), json!(["read"]));
    }
    OAuthParameters::new(values)
}

async fn redirect_array_local_failure(
    fixture: &Fixture,
    request: OAuthParameters,
    expected: &str,
    status: StatusCode,
) -> TestResult {
    let response = match fixture.server.prepare_authorization(&request).await {
        Ok(_) => {
            return Err(
                Error::custom("redirect_array_expected_rejection", "错误注册未按预期拒绝").into(),
            );
        }
        Err(error) => error.into_response(),
    };
    assert_eq!(response.status(), status);
    assert!(response.headers().get(header::LOCATION).is_none());
    let bytes = to_bytes(response.into_body(), 16_384).await?;
    let body: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["error"], expected);
    assert!(!String::from_utf8_lossy(&bytes).contains("rp.example.test"));
    Ok(())
}

#[tokio::test]
async fn real_engine_preserves_redirect_array_keys_types_and_failure_order() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!(
            "未执行redirect-array真实engine回归：缺少隔离MySQL/PostgreSQL测试URL，此跳过不算通过"
        );
        return Ok(());
    };
    let database = TestDatabase::create::<EngineMigrator>(&url).await?;
    let result: TestResult = async {
        let fixture = Fixture::new(&database).await?;
        let db = &fixture.repository.db;
        let binding = fixture
            .repository
            .factory
            .identifier_binding(&json!(PUBLIC_CLIENT), db.backend()?)?;
        for (registration, target, malformed_scope, expected) in [
            (json!([false, 1, CALLBACK]), Some(CALLBACK), false, None),
            (json!({"foo":CALLBACK}), Some(CALLBACK), false, None),
            (
                json!({"foo":CALLBACK}),
                None,
                false,
                Some(("server_error", StatusCode::INTERNAL_SERVER_ERROR)),
            ),
            // scope参数解析先于省略默认键0读取；不能被malformed registration抢走错误。
            (
                json!({"foo":CALLBACK}),
                None,
                true,
                Some(("invalid_request", StatusCode::BAD_REQUEST)),
            ),
            (json!({"0":CALLBACK}), None, false, None),
            (
                json!([]),
                None,
                false,
                Some(("invalid_client", StatusCode::UNAUTHORIZED)),
            ),
            (
                json!([false, "http://127.0.0.1:8000/callback"]),
                Some("http://127.0.0.1:9000/callback"),
                false,
                Some(("server_error", StatusCode::INTERNAL_SERVER_ERROR)),
            ),
            (
                json!(["http://127.0.0.1:8000/callback", false]),
                Some("http://127.0.0.1:9000/callback"),
                false,
                None,
            ),
            (
                json!([true, CALLBACK]),
                Some("https://RP.example.test/callback?registered=yes"),
                false,
                Some(("invalid_client", StatusCode::UNAUTHORIZED)),
            ),
        ] {
            db.table("oauth_clients")?
                .where_eq("id", binding.clone())?
                .update(vec![(
                    "redirect_uris",
                    serde_json::to_string(&registration)?.into(),
                )])
                .await?;
            let entry = fixture.fork_entry().await?;
            let request = redirect_array_request(target, malformed_scope);
            match expected {
                Some((error, status)) => {
                    redirect_array_local_failure(&entry, request, error, status).await?;
                }
                None => {
                    entry
                        .server
                        .prepare_authorization(&request)
                        .await
                        .map_err(|_| {
                            Error::custom(
                                "redirect_array_expected_acceptance",
                                "合法数组注册被错误拒绝",
                            )
                        })?;
                }
            }
        }
        Ok(())
    }
    .await;
    let cleanup = database.close().await;
    result?;
    cleanup?;
    Ok(())
}

fn lazy_method_request(client: &str, challenge: Option<Value>, method: Value) -> OAuthParameters {
    let mut fields = serde_json::Map::from_iter([
        ("response_type".into(), json!("code")),
        ("client_id".into(), json!(client)),
        ("redirect_uri".into(), json!(CALLBACK)),
        ("scope".into(), json!("read")),
        ("code_challenge_method".into(), method),
    ]);
    if let Some(challenge) = challenge {
        fields.insert("code_challenge".into(), challenge);
    }
    OAuthParameters::new(fields)
}

async fn assert_lazy_method_failure(
    fixture: &Fixture,
    request: OAuthParameters,
    expected_hint: &str,
) -> TestResult {
    let failure = match fixture.server.prepare_authorization(&request).await {
        Ok(_) => return Err("PKCE准备反例必须拒绝".into()),
        Err(failure) => failure.into_response(),
    };
    assert_eq!(failure.status(), StatusCode::BAD_REQUEST);
    assert!(failure.headers().get(header::LOCATION).is_none());
    let body: Value = serde_json::from_slice(&to_bytes(failure.into_body(), 16_384).await?)?;
    assert_eq!(body["error"], "invalid_request");
    let hint = body["hint"].as_str().ok_or("参数错误缺少hint")?;
    assert_eq!(hint, expected_hint);
    Ok(())
}

async fn lazy_method_matrix(fixture: &Fixture) -> TestResult {
    for method in [json!(["S256"]), json!({"method":"S256"})] {
        for challenge in [
            None,
            Some(Value::Null),
            Some(json!("")),
            Some(json!(" \t\r\n\0")),
        ] {
            let request =
                lazy_method_request(CONFIDENTIAL_CLIENT, challenge.clone(), method.clone());
            let prepared = fixture
                .server
                .prepare_authorization(&request)
                .await
                .map_err(|_| "保密客户端无challenge必须忽略异质method")?;
            // 成功prepare证明异质method被忽略；这里只核公开展示事实，不读取内部PKCE状态。
            assert_eq!(prepared.scopes(), ["read"]);
            assert_eq!(prepared.client().get_identifier(), CONFIDENTIAL_CLIENT);
            assert_lazy_method_failure(
                fixture,
                lazy_method_request(PUBLIC_CLIENT, challenge, method.clone()),
                "Code challenge must be provided for public clients",
            )
            .await?;
        }
        for client in [PUBLIC_CLIENT, CONFIDENTIAL_CLIENT] {
            assert_lazy_method_failure(
                fixture,
                lazy_method_request(client, Some(json!(CHALLENGE)), method.clone()),
                "Check the `code_challenge_method` parameter",
            )
            .await?;
            assert_lazy_method_failure(
                fixture,
                lazy_method_request(client, Some(json!([CHALLENGE])), method.clone()),
                "Check the `code_challenge` parameter",
            )
            .await?;
        }
    }
    // 先于PKCE的state/scope参数失败不得因懒读取而后移。
    for parameter in ["state", "scope"] {
        let mut fields = serde_json::Map::from_iter([
            ("response_type".into(), json!("code")),
            ("client_id".into(), json!(PUBLIC_CLIENT)),
            ("redirect_uri".into(), json!(CALLBACK)),
            ("scope".into(), json!("read")),
            ("code_challenge".into(), json!([CHALLENGE])),
            ("code_challenge_method".into(), json!(["S256"])),
        ]);
        fields.insert(parameter.into(), json!(["invalid"]));
        assert_lazy_method_failure(
            fixture,
            OAuthParameters::new(fields),
            &format!("Check the `{parameter}` parameter"),
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn real_prepare_reads_pkce_method_only_when_challenge_is_present() -> TestResult {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过PKCE method准备链回归：缺少OPENRUST_TEST_DATABASE_URL；不算真实数据库通过");
        return Ok(());
    };
    let database = TestDatabase::create::<EngineMigrator>(&url).await?;
    let result: TestResult = Box::pin(async {
        let fixture = Fixture::new(&database).await?;
        let before = fixture
            .repository
            .db
            .table("oauth_auth_codes")?
            .count()
            .await?;
        Box::pin(lazy_method_matrix(&fixture)).await?;
        assert_eq!(
            fixture
                .repository
                .db
                .table("oauth_auth_codes")?
                .count()
                .await?,
            before
        );
        Ok(())
    })
    .await;
    let cleanup = database.close().await;
    result?;
    cleanup?;
    Ok(())
}
